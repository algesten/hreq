//! Connection pooling, redirects, cookies etc.

use crate::async_impl::AsyncRuntime;
use crate::connect;
use crate::cookies::Cookies;
use crate::reqb_ext::resolve_hreq_ext;
use crate::reqb_ext::{QueryParams, RequestParams};
use crate::uri_ext::UriExt;
use crate::Body;
use crate::Connection;
use crate::Error;
use crate::ResponseExt;
use cookie::Cookie;
use std::time::Duration;

/// Agents provide redirects, connection pooling, cookies and retries.
///
/// Every request is sent through an agent, also when using the extension trait
/// (`Request.send()`). When using the trait, the agent is intantiated with default
/// parameters and lives for the length of the request.
///
/// To amend the default parameters, or reuse an agent over many requests, use `Agent::new()`
/// and send the request using `agent.send(req)`.
///
/// The default agent have the following settings:
///
///   * Redirects: 5
///   * Retries: 5
///   * Connection pooling: on
///   * Cookies: on (TODO)
///
/// The settings can be changed, and are used for the next `.send()` call. It is possible
/// to change the settings between calls.
///
/// ```
/// use hreq::prelude::*;
/// use hreq::Agent;
///
/// let mut agent = Agent::new();
/// agent.retries(0); // disable all retries
///
/// let req = Request::get("https://www.google.com")
///     .with_body(()).unwrap();
///
/// let res = agent.send(req).block();
/// ```
#[derive(Default)]
pub struct Agent {
    connections: Vec<Connection>,
    cookies: Option<Cookies>,
    redirects: i8,
    retries: i8,
    pooling: bool,
    use_cookies: bool,
}

impl Agent {
    /// Creates a new agent with default parameters.
    ///
    /// ```
    /// use hreq::Agent;
    ///
    /// let agent = Agent::new();
    /// ```
    pub fn new() -> Self {
        Agent {
            connections: vec![],
            cookies: None,
            redirects: 5,
            retries: 5,
            pooling: true,
            use_cookies: true,
        }
    }

    /// Changes number of redirects.
    ///
    /// Defaults to `5`. Set to `0` to disable redirects.
    ///
    /// The number of redirects will be used for the next call to `.send()`.
    ///
    /// ```
    /// use hreq::Agent;
    ///
    /// let mut agent = Agent::new();
    /// agent.redirects(0);
    /// ```
    pub fn redirects(&mut self, amount: u8) {
        self.redirects = amount as i8;
    }

    /// Changes the number of retry attempts.
    ///
    /// Defaults to `5`. Set to `0` to disable retries.
    ///
    /// The number of retries will be used for the next call to `.send()`.
    ///
    /// ```
    /// use hreq::Agent;
    ///
    /// let mut agent = Agent::new();
    /// agent.retries(0);
    /// ```
    pub fn retries(&mut self, amount: u8) {
        self.retries = amount as i8;
    }

    /// Turns connection pooling on or off.
    ///
    /// Defaults to `true`. Set to `false` to disable connection pooling.
    ///
    /// The setting will be used for the next call to `.send()`.
    ///
    /// When set to `false` any existing connection currently pooled will be dropped.
    ///
    /// ```
    /// use hreq::Agent;
    ///
    /// let mut agent = Agent::new();
    /// agent.pooling(false);
    /// ```
    pub fn pooling(&mut self, enabled: bool) {
        self.pooling = enabled;
        if !enabled {
            self.connections.clear();
        }
    }

    /// Turns on or off the use of cookies.
    ///
    /// Defaults to `true`. Set to `false` to disable use of cookies.
    ///
    /// The setting will be used for the next call to `.send()`.
    ///
    /// When set to `false`, any previous collected cookie will be dropped.
    ///
    /// ```
    /// use hreq::Agent;
    ///
    /// let mut agent = Agent::new();
    /// agent.cookies(false);
    /// ```
    pub fn cookies(&mut self, enabled: bool) {
        self.use_cookies = enabled;
        if !enabled {
            self.cookies = None;
        }
    }

    /// Get all cookies held in this agent matching the given uri.
    pub fn get_cookies(&self, uri: &http::Uri) -> Vec<&Cookie<'static>> {
        if let Some(cookies) = &self.cookies {
            cookies.get(uri)
        } else {
            vec![]
        }
    }

    fn reuse_from_pool(&mut self, uri: &http::Uri) -> Result<Option<&mut Connection>, Error> {
        if !self.pooling {
            return Ok(None);
        }
        let host_port = uri.host_port()?;
        let ret = self
            .connections
            .iter_mut()
            // http2 multiplexes over the same connection, http1 needs to finish previous req
            .find(|c| {
                c.host_port() == &host_port && (c.is_http2() || c.unfinished_requests() == 0)
            });
        if ret.is_some() {
            trace!("Reuse from pool: {}", uri);
        }
        Ok(ret)
    }

    /// Sends a request using this agent.
    ///
    /// The parameters configured in the agent are used for the request.
    ///
    /// Depending on agent settings, connections are pooled and cookies reused between
    /// repeated calls to `send()`.
    ///
    /// ```
    /// use hreq::prelude::*;
    /// use hreq::Agent;
    ///
    /// let mut agent = Agent::new();
    /// agent.retries(0);
    /// agent.redirects(0);
    ///
    /// let req = Request::get("https://fails-badly-yeah")
    ///     .with_body(()).unwrap();
    ///
    /// let res = agent.send(req).block();
    ///
    /// assert!(res.is_err());
    /// assert!(res.unwrap_err().is_io());
    /// ```
    pub async fn send(&mut self, req: http::Request<Body>) -> Result<http::Response<Body>, Error> {
        // apply the parameters held in a separate storage
        let req = resolve_hreq_ext(req);

        let params = req.extensions().get::<RequestParams>().unwrap().clone();

        // the request should be time limited regardless of retries. the entire do_send()
        // is wrapped in a ticking timer...
        let deadline = params.deadline();

        // for lifetime reasons it's easier to handle the cookie storage separately
        let mut cookies = self.cookies.take();

        let ret = deadline.race(self.do_send(req, params, &mut cookies)).await;

        self.cookies = cookies;

        ret
    }

    async fn do_send(
        &mut self,
        req: http::Request<Body>,
        params: RequestParams,
        cookies: &mut Option<Cookies>,
    ) -> Result<http::Response<Body>, Error> {
        trace!("Agent {} {}", req.method(), req.uri());

        let mut retries = self.retries;
        let mut backoff_millis: u64 = 125;
        let mut redirects = self.redirects;
        let pooling = self.pooling;
        let mut unpooled: Option<Connection> = None;
        let use_cookies = self.use_cookies;

        // if we have a param.with_override, whenever we are to open a connection,
        // we check whether the current uri has an equal hostport to this, that
        // way we can override also subsequent requests for the original uri.
        let orig_hostport = req.uri().host_port()?.to_owned();

        let mut next_req = req;

        loop {
            let mut req = next_req;
            let uri = req.uri().clone();

            // add cookies to send
            if self.use_cookies {
                if let Some(cookies) = cookies {
                    let cookies = cookies.get(&uri);
                    for cookie in cookies {
                        // TODO this is a bit inefficient, the .encoded() returns
                        // the full cookie including ;HttpOnly etc.
                        let no_param = Cookie::new(cookie.name(), cookie.value());
                        let cval = no_param.encoded().to_string();
                        let val = http::header::HeaderValue::from_str(&cval)
                            .expect("Cookie header value");
                        // TODO combine multiple cookies into less headers.
                        req.headers_mut().append("cookie", val);
                    }
                }
            }

            // remember whether request is idempotent in case we are to retry
            let is_idempotent = req.method().is_idempotent();

            // next_req holds our (potential) next request in case of redirects.
            next_req = clone_to_empty_body(&req);

            // grab connection for the current request
            let conn = match self.reuse_from_pool(&uri)? {
                Some(conn) => conn,
                None => {
                    let hostport_uri = uri.host_port()?;
                    let mut conn: Option<Connection> = None;

                    // if the current request is for the same uri (hostport part) as
                    // the original uri, we will use the override.
                    if orig_hostport == hostport_uri {
                        if let Some(arc) = params.with_override.clone() {
                            let hostport = &*arc;
                            trace!("Connect new: {} with override: {}", uri, hostport);
                            conn = Some(connect(&hostport, params.force_http2).await?);
                        }
                    }

                    // no override for this connection.
                    let conn = match conn {
                        Some(conn) => conn,
                        None => {
                            trace!("Connect new: {}", hostport_uri);
                            connect(&hostport_uri, params.force_http2).await?
                        }
                    };

                    if pooling {
                        self.connections.push(conn);
                        let idx = self.connections.len() - 1;
                        self.connections.get_mut(idx).unwrap()
                    } else {
                        unpooled.replace(conn);
                        unpooled.as_mut().unwrap()
                    }
                }
            };

            match conn.send_request(req).await {
                Ok(mut res) => {
                    // squirrel away cookies (also in redirects)
                    if use_cookies {
                        for cookie_head in res.headers().get_all("set-cookie") {
                            if let Ok(v) = cookie_head.to_str() {
                                if let Ok(cookie) = Cookie::parse_encoded(v.to_string()) {
                                    if cookies.is_none() {
                                        *cookies = Some(Cookies::new());
                                    }
                                    cookies.as_mut().unwrap().add(&uri, cookie);
                                } else {
                                    info!("Failed to parse cookie: {}", v);
                                }
                            } else {
                                info!("Failed to read cookie value: {:?}", cookie_head);
                            }
                        }
                    }

                    // follow redirections
                    let code = res.status_code();
                    if res.status().is_redirection() {
                        redirects -= 1;

                        // no more redirections. return what we have.
                        if redirects < 0 {
                            trace!("Not following more redirections");
                            break Ok(res);
                        }

                        // amend uri in next_req relative to the old request.
                        let location = res.header("location").ok_or_else(|| {
                            Error::Proto("Redirect without Location header".into())
                        })?;

                        trace!("Redirect to: {}", location);

                        let (mut parts, body) = next_req.into_parts();
                        parts.uri = parts.uri.parse_relative(location)?;
                        next_req = http::Request::from_parts(parts, body);

                        if code > 303 {
                            // TODO fix 307 and 308 using Expect-100 mechanic.
                            warn!("Unhandled redirection status: {} {}", code, location);
                            break Ok(res);
                        }

                        // exhaust the previous body before following the redirect.
                        // this is to ensure http1.1 connections are in a good state.
                        if res.body_mut().read_to_end().await.is_err() {
                            // some servers just close the connection after a redirect.
                            let conn_id = conn.id();
                            self.connections.retain(|c| c.id() != conn_id);
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
                    if retries == 0 || !is_idempotent || !err.is_retryable() {
                        trace!("Abort with error, {}", err);
                        break Err(err);
                    }

                    trace!("Retrying on error, {}", err);
                }
            }
            // retry backoff
            trace!("Retry backoff: {}ms", backoff_millis);
            AsyncRuntime::current()
                .timeout(Duration::from_millis(backoff_millis))
                .await;
            backoff_millis = (backoff_millis * 2).min(10_000);
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
    if let Some(params) = from.extensions().get::<QueryParams>() {
        parts.extensions.insert(params.clone());
    }

    http::Request::from_parts(parts, body)
}
