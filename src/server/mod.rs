//! Server that handles http requests.
//!
//! hreq can act as an http server. It supports [path] based [routing] to functions
//! acting as [handlers], [middleware] chains and [state handling].
//!
//! ## Example
//!
//! ```no_run
//! use hreq::prelude::*;
//! use hreq::server::Next;
//!
//! async fn start_server() {
//!    // Server without a state
//!    let mut server = Server::new();
//!
//!    // route requests for /hello/<name> where
//!    // "name" is a path parameter that can
//!    // be obtained in the request handler.
//!    server.at("/hello/:name")
//!        // logging middleware
//!        .middleware(logging)
//!        // dispatch to the handler
//!        .get(hello_there);
//!
//!    // Listen without TLS
//!    let (handle, addr) = server.listen(3000).await.unwrap();
//!
//!    println!("Server listening to: {}", addr);
//!
//!    handle.keep_alive().await;
//! }
//!
//! // Middleware logging request and response
//! async fn logging(
//!    req: http::Request<Body>, next: Next
//! ) -> http::Response<Body> {
//!
//!     println!("Request is: {:?}", req);
//!     let res = next.run(req).await.unwrap();
//!     println!("Response is: {:?}", res);
//!
//!     res
//! }
//!
//! // Handler of request producing responses. The String is
//! // converted to a 200 response with text/plain.
//! async fn hello_there(req: http::Request<Body>) -> String {
//!    format!("Hello {}", req.path_param("name").unwrap())
//! }
//! ```
//!
//! # State
//!
//! Many servers needs to work over some shared mutable state to function.
//! The server runs in an async runtime such as async-std or tokio, typically
//! with multiple threads accepting connections. Therefore the state needs
//! to be shareable between threads, in rust terms [`Sync`], as well being
//! clonable with [`Clone`].
//!
//! In practice this often means using a strategy seen in a lot of Rust code:
//! wrapping the state in `Arc<Mutex<State>>`.
//!
//! ## Example
//!
//! ```no_run
//! use hreq::prelude::*;
//! use std::sync::{Arc, Mutex};
//!
//! #[derive(Clone)]
//! struct MyCounter(Arc<Mutex<u64>>);
//!
//! async fn start_server() {
//!    // Shared state
//!    let state = MyCounter(Arc::new(Mutex::new(0)));
//!    // Server with a state
//!    let mut server = Server::with_state(state);
//!
//!    server.at("/do_something")
//!        // use stateful middleware/handlers
//!        .with_state()
//!        .get(my_handler);
//!
//!    let (handle, addr) = server.listen(3000).await.unwrap();
//!
//!    handle.keep_alive().await;
//! }
//!
//! async fn my_handler(
//!     counter: MyCounter,
//!     req: http::Request<Body>
//! ) -> String {
//!     let mut lock = counter.0.lock().unwrap();
//!     let req_count = *lock;
//!     *lock += 1;
//!     format!("Req number: {}", req_count)
//! }
//! ```
//!
//! [path]: struct.Server.html#method.at
//! [routing]: struct.Router.html
//! [handlers]: trait.Handler.html
//! [middleware]: trait.Middleware.html
//! [state handling]: struct.Route.html#method.with_state
//! [`Sync`]: https://doc.rust-lang.org/std/marker/trait.Sync.html
//! [`Clone`]: https://doc.rust-lang.org/std/clone/trait.Clone.html

use crate::params::resolve_hreq_params;
use crate::params::HReqParams;
use crate::proto::Protocol;
use crate::AsyncRuntime;
use crate::Body;
use crate::Error;
use crate::Stream;
use peek::Peekable;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

mod chain;
mod conn;
mod handler;
mod limit;
mod middle;
mod path;
mod peek;
mod reply;
mod resb_ext;
mod route;
mod router;
mod serv_handle;
mod serv_req_ext;
mod statik;

#[cfg(feature = "tls")]
mod tls_config;

use conn::Connection;
use serv_handle::EndFut;

pub use chain::Next;
pub use handler::{Handler, StateHandler};
pub use middle::{Middleware, StateMiddleware};
pub use reply::Reply;
pub use resb_ext::ResponseBuilderExt;
pub use route::{Route, StateRoute};
pub use router::Router;
pub use serv_handle::ServerHandle;
pub use serv_req_ext::ServerRequestExt;
pub use statik::serve_dir;

#[cfg(feature = "tls")]
pub use tls_config::TlsConfig;

/// Server of http requests.
///
/// See module documentation for example.
#[derive(Clone)]
pub struct Server<State> {
    state: Arc<State>,
    router: Router<State>,
}

impl Server<()> {
    /// Create a server without a state.
    pub fn new() -> Server<()> {
        Server::with_state(())
    }
}

impl<State> Server<State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    /// Create a server over a provided state.
    pub fn with_state(state: State) -> Self {
        Server {
            state: Arc::new(state),
            router: Router::new(),
        }
    }

    /// Get a reference to the current state.
    pub fn state(&self) -> &State {
        &*self.state
    }

    /// Configure a route for this server.
    ///
    /// A route is a chain of zero or more [`Middleware`]
    /// followed by a [`Handler`].
    ///
    /// All routes must be added before the call to `listen`. This configures
    /// the default [`Router`] in the server. It's possible to configiure
    /// separate routers and attach them later.
    ///
    /// Reusing the same `path` will overwrite the previous config.
    ///
    /// [`Middleware`]: trait.Middleware.html
    /// [`Handler`]: trait.Handler.html
    /// [`Router`]: struct.Router.html
    pub fn at(&mut self, path: &str) -> Route<'_, State> {
        self.router.at(path)
    }

    /// Bind and listen to the port (without TLS).
    ///
    /// The address bound will be `0.0.0.0:<port>`. Use port `0` to get a random port.
    ///
    /// The internal router is cloned on this call. That means all routes must be added
    /// already. Routes added after this call will not cause an error, but will not
    /// be dispatched to either.
    pub async fn listen(&self, port: u16) -> Result<(ServerHandle, SocketAddr), Error> {
        #[cfg(feature = "tls")]
        {
            Ok(self.do_listen(port, None).await?)
        }
        #[cfg(not(feature = "tls"))]
        {
            Ok(self.do_listen(port).await?)
        }
    }

    /// Bind and listen to the port with TLS.
    ///
    /// The address bound will be `0.0.0.0:<port>`. Use port `0` to get a random port.
    ///
    /// The internal router is cloned on this call. That means all routes must be added
    /// already. Routes added after this call will not cause an error, but will not
    /// be dispatched to either.
    #[cfg(feature = "tls")]
    pub async fn listen_tls(
        &self,
        port: u16,
        config: TlsConfig,
    ) -> Result<(ServerHandle, SocketAddr), Error> {
        let rustls_config = config.into_rustls_config()?;
        Ok(self.listen_tls_rustls(port, rustls_config).await?)
    }

    /// Bind and listen to the port with TLS using a specific Rustls config.
    ///
    /// The address bound will be `0.0.0.0:<port>`. Use port `0` to get a random port.
    ///
    /// The internal router is cloned on this call. That means all routes must be added
    /// already. Routes added after this call will not cause an error, but will not
    /// be dispatched to either.
    #[cfg(feature = "rustls")]
    pub async fn listen_tls_rustls(
        &self,
        port: u16,
        tls: rustls::ServerConfig,
    ) -> Result<(ServerHandle, SocketAddr), Error> {
        Ok(self.do_listen(port, Some(tls)).await?)
    }

    async fn do_listen(
        &self,
        port: u16,
        #[cfg(feature = "tls")] tls: Option<rustls::ServerConfig>,
    ) -> Result<(ServerHandle, SocketAddr), Error> {
        // TODO: async dns lookup in those cases where the async impl can do that.
        let bind_addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;

        let mut listener = AsyncRuntime::listen(bind_addr).await?;
        let local_addr = listener.local_addr()?;

        let (shut, end) = ServerHandle::new().await;

        // Driver that is cheap to clone.
        let driver = Arc::new(Driver::new(
            self.router.clone(),
            self.state.clone(),
            end.clone(),
        ));

        #[cfg(feature = "tls")]
        let tls = {
            if let Some(mut tls) = tls {
                crate::tls::configure_tls_server(&mut tls);
                Some(Arc::new(tls))
            } else {
                None
            }
        };

        // listening is a task so we can return the shutdown handles.
        let task = async move {
            loop {
                trace!("Waiting for connection");

                // accept new connections as long as not shut down.
                let next = end.race(listener.accept()).await?;

                match next {
                    Ok(v) => {
                        let (stream, remote_addr) = v;

                        trace!("Connection from: {}", remote_addr);

                        // Local clone for this connection.
                        let driver = driver.clone();

                        #[cfg(feature = "tls")]
                        let tls = tls.clone();

                        let conn_task = async move {
                            #[cfg(feature = "tls")]
                            {
                                if let Err(e) =
                                    driver.connect(stream, local_addr, remote_addr, tls).await
                                {
                                    debug!("Client connection failed: {}", e);
                                }
                            }

                            #[cfg(not(feature = "tls"))]
                            {
                                if let Err(e) =
                                    driver.connect(stream, local_addr, remote_addr).await
                                {
                                    debug!("Client connection failed: {}", e);
                                }
                            }
                        };

                        // each socket is handled in another spawn to listen for more sockets.
                        AsyncRuntime::spawn(conn_task);
                    }
                    Err(e) => {
                        // We end up here if we have too many open file descriptors.
                        warn!("Listen failed: {}, retrying…", e);
                        AsyncRuntime::timeout(Duration::from_secs(1)).await;
                    }
                }
            }

            #[allow(unreachable_code)] // for type checker
            Some(())
        };

        AsyncRuntime::spawn(task);

        Ok((shut, local_addr))
    }

    /// Manually dispatch a request to this server.
    ///
    /// This is mainly useful for building tests without binding a port.
    pub async fn handle<B: Into<Body>>(
        &self,
        req: http::Request<B>,
    ) -> Result<http::Response<Body>, Error> {
        // rebuild incoming request into Request<Body>

        // Body allows us to translate from one char encoding to another.
        //
        // Example:
        //
        //       Client           =>        Server
        // EUC-JP -> Shift_JIS          Shift_JIS -> UTF-8
        //
        // For a "normal" server with a socket in-between client/server
        // this is quite simple. We have one Body client side that
        // translates outgoing, and one Body server side that translates
        // incoming.
        //
        // When we use handle(), we don't have the socket in between.
        // In theory we should be able to shortcut the need for an
        // extra Body, the above example could be shortened EUC-JP -> UTF-8,
        // in practice it's not that easy to achieve.
        //
        // For now we rig a Body -> Body pair both for request and response
        // to simulate having a socket in between.
        //
        // TODO: gosh, this needs some refactoring.

        // 1. split/configure client request.
        let (mut parts, body, client_req_params) = {
            let (parts, body) = req.into_parts();
            let mut parts = resolve_hreq_params(parts);
            let mut body = body.into();
            let params = parts.extensions.get::<HReqParams>().cloned().unwrap();
            body.configure(&params, &parts.headers, false);
            // set appropriate headers
            crate::client::configure_request(&mut parts, &body, false);
            (parts, body, params)
        };

        // 2. make server request using parts/body from 1.
        let (req, server_req_params) = {
            let len = body.content_encoded_length();
            let mut body = Body::from_async_read(body, len);
            let params = HReqParams::new();
            body.configure(&params, &parts.headers, true);
            parts.extensions.insert(params.clone());
            (http::Request::from_parts(parts, body), params)
        };

        // state for stateful handlers.
        let state = self.state.clone();

        // dispatch server request from 2.
        let res = self.router.run(state, req).await.into_result()?;

        // 3. split server response.
        let (mut parts, body) = {
            // post configure the body
            let (parts, mut body) = res.into_parts();
            let mut server_res_params = parts
                .extensions
                .get::<HReqParams>()
                .cloned()
                .unwrap_or_else(HReqParams::new);

            server_res_params.copy_from_request(&server_req_params);
            body.configure(&server_res_params, &parts.headers, false);
            (parts, body)
        };

        // 4. make client response using parts/body from 3.
        let (parts, body) = {
            let len = body.content_encoded_length();
            let mut body = Body::from_async_read(body, len);
            body.configure(&client_req_params, &parts.headers, true);
            conn::configure_response(&mut parts, &body, false);
            parts.extensions.insert(client_req_params.clone());
            (parts, body)
        };

        Ok(http::Response::from_parts(parts, body))
    }
}

/// Connects TLS, routes requests and responses.
struct Driver<State> {
    router: Router<State>,
    state: Arc<State>,
    end: EndFut,
}

impl<State> Driver<State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    fn new(router: Router<State>, state: Arc<State>, end: EndFut) -> Self {
        Driver { router, state, end }
    }

    /// Optionally connects the incoming stream in TLS and figures out the protocol
    /// to talk either via ALPN or peeking the incoming bytes.
    pub(crate) async fn connect(
        self: Arc<Self>,
        tcp: impl Stream,
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        #[cfg(feature = "tls")] config: Option<Arc<rustls::ServerConfig>>,
    ) -> Result<(), Error> {
        //

        // Maybe wrap in TLS.
        let (stream, alpn_proto) = {
            #[cfg(feature = "tls")]
            {
                use crate::either::Either;
                use crate::tls::wrap_tls_server;
                if let Some(config) = config {
                    // wrap in tls
                    let (tls, proto) = wrap_tls_server(tcp, config).await?;
                    (Either::A(tls), proto)
                } else {
                    // tls feature on, but not using it.
                    (Either::B(tcp), Protocol::Unknown)
                }
            }

            #[cfg(not(feature = "tls"))]
            {
                // tls feature is off.
                (tcp, Protocol::Unknown)
            }
        };

        const H2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

        let mut peek = Peekable::new(stream, H2_PREFACE.len());

        // If we don't know what the protocol is by from tls ALPN,
        // we fall back on peeking the incoming bytes for the
        // http2 preface
        let proto = if alpn_proto == Protocol::Unknown {
            let peeked = peek.peek(H2_PREFACE.len()).await?;

            let p = if peeked == H2_PREFACE {
                Protocol::Http2
            } else {
                Protocol::Http11
            };

            trace!("Protocol by peek ({}): {:?}", remote_addr, p);
            p
        } else {
            trace!("Protocol by ALPN ({}): {:?}", remote_addr, alpn_proto);
            alpn_proto
        };

        Ok(self
            .handle_incoming(peek, local_addr, remote_addr, proto)
            .await?)
    }

    /// Handle all incoming requests from the given stream.
    pub(crate) async fn handle_incoming(
        self: Arc<Self>,
        stream: impl Stream,
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        proto: Protocol,
    ) -> Result<(), Error> {
        //

        // Make h1 or h2 abstraction over the connection.
        let mut conn = if proto == Protocol::Http2 {
            let h2conn = hreq_h2::server::handshake(stream).await?;
            Connection::H2(h2conn)
        } else {
            let h1conn = hreq_h1::server::handshake(stream);
            Connection::H1(h1conn)
        };

        debug!("Handshake done, waiting for requests: {}", remote_addr);

        loop {
            // Process each incoming request in turn.
            let inc = self.end.race(conn.accept(local_addr, remote_addr)).await;

            // outer Option is the shutdown
            // inner Option is whether there are more requests from conn.
            let next = if let Some(Some(r)) = inc {
                // Incoming can be an error
                r?
            } else {
                // either shutdown or no more requests from conn
                return Ok(());
            };

            // Cloning the driver is cheap for the inner spawn.
            let driver = self.clone();

            // Each request is handled in a separate spawn. This allow http2 to
            // do multiple requests (streams) multiplexed over the same connection
            // in parallel.
            let req_task = async move {
                let (req, send) = next;
                let params = req
                    .extensions()
                    .get::<HReqParams>()
                    .expect("Missing hreq_params in request")
                    .clone();

                // To run the request through the middleware/handlers we need a clone of the state.
                let state = driver.state.clone();

                // Keep this result as is since it's an error originating in the
                // middleware/handlers. Most likely it will be translated to a 500
                // error, but it's still semantically different from an error encountered
                // while trying to send the response back.
                let result = driver.router.run(state, req).await.into_result();

                // Send the response
                if let Err(err) = send.send_response(result, params).await {
                    if err.is_io() {
                        // Error encountered while sending a response back, maybe peer
                        // disconnected or similar.
                        debug!("Error sending response: {}", err);
                    } else {
                        // Error, like sending a body on a HEAD request.
                        error!("{}", err);
                    }
                }
            };

            AsyncRuntime::spawn(req_task);
        }
    }
}

impl<State> fmt::Debug for Server<State> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Server")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use http::{Request, Response};
    use std::io;

    #[derive(Clone)]
    pub struct App;

    #[test]
    pub fn ensure_type_signatures() {
        let mut server = Server::with_state(App);

        server
            .at("/p1")
            // check we can have a closure with async inner
            .get(|_req| async { "yo" });

        server
            .at("/p2")
            // simple scalar value return
            .get(return_scalar);

        server
            .at("/p3")
            // result returning something that is Into<crate::Error>
            .get(return_io_result)
            // check we chain also on endpoints
            .post(return_io_result);

        server
            .at("/p4")
            // middleware without state
            .middleware(mid_nostate)
            // straight up http response
            .get(return_response);

        server
            .at("/p5")
            // http response in a result
            .get(return_result_response);

        server
            .at("/op")
            // option for scalar
            .get(return_option);

        server
            .at("/p6")
            .with_state()
            // middleware taking state
            .middleware(mid_state)
            // endpoint taking state
            .get(return_result_response_state);
    }

    async fn return_scalar(_req: Request<Body>) -> String {
        format!("Yo {}", "world")
    }

    async fn mid_nostate(req: Request<Body>, next: Next) -> Result<Response<Body>, Error> {
        let res = next.run(req).await;
        res
    }

    async fn mid_state(_st: App, req: Request<Body>, next: Next) -> Result<Response<Body>, Error> {
        let res = next.run(req).await;
        res
    }

    async fn return_io_result(_req: Request<Body>) -> Result<String, io::Error> {
        Ok("yo".into())
    }

    async fn return_response(_req: Request<Body>) -> Response<String> {
        Response::builder().body("yo".into()).unwrap()
    }

    async fn return_option(_req: Request<Body>) -> Option<String> {
        None
    }

    async fn return_result_response(_req: Request<Body>) -> Result<Response<String>, http::Error> {
        Response::builder().body("yo".into())
    }

    async fn return_result_response_state(
        _state: App,
        _req: Request<Body>,
    ) -> Result<Response<String>, http::Error> {
        Response::builder().body("yo".into())
    }
}
