mod chunked;
mod connection;
mod error;
mod http11;
mod limit;
mod task;

use connection::Connection;
pub use error::Error;
pub(crate) use futures_io::{AsyncRead, AsyncWrite};
use futures_util::future::poll_fn;
use futures_util::ready;
use limit::{LimitRead, LimitWrite};
use std::future::Future;
use std::io;
use std::mem;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use task::{RecvBody, RecvRes, SendBody, SendReq, Seq, Task, Tasks};

pub fn handshake<S>(io: S) -> (SendRequest, Connection<S>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let inner = Arc::new(Mutex::new(Inner::new()));
    let conn = Connection::new(io, &inner);
    let send_req = SendRequest::new(inner);
    (send_req, conn)
}

#[derive(Clone)]
pub struct SendRequest {
    inner: Arc<Mutex<Inner>>,
}

impl SendRequest {
    fn new(inner: Arc<Mutex<Inner>>) -> Self {
        SendRequest { inner }
    }

    pub fn send_request(
        &mut self,
        req: http::Request<()>,
        end: bool,
    ) -> Result<(ResponseFuture, SendStream), Error> {
        let seq = {
            let mut inner = self.inner.lock().unwrap();
            let seq = Seq(inner.next_seq);
            inner.next_seq += 1;
            let task = SendReq::from_request(seq, &req, end)?;
            inner.enqueue(task)?;
            seq
        };
        let fut_response = ResponseFuture::new(self.inner.clone(), seq);
        let limiter = LimitWrite::from_request(&req);
        let send_stream = SendStream::new(self.inner.clone(), seq, limiter);
        Ok((fut_response, send_stream))
    }
}

pub struct ResponseFuture {
    inner: Arc<Mutex<Inner>>,
    seq: Seq,
}

impl ResponseFuture {
    fn new(inner: Arc<Mutex<Inner>>, seq: Seq) -> Self {
        ResponseFuture { inner, seq }
    }
}

impl Future for ResponseFuture {
    type Output = Result<http::Response<RecvStream>, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.inner.lock().unwrap();

        // Despite any error, we might have a complete response. This happens
        // when a server sends a full response header and then closes the
        // connection straight after.
        if let Some(task) = inner.tasks.get_recv_res(self.seq) {
            let res = task.try_parse()?;
            if let Some(res) = res {
                let limiter = LimitRead::from_response(&res);
                let recv_stream = RecvStream::new(self.inner.clone(), self.seq, limiter);
                let (parts, _) = res.into_parts();
                task.info.complete = true;
                Ok(http::Response::from_parts(parts, recv_stream)).into()
            } else {
                task.task_waker = cx.waker().clone();
                Poll::Pending
            }
        } else if let Some(err) = inner.get_remote_error() {
            Err(err).into()
        } else {
            let task = RecvRes::new(self.seq, cx.waker().clone());
            inner.enqueue(task)?;
            Poll::Pending
        }
    }
}

pub struct SendStream {
    inner: Arc<Mutex<Inner>>,
    seq: Seq,
    limiter: LimitWrite,
}

impl SendStream {
    fn new(inner: Arc<Mutex<Inner>>, seq: Seq, limiter: LimitWrite) -> Self {
        SendStream {
            inner,
            seq,
            limiter,
        }
    }

    fn poll_can_send_data(&self, cx: &mut Context) -> Poll<Result<(), Error>> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(err) = inner.get_remote_error() {
            return Err(err).into();
        }
        if let Some(err) = inner.assert_can_send_body(self.seq) {
            return Err(err).into();
        }
        if let Some(task) = inner.tasks.get_send_body(self.seq) {
            task.task_waker.replace(cx.waker().clone());
            Poll::Pending
        } else {
            Ok(()).into()
        }
    }

    pub async fn ready(self) -> Result<SendStream, Error> {
        poll_fn(|cx| self.poll_can_send_data(cx)).await?;
        Ok(self)
    }

    pub fn send_data(&mut self, data: &[u8], end: bool) -> Result<(), Error> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(err) = inner.assert_can_send_body(self.seq) {
            return Err(err);
        }
        let mut out = Vec::with_capacity(data.len() + self.limiter.overhead());
        self.limiter.write(data, &mut out)?;
        if end {
            self.limiter.finish(&mut out)?;
        }
        let task = SendBody::new(self.seq, out, end);
        inner.enqueue(task)?;
        Ok(())
    }
}

pub struct RecvStream {
    inner: Arc<Mutex<Inner>>,
    seq: Seq,
    limiter: LimitRead,
    finished: bool,
}

impl RecvStream {
    fn new(inner: Arc<Mutex<Inner>>, seq: Seq, limiter: LimitRead) -> Self {
        Self {
            inner,
            seq,
            limiter,
            finished: false,
        }
    }

    pub fn poll_read(&mut self, cx: &mut Context, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        if self.finished {
            return Ok(0).into();
        }
        let mut reader = RecvReader::new(
            self.inner.clone(),
            self.seq,
            self.limiter.is_reusable_conn(),
        );
        let amount = ready!(self.limiter.poll_read(cx, &mut reader, buf))?;
        if amount == 0 {
            self.finished = true;
        }
        Ok(amount).into()
    }

    #[allow(dead_code)]
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        Ok(poll_fn(|cx| self.poll_read(cx, buf)).await?)
    }

    #[allow(dead_code)]
    pub fn is_end(&self) -> bool {
        self.finished
    }
}

pub(crate) struct RecvReader {
    inner: Arc<Mutex<Inner>>,
    seq: Seq,
    reuse_conn: bool,
}

impl RecvReader {
    fn new(inner: Arc<Mutex<Inner>>, seq: Seq, reuse_conn: bool) -> Self {
        RecvReader {
            inner,
            seq,
            reuse_conn,
        }
    }

    pub fn poll_read(&self, cx: &mut Context, out: &mut [u8]) -> Poll<io::Result<usize>> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(task) = inner.tasks.get_recv_body(self.seq) {
            task.task_waker = cx.waker().clone();
            let buf = &mut task.buf;
            if buf.is_empty() {
                if task.end {
                    task.read_max = 0;
                    Ok(0).into()
                } else {
                    task.read_max = out.len();
                    inner.try_wake_conn();
                    Poll::Pending
                }
            } else {
                let max = buf.len().min(out.len());
                (&mut out[0..max]).copy_from_slice(&buf[0..max]);
                if max == buf.len() {
                    // all content was used, up, reuse buffer
                    task.read_max = 0;
                    buf.resize(0, 0);
                } else {
                    task.buf = buf.split_off(max);
                    task.read_max = out.len() - task.buf.len();
                }
                Ok(max).into()
            }
        } else {
            let mut task = RecvBody::new(self.seq, self.reuse_conn, cx.waker().clone());
            task.read_max = out.len();
            inner.enqueue(task)?;
            Poll::Pending
        }
    }

    // pub async fn read(&self, buf: &mut [u8]) -> Result<usize, Error> {
    //     poll_fn(|cx| self.poll_read(cx, buf)).await
    // }
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum State {
    /// Can accept a new request.
    Ready,
    /// After request header is sent, and we can send a body.
    SendBody,
    /// Waiting to receive response header.
    Waiting,
    /// After we received response header and are waiting for a body.
    RecvBody,
    /// If connection failed.
    Closed,
}

#[derive(Debug)]
pub(crate) struct Inner {
    next_seq: usize,
    cur_seq: usize,
    state: State,
    error: Option<io::Error>,
    tasks: Tasks,
    conn_waker: Option<Waker>,
}

impl Inner {
    fn new() -> Self {
        Inner {
            next_seq: 0,
            cur_seq: 0,
            state: State::Ready,
            error: None,
            tasks: Tasks::new(),
            conn_waker: None,
        }
    }

    fn enqueue<T: Into<Task>>(&mut self, task: T) -> io::Result<()> {
        if self.state == State::Closed {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Connection is closed",
            ));
        }
        self.tasks.push(task.into());
        self.try_wake_conn();
        Ok(())
    }

    fn try_wake_conn(&mut self) {
        if let Some(waker) = self.conn_waker.take() {
            waker.wake();
        }
    }

    fn get_remote_error(&mut self) -> Option<Error> {
        if let State::Closed = &mut self.state {
            if let Some(e) = &mut self.error {
                // first ever to do this, gets the original io error
                // after that it will be fake copies.
                let fake = io::Error::new(e.kind(), e.to_string());
                let orig = mem::replace(e, fake);
                return Some(Error::Io(orig));
            }
        }
        None
    }

    fn assert_can_send_body(&mut self, seq: Seq) -> Option<Error> {
        if self.cur_seq > *seq {
            return Some(Error::User("Can't send body for old request".into()));
        }
        if self.cur_seq == *seq {
            match self.state {
                State::Ready => {
                    if self.tasks.get_send_req(seq).is_none() {
                        panic!("Send body in state Waiting without a send_req");
                    }
                }
                State::SendBody => {
                    // we are expecting more body parts
                }
                _ => {
                    let message = format!("Can't send body in state: {:?}", self.state);
                    return Some(Error::User(message));
                }
            }
        }
        None
    }
}
