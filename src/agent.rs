use crate::connect;
use crate::req_ext::RequestParams;
use crate::uri_ext::UriExt;
use crate::Body;
use crate::Connection;
use crate::Error;
use crate::ResponseExt;
use std::marker::PhantomData;
use tls_api::TlsConnector;

#[derive(Default)]
pub struct Agent<Tls: TlsConnector> {
    connections: Vec<Connection>,
    retries: usize,
    redirects: usize,
    _ph: PhantomData<Tls>,
}

impl<Tls: TlsConnector> Agent<Tls> {
    pub fn new() -> Self {
        Agent {
            connections: vec![],
            retries: 5,
            redirects: 5,
            _ph: PhantomData,
        }
    }

    pub fn retries(&mut self, retries: usize) {
        self.retries = retries;
    }

    pub fn redirects(&mut self, redirects: usize) {
        self.redirects = redirects;
    }

    fn reuse_from_pool(&mut self, uri: &http::Uri) -> Result<Option<&mut Connection>, Error> {
        trace!("Reuse from pool: {}", uri);
        let hostport = uri.host_port()?;
        let addr = hostport.to_string();
        Ok(self
            .connections
            .iter_mut()
            // http2 multiplexes over the same connection, http1 needs to finish previous req
            .find(|c| c.addr() == addr && (c.is_http2() || c.unfinished_requests() == 0)))
    }

    async fn connect_and_pool(&mut self, uri: &http::Uri) -> Result<&mut Connection, Error> {
        trace!("Connect new: {}", uri);
        let conn = connect::<Tls>(uri).await?;
        self.connections.push(conn);
        let idx = self.connections.len() - 1;
        Ok(self.connections.get_mut(idx).unwrap())
    }

    pub async fn send(&mut self, req: http::Request<Body>) -> Result<http::Response<Body>, Error> {
        let mut retries = self.retries;
        let mut redirects = self.redirects;

        let mut next_req = req;

        loop {
            let req = next_req;

            // next_req holds our (potential) next request in case of redirects.
            next_req = clone_to_empty_body(&req);

            // grab connection for the current request
            let conn = match self.reuse_from_pool(req.uri())? {
                Some(conn) => conn,
                None => self.connect_and_pool(req.uri()).await?,
            };

            match conn.send_request(req).await {
                Ok(res) => {
                    // follow redirections
                    let code = res.status_code();
                    if res.status().is_redirection() {
                        redirects -= 1;

                        // no more redirections. return what we have.
                        if redirects == 0 {
                            break Ok(res);
                        }

                        // amend uri in next_req relative to the old request.
                        let location = res
                            .header("location")
                            .ok_or_else(|| Error::Static("Redirect without Location header"))?;
                        let (mut parts, body) = next_req.into_parts();
                        parts.uri = parts.uri.parse_relative(location)?;
                        next_req = http::Request::from_parts(parts, body);

                        if code > 303 {
                            // TODO fix 307 and 308 using Expect-100 mechanic.
                            warn!("Unhandled redirection status: {} {}", code, location);
                            break Ok(res);
                        }

                        continue;
                    }
                    break Ok(res);
                }
                Err(err) => {
                    // remove this (failed) connection from the pool.
                    let conn_id = conn.id();
                    self.connections.retain(|c| c.id() != conn_id);

                    // retry?
                    retries -= 1;
                    if retries == 0 {
                        break Err(err);
                    }
                }
            }
        }
    }
}

fn clone_to_empty_body(from: &http::Request<Body>) -> http::Request<Body> {
    // most things can be cloned in the builder.
    let req = http::Request::builder()
        .method(from.method().clone())
        .uri(from.uri().clone())
        .version(from.version().clone())
        .body(Body::empty())
        .unwrap();

    let (mut parts, body) = req.into_parts();

    // headers can not be inserted as a complete cloned HeaderMap
    parts.headers = from.headers().clone();

    // there might be other extensions we're missing here.
    if let Some(params) = from.extensions().get::<RequestParams>() {
        parts.extensions.insert(params.clone());
    }

    http::Request::from_parts(parts, body)
}
