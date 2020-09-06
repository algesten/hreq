use super::chain::Mid;
use super::path::ParsedPath;
use super::router::RouteMethod;
use super::Handler;
use super::Router;
use super::StateHandler;
use super::{Middleware, StateMiddleware};
use http::Method;
use std::fmt;
use std::sync::Arc;

/// A route as obtained by [`Server::at`] or [`Router::at`].
///
/// [`Server::at`]: struct.Server.html#method.at
/// [`Router::at`]: struct.Router.html#method.at
pub struct Route<'a, State> {
    router: &'a mut Router<State>,
    path: ParsedPath,
    middlewares: Vec<Arc<Mid<State>>>,
}

impl<'a, State> Route<'a, State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    pub(crate) fn new(router: &'a mut Router<State>, path: ParsedPath) -> Self {
        Route {
            router,
            path,
            middlewares: vec![],
        }
    }

    /// Attach [`Middleware`].
    ///
    /// Middleware must be added before handlers that are to be affected by it.
    ///
    /// [`Middleware`]: trait.Middleware.html
    pub fn middleware<M: Middleware>(mut self, middleware: M) -> Self {
        let boxed: Box<dyn Middleware> = Box::new(middleware);
        self.middlewares.push(Arc::new(boxed.into()));
        self
    }

    /// Attach a [`Router`].
    ///
    /// [`Router`]: struct.Router.html
    pub fn router(self, mut router: Router<State>) -> Self {
        let m = RouteMethod::All;
        let mw = self.middlewares.clone();
        router.set_prefix(self.path.path());
        self.router.add_handler(m, &self.path, mw, router.into());
        self
    }

    /// Continue using stateful middleware and handlers.
    ///
    /// # Example
    ///
    /// ```
    /// use hreq::prelude::*;
    /// use std::sync::{Arc, Mutex};
    ///
    /// #[derive(Clone)]
    /// struct MyState(Arc<Mutex<String>>);
    ///
    /// async fn start_server() {
    ///     let state = MyState(Arc::new(Mutex::new("Hello".to_string())));
    ///     let mut server = Server::with_state(state);
    ///     server.at("/stateful").with_state().get(get_thing);
    /// }
    ///
    /// async fn get_thing(state: MyState, req: http::Request<Body>) -> String {
    ///     format!("Hello {}", req.path_param("name").unwrap())
    /// }
    /// ```
    pub fn with_state(self) -> StateRoute<'a, State> {
        StateRoute(self)
    }
}

impl<'a, State> Route<'a, State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    /// Attach a handler for the given method.
    pub fn method<H: Handler>(self, method: Method, handler: H) -> Self {
        let m = RouteMethod::Method(method);
        let mw = self.middlewares.clone();
        let boxed: Box<dyn Handler> = Box::new(handler);
        self.router.add_handler(m, &self.path, mw, boxed.into());
        self
    }

    /// Attach a handler for all methods.
    pub fn all<H: Handler>(self, handler: H) -> Self {
        let m = RouteMethod::All;
        let mw = self.middlewares.clone();
        let boxed: Box<dyn Handler> = Box::new(handler);
        self.router.add_handler(m, &self.path, mw, boxed.into());
        self
    }

    /// GET request handler.
    pub fn get<H: Handler>(self, handler: H) -> Self {
        self.method(Method::GET, handler)
    }

    /// HEAD request handler.
    pub fn head<H: Handler>(self, handler: H) -> Self {
        self.method(Method::HEAD, handler)
    }

    /// POST request handler.
    pub fn post<H: Handler>(self, handler: H) -> Self {
        self.method(Method::POST, handler)
    }

    /// PUT request handler.
    pub fn put<H: Handler>(self, handler: H) -> Self {
        self.method(Method::PUT, handler)
    }

    /// DELETE request handler.
    pub fn delete<H: Handler>(self, handler: H) -> Self {
        self.method(Method::DELETE, handler)
    }

    /// OPTIONS request handler.
    pub fn options<H: Handler>(self, handler: H) -> Self {
        self.method(Method::OPTIONS, handler)
    }

    /// CONNECT request handler.
    pub fn connect<H: Handler>(self, handler: H) -> Self {
        self.method(Method::CONNECT, handler)
    }

    /// PATCH request handler.
    pub fn patch<H: Handler>(self, handler: H) -> Self {
        self.method(Method::PATCH, handler)
    }

    /// TRACE request handler.
    pub fn trace<H: Handler>(self, handler: H) -> Self {
        self.method(Method::TRACE, handler)
    }
}

/// A state route as obtained by [`with_state`].
///
/// ```
/// use hreq::prelude::*;
/// use std::sync::{Arc, Mutex};
///
/// #[derive(Clone)]
/// struct MyState(Arc<Mutex<String>>);
///
/// async fn start_server() {
///     let state = MyState(Arc::new(Mutex::new("Hello".to_string())));
///     let mut server = Server::with_state(state);
///     server.at("/stateful").with_state().get(get_thing);
/// }
///
/// async fn get_thing(state: MyState, req: http::Request<Body>) -> String {
///     format!("Hello {}", req.path_param("name").unwrap())
/// }
/// ```
///
/// [`Server::at`]: struct.Server.html#method.at
/// [`Router::at`]: struct.Router.html#method.at
/// [`with_state`]: struct.Route.html#method.with_state
pub struct StateRoute<'a, State>(Route<'a, State>);

impl<'a, State> StateRoute<'a, State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    /// Attach [`Middleware`].
    ///
    /// Middleware must be added before handlers that are to be affected by it.
    ///
    /// [`Middleware`]: trait.Middleware.html
    pub fn middleware<M: StateMiddleware<State>>(mut self, middleware: M) -> Self {
        let boxed: Box<dyn StateMiddleware<State>> = Box::new(middleware);
        self.0.middlewares.push(Arc::new(boxed.into()));
        self
    }

    /// Attach a [`Router`].
    ///
    /// [`Router`]: struct.Router.html
    pub fn router(self, mut router: Router<State>) {
        let m = RouteMethod::All;
        let mw = self.0.middlewares.clone();
        router.set_prefix(self.0.path.path());
        self.0
            .router
            .add_handler(m, &self.0.path, mw, router.into());
    }
}

impl<'a, State> StateRoute<'a, State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    /// Attach a handler for the given method.
    pub fn method<H: StateHandler<State>>(self, method: Method, handler: H) -> Self {
        let m = RouteMethod::Method(method);
        let mw = self.0.middlewares.clone();
        let boxed: Box<dyn StateHandler<State>> = Box::new(handler);
        self.0.router.add_handler(m, &self.0.path, mw, boxed.into());
        self
    }

    /// Attach a handler for all methods.
    pub fn all<H: StateHandler<State>>(self, handler: H) -> Self {
        let m = RouteMethod::All;
        let mw = self.0.middlewares.clone();
        let boxed: Box<dyn StateHandler<State>> = Box::new(handler);
        self.0.router.add_handler(m, &self.0.path, mw, boxed.into());
        self
    }

    /// GET request handler.
    pub fn get<H: StateHandler<State>>(self, handler: H) -> Self {
        self.method(Method::GET, handler)
    }

    /// HEAD request handler.
    pub fn head<H: StateHandler<State>>(self, handler: H) -> Self {
        self.method(Method::HEAD, handler)
    }

    /// POST request handler.
    pub fn post<H: StateHandler<State>>(self, handler: H) -> Self {
        self.method(Method::POST, handler)
    }

    /// PUT request handler.
    pub fn put<H: StateHandler<State>>(self, handler: H) -> Self {
        self.method(Method::PUT, handler)
    }

    /// DELETE request handler.
    pub fn delete<H: StateHandler<State>>(self, handler: H) -> Self {
        self.method(Method::DELETE, handler)
    }

    /// OPTIONS request handler.
    pub fn options<H: StateHandler<State>>(self, handler: H) -> Self {
        self.method(Method::OPTIONS, handler)
    }

    /// CONNECT request handler.
    pub fn connect<H: StateHandler<State>>(self, handler: H) -> Self {
        self.method(Method::CONNECT, handler)
    }

    /// PATCH request handler.
    pub fn patch<H: StateHandler<State>>(self, handler: H) -> Self {
        self.method(Method::PATCH, handler)
    }

    /// TRACE request handler.
    pub fn trace<H: StateHandler<State>>(self, handler: H) -> Self {
        self.method(Method::TRACE, handler)
    }
}

impl<'a, State> fmt::Debug for Route<'a, State> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Route")
    }
}

impl<'a, State> fmt::Debug for StateRoute<'a, State> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "StateRoute")
    }
}
