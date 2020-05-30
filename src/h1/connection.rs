use super::task::{RecvBody, RecvRes, SendBody, SendReq, Seq};
use super::Inner;
use super::State;
use super::{AsyncRead, AsyncWrite};
use futures_util::ready;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll, Waker};

pub struct Connection<S> {
    io: S,
    inner: Weak<Mutex<Inner>>,
}

impl<S> Connection<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    pub(crate) fn new(io: S, inner: &Arc<Mutex<Inner>>) -> Self {
        Connection {
            io,
            inner: Arc::downgrade(inner),
        }
    }

    fn poll_try_recv_res(
        &mut self,
        inner: &mut Inner,
        cur_seq: Seq,
        last_task_waker: &mut Option<Waker>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<bool, io::Error>> {
        if let Some(task) = inner.tasks.get_recv_res(cur_seq) {
            *last_task_waker = Some(task.task_waker.clone());
            if task.end {
                return Ok(true).into();
            }
            if let Err(e) = ready!(task.poll_connection(cx, &mut self.io)) {
                // If the remote side abruptly closes the connection after
                // sending the response header, we might have a whole response
                if task.end {
                    inner.state = State::Closed;
                    return Ok(true).into();
                } else {
                    return Err(e).into();
                }
            }
            if task.end {
                // we got a complete response. this means we will
                // transition away from this state, either to
                // SendBody (if there are body chunks left to send)
                // or RecvBody).
                return Ok(true).into();
            }
        }
        Ok(false).into()
    }

    fn poll_drive(
        &mut self,
        inner: &mut Inner,
        last_task_waker: &mut Option<Waker>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            let cur_seq = Seq(inner.cur_seq);

            // prune before getting any task for the state, to avoid getting stale.
            inner.tasks.prune_completed();

            trace!("poll_drive in state: {:?}", inner.state);

            match inner.state {
                State::Ready => {
                    if let Some(task) = inner.tasks.get_send_req(cur_seq) {
                        *last_task_waker = None;
                        ready!(task.poll_connection(cx, &mut self.io))?;
                        if task.info.complete {
                            if task.end {
                                // no body to send
                                inner.state = State::Waiting;
                            } else {
                                inner.state = State::SendBodyAndWaiting;
                            }
                        }
                    } else {
                        break Poll::Pending;
                    }
                }
                State::SendBodyAndWaiting => {
                    // We might receive the response while still sending the body.
                    // This could happen on Expect-100 or 307/308 redirects.
                    // We will drive both the RecvRes task and the SendBody task
                    // at the same time.

                    let has_response =
                        match self.poll_try_recv_res(inner, cur_seq, last_task_waker, cx) {
                            Poll::Pending => false,
                            Poll::Ready(Ok(v)) => v,
                            Poll::Ready(Err(e)) => return Err(e).into(),
                        };

                    if has_response && inner.state == State::SendBodyAndWaiting {
                        inner.state = State::SendBody;
                        continue;
                    }

                    if let Some(task) = inner.tasks.get_send_body(cur_seq) {
                        *last_task_waker = task.task_waker.clone();
                        ready!(task.poll_connection(cx, &mut self.io))?;
                        if task.info.complete && task.end {
                            // send body chunks is done, just wait for response
                            inner.state = State::Waiting;
                        }
                    } else {
                        return Poll::Pending;
                    }
                }
                State::SendBody => {
                    if let Some(task) = inner.tasks.get_send_body(cur_seq) {
                        *last_task_waker = task.task_waker.clone();
                        ready!(task.poll_connection(cx, &mut self.io))?;
                        if task.info.complete && task.end {
                            // send body is done, and we already got a response
                            inner.state = State::RecvBody;
                        }
                    } else {
                        return Poll::Pending;
                    }
                }
                State::Waiting => {
                    let has_response =
                        match ready!(self.poll_try_recv_res(inner, cur_seq, last_task_waker, cx)) {
                            Ok(v) => v,
                            Err(e) => return Err(e).into(),
                        };
                    if has_response {
                        // we got a response, and send body is done
                        inner.state = State::RecvBody;
                    } else {
                        return Poll::Pending;
                    }
                }
                State::RecvBody => {
                    if let Some(task) = inner.tasks.get_recv_body(cur_seq) {
                        *last_task_waker = Some(task.task_waker.clone());
                        ready!(task.poll_connection(cx, &mut self.io))?;
                        if task.end {
                            if task.reuse_conn {
                                inner.cur_seq += 1;
                                trace!("New cur_seq: {}", inner.cur_seq);
                                inner.state = State::Ready;
                            } else {
                                inner.state = State::Closed;
                            }
                        }
                    } else {
                        return Poll::Pending;
                    }
                }
                State::Closed => {
                    if let Some(e) = inner.error.as_ref() {
                        trace!("Connection closed with error");
                        // connection gets a fake copy while the client gets the original
                        let fake = io::Error::new(e.kind(), e.to_string());
                        return Err(fake).into();
                    } else {
                        trace!("Connection closed");
                        return Ok(()).into();
                    }
                }
            }
        }
    }
}

impl<S> Future for Connection<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        let arc_mutex = match this.inner.upgrade() {
            None => {
                // all handles to operate on this connection are gone.
                // TODO preserve connection errors that are gone with Inner being dropped.
                return Ok(()).into();
            }
            Some(v) => v,
        };

        let mut inner = arc_mutex.lock().unwrap();

        inner.conn_waker = Some(cx.waker().clone());

        let mut last_task_waker = None;

        if let Err(err) = ready!(this.poll_drive(&mut *inner, &mut last_task_waker, cx)) {
            inner.error = Some(err);
            inner.state = State::Closed;
            if let Some(waker) = last_task_waker.take() {
                waker.wake();
            }
        }

        Ok(()).into()
    }
}

trait ConnectionPoll {
    fn poll_connection<S>(&mut self, cx: &mut Context, io: &mut S) -> Poll<io::Result<()>>
    where
        S: AsyncRead + AsyncWrite + Unpin;
}

impl ConnectionPoll for SendReq {
    fn poll_connection<S>(&mut self, cx: &mut Context, io: &mut S) -> Poll<io::Result<()>>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        loop {
            let amount = ready!(Pin::new(&mut *io).poll_write(cx, &self.req[..]))?;
            if amount < self.req.len() {
                self.req = self.req.split_off(amount);
                continue;
            }
            break;
        }

        self.info.complete = true;

        Ok(()).into()
    }
}

impl ConnectionPoll for SendBody {
    fn poll_connection<S>(&mut self, cx: &mut Context, io: &mut S) -> Poll<io::Result<()>>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        loop {
            if self.body.is_empty() {
                break;
            }
            let amount = ready!(Pin::new(&mut *io).poll_write(cx, &self.body[..]))?;
            self.body = self.body.split_off(amount);
        }

        // post sending body, flush
        ready!(Pin::new(&mut *io).poll_flush(cx))?;

        // entire current send_body was sent, waker is for a
        // someone potentially waiting to send more.
        if let Some(waker) = self.task_waker.take() {
            waker.wake();
        }

        self.info.complete = true;

        Ok(()).into()
    }
}

impl ConnectionPoll for RecvRes {
    fn poll_connection<S>(&mut self, cx: &mut Context, io: &mut S) -> Poll<io::Result<()>>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        const END_OF_HEADER: &[u8] = &[b'\r', b'\n', b'\r', b'\n'];
        let mut end_index = 0;
        let mut buf_index = 0;
        let mut one = [0_u8; 1];

        // fix so end_index is where it needs to be
        loop {
            if buf_index == self.buf.len() {
                break;
            }
            if self.buf[buf_index] == END_OF_HEADER[end_index] {
                end_index += 1;
            } else if end_index > 0 {
                end_index = 0;
            }
            buf_index += 1;
        }

        loop {
            if buf_index == self.buf.len() {
                // read one more char
                let amount = ready!(Pin::new(&mut &mut *io).poll_read(cx, &mut one[..]))?;
                if amount == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "EOF before complete http11 header",
                    ))
                    .into();
                }
                self.buf.push(one[0]);
            }

            if self.buf[buf_index] == END_OF_HEADER[end_index] {
                end_index += 1;
            } else if end_index > 0 {
                end_index = 0;
            }

            if end_index == END_OF_HEADER.len() {
                // we found the end of header sequence
                break;
            }
            buf_index += 1;
        }

        // in theory we're now have a complete header ending \r\n\r\n
        self.end = true;
        self.task_waker.wake_by_ref();

        Ok(()).into()
    }
}

impl ConnectionPoll for RecvBody {
    fn poll_connection<S>(&mut self, cx: &mut Context, io: &mut S) -> Poll<io::Result<()>>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let cur_len = self.buf.len();
        self.buf.resize(self.read_max, 0);
        if cur_len == self.read_max && self.read_max > 0 {
            self.task_waker.wake_by_ref();
            return Poll::Pending;
        }

        let amount = if self.read_max == 0 {
            // ContentLengthRead does a read for "content-length: 0", to
            // ensure the connection is in correct state for the next req.
            0
        } else {
            let read = Pin::new(&mut *io).poll_read(cx, &mut self.buf[cur_len..]);
            if let Poll::Pending = read {
                self.buf.resize(cur_len, 0);
            }
            let amount = ready!(read)?;
            self.buf.resize(cur_len + amount, 0);
            amount
        };

        trace!(
            "RecvBody read_max: {} amount: {} buf size: {}",
            self.read_max,
            amount,
            self.buf.len(),
        );

        if amount == 0 {
            self.end = true;
        }

        self.task_waker.clone().wake();
        Ok(()).into()
    }
}
