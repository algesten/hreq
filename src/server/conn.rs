use crate::body::{Body, BodyImpl};
use crate::body_send::BodySender;
use crate::head_ext::HeaderMapExt;
use crate::params::HReqParams;
use crate::Error;
use crate::AGENT_IDENT;
use crate::{AsyncRead, AsyncWrite};
use bytes::Bytes;
use hreq_h1::server::Connection as H1Connection;
use hreq_h1::server::SendResponse as H1SendResponse;
use hreq_h2::server::Connection as H2Connection;
use hreq_h2::server::SendResponse as H2SendResponse;
use httpdate::fmt_http_date;
use std::net::SocketAddr;
use std::time::SystemTime;

const BUF_SIZE: usize = 16_384;

pub(crate) enum Connection<Stream> {
    H1(H1Connection<Stream>),
    H2(H2Connection<Stream, Bytes>),
}

impl<Stream> Connection<Stream>
where
    Stream: AsyncRead + AsyncWrite + Unpin,
{
    pub async fn accept(
        &mut self,
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
    ) -> Option<Result<(http::Request<Body>, SendResponse), Error>> {
        match self {
            Connection::H1(c) => {
                if let Some(next) = c.accept().await {
                    match next {
                        Err(e) => return Some(Err(e.into())),
                        Ok(v) => {
                            let (req, send) = v;

                            let (parts, recv) = req.into_parts();

                            let body = Body::new(BodyImpl::Http1(recv), None);
                            let send = SendResponse::H1(send);

                            return Some(Ok(Self::configure(
                                parts,
                                body,
                                local_addr,
                                remote_addr,
                                send,
                            )));
                        }
                    }
                }
                trace!("H1 accept incoming end");
            }
            Connection::H2(c) => {
                if let Some(next) = c.accept().await {
                    match next {
                        Err(e) => return Some(Err(e.into())),
                        Ok(v) => {
                            let (req, send) = v;

                            let (parts, recv) = req.into_parts();

                            let body = Body::new(BodyImpl::Http2(recv), None);
                            let send = SendResponse::H2(send);

                            return Some(Ok(Self::configure(
                                parts,
                                body,
                                local_addr,
                                remote_addr,
                                send,
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
    ) -> (http::Request<Body>, SendResponse) {
        // Instantiate new HReqParams that will follow the request and response through.
        let mut hreq_params = HReqParams::new();
        hreq_params.mark_request_start();
        hreq_params.local_addr = local_addr;
        hreq_params.remote_addr = remote_addr;

        parts.extensions.insert(hreq_params.clone());

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
            .unwrap_or_else(|| HReqParams::new());

        // merge parameters together
        params.copy_from_request(&req_params);

        let (mut parts, mut body) = res.into_parts();

        body.configure(&params, &parts.headers, false);

        configure_response(&mut parts, &body, self.is_http2());

        let res = http::Response::from_parts(parts, ());
        let mut body_send = self.do_send(res)?;

        if !body.is_definitely_no_body() {
            loop {
                // This buffer must be less than h2 window size
                let mut buf = vec![0_u8; BUF_SIZE];

                // Wait for body_send to be able to receive more data
                body_send = body_send.ready().await?;

                let amount_read = body.read(&mut buf[..]).await?;

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

    fn do_send(self, res: http::Response<()>) -> Result<BodySender, Error> {
        Ok(match self {
            SendResponse::H1(send) => {
                let send_body = send.send_response(res, false)?;
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

        let mut body_send = self.do_send(res)?;

        body_send.send_end().await?;

        Ok(())
    }
}

pub(crate) fn configure_response(parts: &mut http::response::Parts, body: &Body, is_http2: bool) {
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

    if parts.headers.get("server").is_none() {
        parts.headers.set("server", &*AGENT_IDENT);
    }

    if parts.headers.get("date").is_none() {
        // Wed, 17 Apr 2013 12:00:00 GMT
        parts.headers.set("date", fmt_http_date(SystemTime::now()));
    }
}
