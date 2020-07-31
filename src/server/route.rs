use super::chain::Mid;
use super::path::ParsedPath;
use super::router::RouteMethod;
use super::Handler;
use super::Router;
use super::StateHandler;
use super::{Middleware, StateMiddleware};
use http::Method;
use std::sync::Arc;

pub trait MethodHandlers<H>: Sized {
    fn method(self, method: Method, handler: H) -> Self;

    fn all(self, handler: H) -> Self;

    fn get(self, handler: H) -> Self {
        self.method(Method::GET, handler)
    }

    fn head(self, handler: H) -> Self {
        self.method(Method::HEAD, handler)
    }

    fn post(self, handler: H) -> Self {
        self.method(Method::POST, handler)
    }

    fn put(self, handler: H) -> Self {
        self.method(Method::PUT, handler)
    }

    fn delete(self, handler: H) -> Self {
        self.method(Method::DELETE, handler)
    }

    fn options(self, handler: H) -> Self {
        self.method(Method::OPTIONS, handler)
    }

    fn connect(self, handler: H) -> Self {
        self.method(Method::CONNECT, handler)
    }

    fn patch(self, handler: H) -> Self {
        self.method(Method::PATCH, handler)
    }

    fn trace(self, handler: H) -> Self {
        self.method(Method::TRACE, handler)
    }
}

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

    pub fn middleware<M: Middleware>(mut self, middleware: M) -> Self {
        let boxed: Box<dyn Middleware> = Box::new(middleware);
        self.middlewares.push(Arc::new(boxed.into()));
        self
    }

    pub fn router(self, mut router: Router<State>) -> Self {
        let m = RouteMethod::All;
        let mw = self.middlewares.clone();
        router.set_prefix(self.path.path());
        self.router.add_handler(m, &self.path, mw, router.into());
        self
    }

    pub fn with_state(self) -> StateRoute<'a, State> {
        StateRoute(self)
    }
}

impl<'a, State, H: Handler> MethodHandlers<H> for Route<'a, State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    fn method(self, method: Method, handler: H) -> Self {
        let m = RouteMethod::Method(method);
        let mw = self.middlewares.clone();
        let boxed: Box<dyn Handler> = Box::new(handler);
        self.router.add_handler(m, &self.path, mw, boxed.into());
        self
    }

    fn all(self, handler: H) -> Self {
        let m = RouteMethod::All;
        let mw = self.middlewares.clone();
        let boxed: Box<dyn Handler> = Box::new(handler);
        self.router.add_handler(m, &self.path, mw, boxed.into());
        self
    }
}

pub struct StateRoute<'a, State>(Route<'a, State>);

impl<'a, State> StateRoute<'a, State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    pub fn middleware<M: StateMiddleware<State>>(mut self, middleware: M) -> Self {
        let boxed: Box<dyn StateMiddleware<State>> = Box::new(middleware);
        self.0.middlewares.push(Arc::new(boxed.into()));
        self
    }

    pub fn router(self, mut router: Router<State>) {
        let m = RouteMethod::All;
        let mw = self.0.middlewares.clone();
        router.set_prefix(self.0.path.path());
        self.0
            .router
            .add_handler(m, &self.0.path, mw, router.into());
    }
}

impl<'a, State, H: StateHandler<State>> MethodHandlers<H> for StateRoute<'a, State>
where
    State: Clone + Unpin + Send + Sync + 'static,
{
    fn method(self, method: Method, handler: H) -> Self {
        let m = RouteMethod::Method(method);
        let mw = self.0.middlewares.clone();
        let boxed: Box<dyn StateHandler<State>> = Box::new(handler);
        self.0.router.add_handler(m, &self.0.path, mw, boxed.into());
        self
    }

    fn all(self, handler: H) -> Self {
        let m = RouteMethod::All;
        let mw = self.0.middlewares.clone();
        let boxed: Box<dyn StateHandler<State>> = Box::new(handler);
        self.0.router.add_handler(m, &self.0.path, mw, boxed.into());
        self
    }
}
