use crate::body::BodyImpl;
use crate::h1;
use crate::h1::SendRequest as H1SendRequest;
use crate::reqb_ext::RequestParams;
use crate::res_ext::HeaderMapExt;
use crate::uri_ext::HostPort;
use crate::uri_ext::MethodExt;
use crate::Body;
use crate::Error;
use bytes::Bytes;
use futures_util::future::poll_fn;
use futures_util::ready;
use hreq_h2 as h2;
use hreq_h2::client::SendRequest as H2SendRequest;
use once_cell::sync::Lazy;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

static ID_COUNTER: Lazy<AtomicUsize> = Lazy::new(|| AtomicUsize::new(0));
const BUF_SIZE: usize = 16_384;

#[derive(Clone)]
pub enum ProtocolImpl {
    Http1(H1SendRequest),
    Http2(H2SendRequest<Bytes>),
}

impl fmt::Display for ProtocolImpl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolImpl::Http1(_) => write!(f, "Http1"),
            ProtocolImpl::Http2(_) => write!(f, "Http2"),
        }
    }
}

// #[derive(Clone)]
pub struct Connection {
    id: usize,
    host_port: HostPort<'static>,
    p: ProtocolImpl,
    unfinished_reqs: Arc<()>,
}

impl PartialEq for Connection {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for Connection {}

impl Connection {
    pub(crate) fn new(host_port: HostPort<'static>, p: ProtocolImpl) -> Self {
        Connection {
            id: ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            host_port,
            p,
            unfinished_reqs: Arc::new(()),
        }
    }

    pub(crate) fn id(&self) -> usize {
        self.id
    }

    pub(crate) fn host_port(&self) -> &HostPort<'static> {
        &self.host_port
    }

    pub(crate) fn is_http2(&self) -> bool {
        match self.p {
            ProtocolImpl::Http1(_) => false,
            ProtocolImpl::Http2(_) => true,
        }
    }

    pub(crate) fn unfinished_requests(&self) -> usize {
        Arc::strong_count(&self.unfinished_reqs) - 1 // -1 for self
    }

    pub async fn send_request(
        &mut self,
        req: http::Request<Body>,
    ) -> Result<http::Response<Body>, Error> {
        // up the arc-counter on unfinished reqs
        let unfin = self.unfinished_reqs.clone();

        let (mut parts, mut body) = req.into_parts();

        let params = parts.extensions.get::<RequestParams>().unwrap();
        let deadline = params.deadline();

        // resolve deferred body codecs because content-encoding and content-type are settled.
        body.configure(params.clone(), &parts.headers, false);

        if let Some(len) = body.content_encoded_length() {
            // the body indicates a length (for sure).
            // we don't want to set content-length: 0 unless we know it's
            // a method that really has a body. also we never override
            // a user set content-length header.
            let user_set_length = parts.headers.get("content-length").is_some();

            if !user_set_length && (len > 0 || parts.method.indicates_body()) {
                parts.headers.set("content-length", len.to_string());
            }
        } else if !self.is_http2() && parts.method.indicates_body() {
            // body does not indicate a length (like from a reader),
            // and method indicates there really is one.
            // we chose chunked.
            if parts.headers.get("transfer-encoding").is_none() {
                parts.headers.set("transfer-encoding", "chunked");
            }
        }

        if parts.headers.get("user-agent").is_none() {
            // TODO this could be created once for the entire lifetime of the library
            let agent = format!("rust/hreq/{}", crate::VERSION);
            parts.headers.set("user-agent", agent);
        }

        if parts.headers.get("accept").is_none() {
            parts.headers.set("accept", "*/*");
        }

        if parts.headers.get("content-type").is_none() {
            if let Some(ctype) = body.content_type() {
                parts.headers.set("content-type", ctype);
            }
        }

        let req = http::Request::from_parts(parts, body);

        trace!(
            "{} {} {} {} {:?}",
            self.p,
            self.host_port(),
            req.method(),
            req.uri(),
            req.headers()
        );

        // send request against a deadline
        let response = deadline.race(send_req(&self.p, req, unfin)).await?;

        Ok(response)
    }
}

async fn send_req(
    proto: &ProtocolImpl,
    req: http::Request<Body>,
    unfin: Arc<()>,
) -> Result<http::Response<Body>, Error> {
    let params = req.extensions().get::<RequestParams>().unwrap().clone();

    let (parts, mut body_read) = req.into_parts();
    let req = http::Request::from_parts(parts, ());

    let no_body = body_read.is_definitely_no_body();

    let (mut res_fut, mut body_send) = proto.do_send(req, no_body).await?;
    let mut early_response = None;

    if !no_body {
        // this buffer must be less than h2 window size
        let mut buf = vec![0_u8; BUF_SIZE];

        loop {
            match TryOnceFuture(&mut res_fut).await {
                TryOnce::Pending => {
                    // early response did not happen, keep sending body
                }
                TryOnce::Ready(v) => {
                    early_response = Some(v);
                    break;
                }
            }

            // wait for body_send to be able to receive more data
            body_send = body_send.ready().await?;

            // read more data to send
            let amount_read = body_read.read(&mut buf[..]).await?;
            if amount_read == 0 {
                break;
            }

            body_send.send_data(&buf[0..amount_read]).await?;
        }

        body_send.send_end()?;
    }

    let (mut parts, mut res_body) = if let Some(res) = early_response {
        res?
    } else {
        res_fut.await?
    };

    parts.extensions.insert(params.clone());
    res_body.set_unfinished_recs(unfin);
    res_body.configure(params, &parts.headers, true);

    Ok(http::Response::from_parts(parts, res_body))
}

impl ProtocolImpl {
    // Generalised sending of request
    async fn do_send(
        &self,
        req: http::Request<()>,
        no_body: bool,
    ) -> Result<(ResponseFuture, BodySender), Error> {
        Ok(match self {
            ProtocolImpl::Http1(h1) => {
                let mut h1 = h1.clone();
                let (fut, send_body) = h1.send_request(req, no_body)?;
                (ResponseFuture::H1(fut), BodySender::H1(send_body))
            }
            ProtocolImpl::Http2(h2) => {
                let mut h2 = h2.clone().ready().await?;
                let (fut, send_body) = h2.send_request(req, no_body)?;
                (ResponseFuture::H2(fut), BodySender::H2(send_body))
            }
        })
    }
}

/// Generalisation over response future
enum ResponseFuture {
    H1(h1::ResponseFuture),
    H2(h2::client::ResponseFuture),
}

impl Future for ResponseFuture {
    type Output = Result<(http::response::Parts, Body), Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this {
            ResponseFuture::H1(f) => {
                let (p, b) = ready!(Pin::new(f).poll(cx))?.into_parts();
                let b = Body::new(BodyImpl::Http1(b), None);
                Ok((p, b)).into()
            }
            ResponseFuture::H2(f) => {
                let (p, b) = ready!(Pin::new(f).poll(cx))?.into_parts();
                let b = Body::new(BodyImpl::Http2(b), None);
                Ok((p, b)).into()
            }
        }
    }
}

/// Generalisation over sending body request data
enum BodySender {
    H1(h1::SendStream),
    H2(h2::SendStream<Bytes>),
}

impl BodySender {
    async fn ready(self) -> Result<Self, Error> {
        match self {
            BodySender::H1(s) => Ok(BodySender::H1(s.ready().await?)),
            BodySender::H2(s) => Ok(BodySender::H2(s)),
        }
    }

    async fn send_data(&mut self, mut buf: &[u8]) -> Result<(), Error> {
        match self {
            BodySender::H1(s) => Ok(s.send_data(buf, false)?),
            BodySender::H2(s) => {
                loop {
                    if buf.len() == 0 {
                        break;
                    }

                    s.reserve_capacity(buf.len());

                    let actual_capacity = {
                        let cur = s.capacity();
                        if cur > 0 {
                            cur
                        } else {
                            poll_fn(|cx| s.poll_capacity(cx)).await.ok_or_else(|| {
                                Error::Proto("Stream gone before capacity".into())
                            })??
                        }
                    };

                    // h2::SendStream lacks a sync or async function that allows us
                    // to send borrowed data. This copy is unfortunate.
                    // TODO contact h2 and ask if they would consider some kind of
                    // async variant that takes a &mut [u8].
                    let data = Bytes::copy_from_slice(&buf[..actual_capacity]);

                    s.send_data(data, false)?;

                    buf = &buf[actual_capacity..];
                }

                Ok(())
            }
        }
    }

    fn send_end(&mut self) -> Result<(), Error> {
        match self {
            BodySender::H1(s) => Ok(s.send_data(&[], true)?),
            BodySender::H2(s) => Ok(s.send_data(Bytes::new(), true)?),
        }
    }
}

/// When polling the wrapped future will never go Poll::Pending.
struct TryOnceFuture<F>(F);

impl<F> Future for TryOnceFuture<F>
where
    Self: Unpin,
    F: Future + Unpin,
{
    type Output = TryOnce<F>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match Pin::new(&mut this.0).poll(cx) {
            Poll::Pending => TryOnce::Pending,
            Poll::Ready(v) => TryOnce::Ready(v),
        }
        .into()
    }
}

enum TryOnce<F>
where
    F: Future,
{
    Pending,
    Ready(F::Output),
}
