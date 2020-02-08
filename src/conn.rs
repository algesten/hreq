use crate::conn_http1::send_request_http1;
use crate::conn_http2::send_request_http2;
use crate::h1::SendRequest as H1SendRequest;
use crate::reqb_ext::RequestParams;
use crate::res_ext::HeaderMapExt;
use crate::uri_ext::MethodExt;
use crate::Body;
use crate::Error;
use bytes::Bytes;
use h2::client::SendRequest as H2SendRequest;
use once_cell::sync::Lazy;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

static ID_COUNTER: Lazy<AtomicUsize> = Lazy::new(|| AtomicUsize::new(0));

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
    addr: String,
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
    pub(crate) fn new(addr: String, p: ProtocolImpl) -> Self {
        Connection {
            id: ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            addr,
            p,
            unfinished_reqs: Arc::new(()),
        }
    }

    pub(crate) fn id(&self) -> usize {
        self.id
    }

    pub(crate) fn addr(&self) -> &str {
        &self.addr
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

        let params = *parts.extensions.get::<RequestParams>().unwrap();
        let deadline = params.deadline();

        // resolve deferred body codecs because content-encoding and content-type are settled.
        body.configure(deadline, &parts.headers, false);

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

        let req = http::Request::from_parts(parts, body);

        trace!("{} {} {} {}", self.p, self.addr, req.method(), req.uri());

        match &mut self.p {
            ProtocolImpl::Http1(send_req) => {
                let s = send_req.clone();
                deadline.race(send_request_http1(s, req, unfin)).await
            }
            ProtocolImpl::Http2(send_req) => {
                let s = send_req.clone();
                deadline.race(send_request_http2(s, req, unfin)).await
            }
        }
    }
}
