use super::Reply;
use crate::body::path_to_body;
use crate::Body;
use http::Request;
use std::future::Future;
use std::io;
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

use crate::server::ServerRequestExt;
use crate::Error;
use std::path::{Path, PathBuf};

/// Serve files from a directory.
///
/// Must be used with one path parameter `/path/:name`. Use a rest parameter `/path/*name`, to
/// files also from subdirectories.
///
///
/// Cannot serve files from parent paths. I.e. `/path/../foo.txt`.
///
/// Panics if the path served can not be [`std::fs::canonicalize`].
///
/// [`std::fs::canonicalize`]: https://doc.rust-lang.org/std/fs/fn.canonicalize.html
pub fn serve_dir(path: impl AsRef<Path>) -> impl Handler {
    let path = path.as_ref();

    match path.canonicalize() {
        Err(e) => {
            panic!("Failed to canonicalize path ({:?}): {:?}", path, e);
        }
        Ok(v) => DirHandler(v),
    }
}

struct DirHandler(PathBuf);

fn err(status: u16, msg: &str) -> http::Response<Body> {
    http::Response::builder()
        .status(status)
        .body(msg.into())
        .unwrap()
}

impl DirHandler {
    async fn handle(&self, to_serve: String) -> Result<http::Response<Body>, Error> {
        // Use the segment from the /*name appended to the dir we use.
        // This could be relative such as `"/path/to/serve"` + `"blah/../foo.txt"`
        let mut relative = self.0.clone();
        relative.push(&to_serve);

        // By canonicalizing we remove any `..`.
        let absolute = match relative.canonicalize() {
            Err(e) => {
                debug!("Failed to canonicalize ({}): {:?}", to_serve, e);
                return Ok(err(400, "Bad path"));
            }
            Ok(v) => v,
        };

        // This is a security check that the resolved doesn't go to a parent dir/file.
        // "/path/to/serve" + "../../../etc/passwd". It works because self.0 is canonicalized.
        if !absolute.starts_with(&self.0) {
            debug!("Path not under base path: {}", to_serve);
            return Ok(err(400, "Bad path"));
        }

        let body = match path_to_body(&absolute).await {
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    return Ok(err(404, "Not found"));
                } else {
                    warn!("File open failed ({:?}): {:?}", absolute, e);
                    return Ok(err(500, "Failed"));
                }
            }
            Ok(v) => v,
        };

        Ok(http::Response::builder().body(body).unwrap())
    }
}

impl Handler for DirHandler {
    fn call<'a>(&'a self, req: Request<Body>) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>> {
        let params = req.path_params();

        if params.is_empty() {
            let msg = "serve_dir() must be used with a path param. Example: /dir/*file";

            warn!("{}", msg);

            Box::pin(async move { err(500, msg).into() })
        } else if params.len() > 1 {
            let msg = "serve_dir() should be used with one path param. Example: /dir/*file";

            warn!("{}", msg);

            Box::pin(async move { err(500, msg).into() })
        } else {
            let to_serve = params[0].1.to_owned();

            Box::pin(async move {
                let ret = self.handle(to_serve).await;
                ret.into()
            })
        }
    }
}
