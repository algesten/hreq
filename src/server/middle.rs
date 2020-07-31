use super::Next;
use super::Reply;
use crate::Body;
use http::Request;
use std::future::Future;
use std::pin::Pin;

pub trait Middleware: Send + Sync + 'static {
    fn call<'a>(
        &'a self,
        req: Request<Body>,
        next: Next,
    ) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>>;
}

impl<F: Send + Sync + 'static, Fut, Ret> Middleware for F
where
    F: Fn(Request<Body>, Next) -> Fut,
    Fut: Future<Output = Ret> + Send + 'static,
    Ret: Into<Reply>,
{
    fn call<'a>(
        &'a self,
        req: Request<Body>,
        next: Next,
    ) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>> {
        let fut = (self)(req, next);
        Box::pin(async move { fut.await.into() })
    }
}

pub trait StateMiddleware<State>: Send + Sync + 'static {
    fn call<'a>(
        &'a self,
        state: State,
        req: Request<Body>,
        next: Next,
    ) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>>;
}

impl<State, F: Send + Sync + 'static, Fut, Ret> StateMiddleware<State> for F
where
    F: Fn(State, Request<Body>, Next) -> Fut,
    Fut: Future<Output = Ret> + Send + 'static,
    Ret: Into<Reply>,
{
    fn call<'a>(
        &'a self,
        state: State,
        req: Request<Body>,
        next: Next,
    ) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>> {
        let fut = (self)(state, req, next);
        Box::pin(async move { fut.await.into() })
    }
}
