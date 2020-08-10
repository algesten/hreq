use crate::Body;
use crate::Error;
use http::Response;

/// Concrete return type from endpoints and middleware.
///
/// This type will rarely be used directly. The return signature of endpoints and
/// middleware is `impl Into<Reply>`. As a user of this crate you would prefer
/// using one of the return types possible via the `Into` trait.
///
/// | Return type               | Response                    | Notes                    |
/// |---------------------------|-----------------------------|--------------------------|
/// | `()`                      | text/plain                  | `""`                     |
/// | `&str`                    | text/plain                  | `"Hello"`                |
/// | `String`                  | text/plain                  | `"Hello".to_string()`    |
/// | `&String`                 | text/plain                  | `&"Hello".to_string()`   |
/// | `&[u8]`                   | application/octet-stream    | `b"abcdef"`              |
/// | `Vec<u8>`                 | application/octet-stream    | `vec![65,66,67]`         |
/// | `&Vec<u8>`                | application/octet-stream    | `&vec![65,66,67]`        |
/// | `std::fs::File`           | application/octet-stream    | NB. Only with tokio      |
/// | [`Body`]                  | Body determined             |                          |
/// | `Result<Into<Body>, Into<Error>>` | See [`Body`]        |                          |
/// | `Response<Into<Body>>`    | `Response` and fallback to [`Body`] |                  |
/// | `Result<Response<Into<Body>>, Into<Error>>` | `Response` and falllback to [`Body`] |
/// | `Option<Into<Reply>>`     | 404 on `None`               |                          |
///
/// # Examples
///
/// ```
/// async fn handle(req: http::Request<hreq::Body>) {
///     // 200 with empty reply
/// }
/// ```
///
/// ```
/// async fn handle(req: http::Request<hreq::Body>) -> &'static str {
///     "Hello World!" // 200 with text/plain body
/// }
/// ```
///
/// ```
/// async fn handle(req: http::Request<hreq::Body>) -> &'static str {
///     "Hello World!" // 200 with text/plain body
/// }
/// ```
///
/// ```
/// async fn handle(req: http::Request<hreq::Body>) -> String {
///     format!("Hello there {}", req.uri().path()) // 200 with text/plain body
/// }
/// ```
///
/// ```
/// async fn handle(req: http::Request<hreq::Body>) -> &'static [u8] {
///     &[79, 75, 10] // 200 with application/octet-stream
/// }
/// ```
///
/// ```
/// async fn handle(req: http::Request<hreq::Body>) -> Vec<u8> {
///     vec![79, 75, 10] // 200 with application/octet-stream
/// }
/// ```
///
/// ```
/// use hreq::Body;
///
/// async fn handle(req: http::Request<Body>) -> Body {
///     let data = vec![79, 75, 10];
///     let len = data.len() as u64;
///     let cursor = std::io::Cursor::new(data);
///     Body::from_sync_read(cursor, Some(len)) // check Body doc for more
/// }
/// ```
///
/// ```
/// use hreq::Body;
///
/// async fn handle(
///     req: http::Request<Body>
/// ) -> Result<&'static str, hreq::Error> {
///     let file = std::fs::File::open("/etc/bashrc")?;
///     Ok("File opened OK")
/// }
/// ```
///
/// ```
/// use hreq::prelude::*;
///
/// async fn handle(req: http::Request<hreq::Body>) -> http::Response<()> {
///     http::Response::builder()
///         .status(302)
///         .header("location", "/see-my-other-page")
///         .body(())
///         .unwrap()
/// }
/// ```
///
/// ```
/// use hreq::prelude::*;
///
/// async fn handle(
///     req: http::Request<hreq::Body>
/// ) -> http::Response<&'static str> {
///     http::Response::builder()
///         .header("X-My-Exotic-Header", "Cool")
///         .body("Hello World!")
///         .unwrap()
/// }
/// ```
///
/// ```
/// use hreq::prelude::*;
///
/// async fn handle(
///     req: http::Request<hreq::Body>
/// ) -> Result<http::Response<String>, hreq::Error> {
///     let no = 42;
///
///     Ok(http::Response::builder()
///         .header("X-My-Exotic-Header", "Cool")
///         .body(format!("Hello World!, {}", no))?)
/// }
/// ```
///
/// ```
/// use hreq::prelude::*;
///
/// async fn handle(
///     req: http::Request<hreq::Body>
/// ) -> Option<String> {
///     let no = 42;
///
///     Some(format!("Hello World!, {}", no))
/// }
/// ```
///
/// [`Body`]: ../struct.Body.html
#[derive(Debug)]
pub struct Reply(Result<Response<Body>, Error>);

impl Reply {
    /// Unwrap the inner results.
    pub fn into_result(self) -> Result<Response<Body>, Error> {
        self.0
    }

    fn from(b: Body) -> Reply {
        Reply(Ok(Response::builder().body(b).unwrap()))
    }
}

impl<'a> From<()> for Reply {
    fn from(v: ()) -> Self {
        Reply::from(v.into())
    }
}

impl<'a> From<&'a str> for Reply {
    fn from(v: &'a str) -> Self {
        Reply::from(v.into())
    }
}

impl<'a> From<&'a String> for Reply {
    fn from(v: &'a String) -> Self {
        Reply::from(v.into())
    }
}

impl From<String> for Reply {
    fn from(v: String) -> Self {
        Reply::from(v.into())
    }
}

impl<'a> From<&'a [u8]> for Reply {
    fn from(v: &'a [u8]) -> Self {
        Reply::from(v.into())
    }
}

impl From<Vec<u8>> for Reply {
    fn from(v: Vec<u8>) -> Self {
        Reply::from(v.into())
    }
}

impl<'a> From<&'a Vec<u8>> for Reply {
    fn from(v: &'a Vec<u8>) -> Self {
        Reply::from(v.into())
    }
}

impl From<Body> for Reply {
    fn from(v: Body) -> Self {
        Reply::from(v.into())
    }
}

impl<B> From<Response<B>> for Reply
where
    B: Into<Body>,
{
    fn from(v: Response<B>) -> Self {
        let (p, b) = v.into_parts();
        Reply(Ok(Response::from_parts(p, b.into())))
    }
}

impl<B, E> From<Result<B, E>> for Reply
where
    B: Into<Body>,
    E: Into<Error>,
{
    fn from(r: Result<B, E>) -> Self {
        Reply(
            r.map(|v| Response::builder().body(v.into()).unwrap())
                .map_err(|e| e.into()),
        )
    }
}

impl<B, E> From<Result<Response<B>, E>> for Reply
where
    B: Into<Body>,
    E: Into<Error>,
{
    fn from(r: Result<Response<B>, E>) -> Self {
        Reply(
            r.map(|v| {
                let (p, b) = v.into_parts();
                Response::from_parts(p, b.into())
            })
            .map_err(|e| e.into()),
        )
    }
}

impl<R> From<Option<R>> for Reply
where
    R: Into<Reply>,
{
    fn from(r: Option<R>) -> Self {
        match r {
            Some(r) => r.into(),
            None => Reply(Ok(Response::builder()
                .status(404)
                .body("not found".into())
                .unwrap())),
        }
    }
}
