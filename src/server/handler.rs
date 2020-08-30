use super::Reply;
use crate::head_ext::HeaderMapExt;
use crate::Body;
use http::Request;
use httpdate::{fmt_http_date, parse_http_date};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::time::SystemTime;

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
    async fn handle(
        &self,
        to_serve: String,
        if_modified_since: Option<SystemTime>,
    ) -> Result<http::Response<Body>, Error> {
        // Use the segment from the /*name appended to the dir we use.
        // This could be relative such as `"/path/to/serve"` + `"blah/../foo.txt"`
        let mut relative = self.0.clone();
        relative.push(&to_serve);

        // By canonicalizing we remove any `..`.
        let mut absolute = match relative.canonicalize() {
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    return Ok(err(404, "Not found"));
                } else {
                    warn!("Failed to canonicalize ({}): {:?}", to_serve, e);
                    return Ok(err(400, "Bad request"));
                }
            }
            Ok(v) => v,
        };

        // This is a security check that the resolved doesn't go to a parent dir/file.
        // "/path/to/serve" + "../../../etc/passwd". It works because self.0 is canonicalized.
        if !absolute.starts_with(&self.0) {
            debug!("Path not under base path: {}", to_serve);
            return Ok(err(404, "Bad path"));
        }

        // TODO configurable index files.
        if absolute.is_dir() {
            absolute.push("index.html");
        }

        if let Some(since) = if_modified_since {
            if let Ok(modified) = absolute.metadata().and_then(|v| v.modified()) {
                // for files that updated, since will be earlier than modified.
                if let Ok(diff) = modified.duration_since(since) {
                    // The web format has a resultion of seconds: Fri, 15 May 2015 15:34:21 GMT
                    // So the diff must be less than a second.
                    if diff.as_secs_f32() < 1.0 {
                        return Ok(http::Response::builder()
                            .status(304)
                            // https://tools.ietf.org/html/rfc7232#section-4.1
                            //
                            // The server generating a 304 response MUST generate any of the
                            // following header fields that would have been sent in a 200 (OK)
                            // response to the same request: Cache-Control, Content-Location, Date,
                            // ETag, Expires, and Vary.
                            .header("cache-control", "must-revalidate")
                            .header("last-modified", fmt_http_date(modified))
                            .body(().into())
                            .unwrap());
                    }
                }
            }
        }

        let body = match Body::from_path_io(&absolute).await {
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

        Ok(http::Response::builder()
            .header("cache-control", "must-revalidate")
            .body(body)
            .unwrap())
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
            let if_modified_since = req
                .headers()
                .get_as::<String>("if-modified-since")
                .and_then(|v| parse_http_date(&v).ok());

            Box::pin(async move {
                let ret = self.handle(to_serve, if_modified_since).await;
                ret.into()
            })
        }
    }
}
