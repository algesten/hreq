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
/// | `Response<Into<Body>>`    | `Response` and fallback to [`Body`] |                  |
/// | `Result<Into<Body>, Into<Error>>` | See [`Body`]        |                          |
/// | `Result<Response<Into<Body>>, Into<Error>>` | `Response and falllback to [`Body`]  |
/// | `Option<Into<Reply>>`     | 404 on `None`               |                          |
///
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
