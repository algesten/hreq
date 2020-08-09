use super::Reply;
use crate::Body;
use http::Request;
use std::future::Future;
use std::pin::Pin;

/// Trait for a request handler that doesn't use a state.
///
/// Typically this trait is not used directly since there is a blanket implementation
/// for any function that matches this signature:
///
/// ```ignore
/// async fn my_handler(req: Request<Body>) -> impl Into<Reply> {
///    ...
/// }
/// ```
///
/// [`Reply`] is not a type you would use in your own type signatures. `impl Into<Reply>`
/// represents a whole range of (concrete) possible return types. See [`Reply`] for more details.
///
/// # Examples
///
///  ```
///  use hreq::prelude::*;
///
///  async fn start_server() {
///     let mut server = Server::new();
///
///     server.at("/hello/:name").get(hello_there);
///
///     server.listen(3000).await.unwrap();
///  }
///
///  async fn hello_there(req: http::Request<Body>) -> String {
///     format!("Hello {}", req.path_param("name").unwrap())
///  }
///  ```
///
///  [`Reply`]: struct.Reply.html
pub trait Handler: Send + Sync + 'static {
    /// Call the handler.
    fn call<'a>(&'a self, req: Request<Body>) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>>;
}

impl<F: Send + Sync + 'static, Fut, Ret> Handler for F
where
    F: Fn(Request<Body>) -> Fut,
    Fut: Future<Output = Ret> + Send + 'static,
    Ret: Into<Reply>,
{
    fn call<'a>(&'a self, req: Request<Body>) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>> {
        let fut = (self)(req);
        Box::pin(async move {
            let res = fut.await;
            res.into()
        })
    }
}

/// Trait for a request handler that use a state.
///
/// Typically this trait is not used directly since there is a blanket implementation
/// for any function that matches this signature:
///
/// ```ignore
/// struct MyState { ... }
///
/// async fn my_handler(state: MyState, req: Request<Body>) -> impl Into<Reply> {
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
/// use hreq::prelude::*;
/// use std::sync::{Arc, Mutex};
///
/// #[derive(Clone)]
/// struct MyState(Arc<Mutex<String>>);
///
/// async fn start_server() {
///    let state = MyState(Arc::new(Mutex::new("Hello".to_string())));
///    let mut server = Server::with_state(state);
///
///    server.at("/hello/:name").with_state().get(hello_there);
///
///    server.listen(3000).await.unwrap();
/// }
///
/// async fn hello_there(state: MyState, req: http::Request<Body>) -> String {
///    let lock = state.0.lock().unwrap();
///    format!("{} {}", *lock, req.path_param("name").unwrap())
/// }
/// ```
///
///  [`Reply`]: struct.Reply.html
pub trait StateHandler<State>: Send + Sync + 'static {
    /// Call the handler.
    fn call<'a>(
        &'a self,
        state: State,
        req: Request<Body>,
    ) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>>;
}

impl<State, F: Send + Sync + 'static, Fut, Ret> StateHandler<State> for F
where
    F: Fn(State, Request<Body>) -> Fut,
    Fut: Future<Output = Ret> + Send + 'static,
    Ret: Into<Reply>,
{
    fn call<'a>(
        &'a self,
        state: State,
        req: Request<Body>,
    ) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>> {
        let fut = (self)(state, req);
        Box::pin(async move {
            let res = fut.await;
            res.into()
        })
    }
}
