use crate::body::Body;
use crate::body_codec::BodyImpl;
use crate::body_send::BodySender;
use crate::bw::BandwidthMonitor;
use crate::head_ext::HeaderMapExt;
use crate::params::HReqParams;
use crate::uninit::UninitBuf;
use crate::Error;
use crate::AGENT_IDENT;
use crate::{AsyncRead, AsyncWrite};
use bytes::Bytes;
use futures_util::future::poll_fn;
use h2::server::Connection as H2Connection;
use h2::server::SendResponse as H2SendResponse;
use hreq_h1::server::Connection as H1Connection;
use hreq_h1::server::SendResponse as H1SendResponse;
use httpdate::fmt_http_date;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::Poll;
use std::time::SystemTime;
use tokio_util::compat::Compat;

const START_BUF_SIZE: usize = 16_384;
const MAX_BUF_SIZE: usize = 2 * 1024 * 1024;

pub(crate) struct Connection<Stream> {
    inner: Inner<Stream>,
    bw: Option<BandwidthMonitor>,
}

enum Inner<Stream> {
    H1(H1Connection<Stream>),
    H2(H2Connection<Compat<Stream>, Bytes>),
}

impl<Stream> Connection<Stream>
where
    Stream: AsyncRead + AsyncWrite + Unpin,
{
    pub fn new_h1(conn: H1Connection<Stream>) -> Self {
        Connection {
            inner: Inner::H1(conn),
            bw: None,
        }
    }

    pub fn new_h2(conn: H2Connection<Compat<Stream>, Bytes>, bw: BandwidthMonitor) -> Self {
        Connection {
            inner: Inner::H2(conn),
            bw: Some(bw),
        }
    }

    pub async fn accept(
        &mut self,
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
    ) -> Option<Result<(http::Request<Body>, SendResponse), Error>> {
        // cheap clone, either None or a Arc<Mutex<_>>
        let bw_acc = self.bw.clone();

        match &mut self.inner {
            Inner::H1(c) => {
                if let Some(next) = c.accept().await {
                    match next {
                        Err(e) => return Some(Err(e.into())),
                        Ok(v) => {
                            let (req, send) = v;

                            let (parts, recv) = req.into_parts();

                            let body = Body::new(BodyImpl::Http1(recv), None, false);
                            let send = SendResponse::H1(send);

                            return Some(Ok(Self::configure(
                                parts,
                                body,
                                local_addr,
                                remote_addr,
                                send,
                                None,
                            )));
                        }
                    }
                }
                trace!("H1 accept incoming end");
            }
            Inner::H2(c) => {
                let mut bw_acc = bw_acc.expect("h2 requires bandwidth monitor");

                let bw_req = bw_acc.clone();

                // piggy-back the bandwidth monitor on accepting requests from connection
                let accept_and_bw = poll_fn(move |cx| {
                    if let Poll::Ready(window_size) = bw_acc.poll_window_update(cx) {
                        trace!("Update h2 window size: {}", window_size);
                        c.set_target_window_size(window_size);
                        c.set_initial_window_size(window_size)?;
                    };
                    Pin::new(&mut *c).poll_accept(cx)
                });

                if let Some(next) = accept_and_bw.await {
                    match next {
                        Err(e) => return Some(Err(e.into())),
                        Ok(v) => {
                            let (req, send) = v;

                            let (parts, recv) = req.into_parts();

                            let body = Body::new(BodyImpl::Http2(recv), None, false);
                            let send = SendResponse::H2(send);

                            return Some(Ok(Self::configure(
                                parts,
                                body,
                                local_addr,
                                remote_addr,
                                send,
                                Some(bw_req),
                            )));
                        }
                    }
                }
                trace!("H2 accept incoming end");
            }
        };
        None
    }

    fn configure(
        mut parts: http::request::Parts,
        mut body: Body,
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        send: SendResponse,
        bw: Option<BandwidthMonitor>,
    ) -> (http::Request<Body>, SendResponse) {
        // Instantiate new HReqParams that will follow the request and response through.
        let mut hreq_params = HReqParams::new();
        hreq_params.mark_request_start();
        hreq_params.local_addr = local_addr;
        hreq_params.remote_addr = remote_addr;

        parts.extensions.insert(hreq_params.clone());

        body.set_bw_monitor(bw);
        body.configure(&hreq_params, &parts.headers, true);

        (http::Request::from_parts(parts, body), send)
    }
}

pub(crate) enum SendResponse {
    H1(H1SendResponse),
    H2(H2SendResponse<Bytes>),
}

impl SendResponse {
    pub async fn send_response(
        self,
        result: Result<http::Response<Body>, Error>,
        req_params: HReqParams,
    ) -> Result<(), Error> {
        match result {
            Ok(res) => self.handle_response(res, req_params).await?,
            Err(err) => self.handle_error(err).await?,
        }
        Ok(())
    }

    fn is_http2(&self) -> bool {
        if let SendResponse::H2(_) = self {
            return true;
        }
        false
    }

    async fn handle_response(
        self,
        mut res: http::Response<Body>,
        req_params: HReqParams,
    ) -> Result<(), Error> {
        //
        let mut params = res
            .extensions_mut()
            .remove::<HReqParams>()
            .unwrap_or_else(HReqParams::new);

        // merge parameters together
        params.copy_from_request(&req_params);

        let (mut parts, mut body) = res.into_parts();

        body.configure(&params, &parts.headers, false);

        // for small response bodies, we try to fully buffer the data.
        if params.prebuffer {
            body.attempt_prebuffer().await?;
        }

        configure_response(&mut parts, &body, self.is_http2());

        let res = http::Response::from_parts(parts, ());
        let mut body_send = self.do_send(res).await?;

        // this buffer should probably be less than h2 window size
        let mut buf = UninitBuf::with_capacity(START_BUF_SIZE, MAX_BUF_SIZE);

        if !body.is_definitely_no_body() {
            loop {
                buf.clear();

                let amount_read = buf.read_from_async(&mut body).await?;

                // Ship it to they underlying http1.1/http2 layer.
                body_send.send_data(&buf[0..amount_read]).await?;

                if amount_read == 0 {
                    break;
                }
            }
        }

        body_send.send_end().await?;

        Ok(())
    }

    async fn do_send(self, res: http::Response<()>) -> Result<BodySender, Error> {
        Ok(match self {
            SendResponse::H1(send) => {
                let send_body = send.send_response(res, false).await?;
                BodySender::H1(send_body)
            }
            SendResponse::H2(mut send) => {
                let send_body = send.send_response(res, false)?;
                BodySender::H2(send_body)
            }
        })
    }

    async fn handle_error(self, err: Error) -> Result<(), Error> {
        warn!("Middleware/handlers failed: {}", err);

        let res = http::Response::builder().status(500).body(()).unwrap();

        let mut body_send = self.do_send(res).await?;

        body_send.send_end().await?;

        Ok(())
    }
}

pub(crate) fn configure_response(parts: &mut http::response::Parts, body: &Body, is_http2: bool) {
    let is304 = parts.status == 304;

    // https://tools.ietf.org/html/rfc7232#section-4.1
    //
    // Since the goal of a 304 response is to minimize information transfer
    // when the recipient already has one or more cached representations, a
    // sender SHOULD NOT generate representation metadata other than the
    // above listed fields unless said metadata exists for the purpose of
    // guiding cache updates (e.g., Last-Modified might be useful if the
    // response does not have an ETag field).
    if !is304 {
        if let Some(len) = body.content_encoded_length() {
            // the body indicates a length (for sure).
            let user_set_length = parts.headers.get("content-length").is_some();

            if !user_set_length && (len > 0 || !parts.status.is_redirection()) {
                parts.headers.set("content-length", len.to_string());
            }
        } else if !is_http2 && !parts.status.is_redirection() {
            // body does not indicate a length (like from a reader),
            // and status indicates there really is one.
            // we chose chunked.
            if parts.headers.get("transfer-encoding").is_none() {
                parts.headers.set("transfer-encoding", "chunked");
            }
        }

        if parts.headers.get("content-type").is_none() {
            if let Some(ctype) = body.content_type() {
                parts.headers.set("content-type", ctype);
            }
        }
    }

    if parts.headers.get("server").is_none() {
        parts.headers.set("server", &*AGENT_IDENT);
    }

    if parts.headers.get("date").is_none() {
        // Wed, 17 Apr 2013 12:00:00 GMT
        parts.headers.set("date", fmt_http_date(SystemTime::now()));
    }
}
