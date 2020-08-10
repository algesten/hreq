use super::Next;
use super::Reply;
use crate::Body;
use http::Request;
use std::future::Future;
use std::pin::Pin;

/// Trait for middleware that doesn't use a state.
///
/// Typically this trait is not used directly since there is a blanket implementation
/// for any function that matches this signature:
///
/// ```ignore
/// async fn my_middleware(req: Request<Body>, next: Next) -> impl Into<Reply> {
///    ...
/// }
/// ```
///
/// [`Reply`] is not a type you would use in your own type signatures. `impl Into<Reply>`
/// represents a whole range of (concrete) possible return types. See [`Reply`] for more details.
///
/// # Examples
///
/// ```
/// use hreq::Error;
/// use hreq::prelude::*;
/// use hreq::server::Next;
///
/// fn main() {
///     let mut server = Server::new();
///
///     server.at("/path")
///         .middleware(my_middle)
///         .get(|_req| async { "Hello" });
/// }
///
/// async fn my_middle(
///     req: Request<Body>,
///     next: Next,
/// ) -> Result<Response<Body>, Error> {
///
///     // Do things with request here.
///
///     // Continue the request chain.
///     let res = next.run(req).await?;
///
///     // Do things with the response here.
///
///     Ok(res)
/// }
/// ```
///
///  [`Reply`]: struct.Reply.html
pub trait Middleware: Send + Sync + 'static {
    /// Call the middleware.
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

/// Trait for middleware that use a state.
///
/// Typically this trait is not used directly since there is a blanket implementation
/// for any function that matches this signature:
///
/// ```ignore
/// async fn my_middleware(
///     state: MyState,
///     req: Request<Body>,
///     next: Next
/// ) -> impl Into<Reply> {
///    ...
/// }
/// ```
///
/// [`Reply`] is not a type you would use in your own type signatures. `impl Into<Reply>`
/// represents a whole range of (concrete) possible return types. See [`Reply`] for more details.
///
/// # Examples
///
/// ```
/// use hreq::Error;
/// use hreq::prelude::*;
/// use hreq::server::Next;
/// use std::sync::{Arc, Mutex};
///
/// #[derive(Clone)]
/// struct MyState(Arc<Mutex<String>>);
///
/// fn main() {
///    let state = MyState(Arc::new(Mutex::new("Hello".to_string())));
///    let mut server = Server::with_state(state);
///
///    server.at("/path")
///        .with_state()
///        .middleware(my_middle)
///        .get(|state, req| async { "Hello" });
/// }
///
/// async fn my_middle(
///     state: MyState,
///     req: Request<Body>,
///     next: Next,
/// ) -> Result<Response<Body>, Error> {
///
///     // Do things with request here.
///
///     // Continue the request chain.
///     let res = next.run(req).await?;
///
///     // Do things with the response here.
///
///     Ok(res)
/// }
/// ```
///
///  [`Reply`]: struct.Reply.html
pub trait StateMiddleware<State>: Send + Sync + 'static {
    /// Call the middleware.
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
