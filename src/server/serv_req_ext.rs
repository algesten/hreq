use super::path::PathMatch;
use crate::params::{AutoCharset, HReqParams};
use crate::Body;
use encoding_rs::Encoding;
use http::Request;
use std::str::FromStr;

/// Extends [`http::Request`] with ergonomic extras for server requests to hreq.
///
/// [`http::Request`]: https://docs.rs/http/latest/http/request/struct.Request.html
pub trait ServerRequestExt {
    /// Get the value from a named parameter.
    ///
    /// # Example
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
    fn path_param(&self, key: &str) -> Option<&str>;

    /// Get the value from a named parameter coerced to type.
    ///
    /// Rust fabulous `FromStr` trait means we can quickly parse a value into something else.
    ///
    /// # Example
    ///
    ///  ```
    ///  use hreq::prelude::*;
    ///
    ///  async fn start_server() {
    ///     let mut server = Server::new();
    ///
    ///     server.at("/get_number/:number").get(hello_there);
    ///
    ///     server.listen(3000).await.unwrap();
    ///  }
    ///
    ///  async fn hello_there(req: http::Request<Body>) -> String {
    ///      let number: usize = req.path_param_as("number").unwrap();
    ///      format!("The number is: {}", number)
    ///  }
    ///  ```
    fn path_param_as<T: FromStr>(&self, key: &str) -> Option<T>;

    /// Enumerate all named parameters with their values.
    ///
    /// # Example
    ///
    ///  ```
    ///  use hreq::prelude::*;
    ///
    ///  async fn start_server() {
    ///     let mut server = Server::new();
    ///
    ///     server.at("/:verb/:name").get(verb_name);
    ///
    ///     server.listen(3000).await.unwrap();
    ///  }
    ///
    ///  async fn verb_name(req: http::Request<Body>) -> String {
    ///      // Called with `/goodbye/martin`, these params would
    ///      // be: `vec![("verb", "goodbye"), ("name", ",martin")]
    ///      let params = req.path_params();
    ///      format!("{} {}", params[0].1, params[1].1)
    ///  }
    ///  ```
    fn path_params(&self) -> Vec<(&str, &str)>;

    /// Toggle automatic response body charset decoding. Defaults to `true`.
    ///
    /// hreq decodes the response body of text MIME types according to the `charset` in
    /// the `content-type` response header:
    ///
    ///   * `content-type: text/html; charset=iso8859-1`
    ///
    /// The behavior is triggered for any MIME type starting with `text/`. Because we're in rust,
    /// there's an underlying assumption that the wanted encoding is `utf-8`, but this can be
    /// changed using [`charset_decode_target`].
    ///
    /// [`charset_decode_target`]: trait.ServerRequestExt.html#tymethod.charset_decode_target
    fn charset_decode(self, enable: bool) -> Self;

    /// Sets how to output the response body. Defaults to `utf-8`.
    ///
    /// When doing charset conversion of the response body, this sets how to output the
    /// the response body.
    ///
    /// The setting works together with the mechanic described in [`charset_decode`], i.e.
    /// it is triggered by the presence of a `charset` part in a `content-type` response header
    /// with a `text` MIME.
    ///
    ///   * `content-type: text/html; charset=iso8859-1`
    ///
    /// Notice if you use the [`Body.read_to_string()`] method, this setting is ignored since
    /// rust's internal representation is always `utf-8`.
    ///
    /// [`charset_decode`]: trait.ServerRequestExt.html#tymethod.charset_decode
    /// [`Body.read_to_string()`]: ../struct.Body.html#method.read_to_string
    fn charset_decode_target(self, encoding: &str) -> Self;

    /// Whether to use the `content-encoding` response header. Defaults to `true`.
    ///
    /// By default hreq decodes compressed body data automatically. The behavior is
    /// triggered by when hreq encounters the response header `content-encoding: gzip`.
    ///
    /// If we want to keep the body data compressed, we can turn off the default behavior.
    fn content_decode(self, enable: bool) -> Self;
}

impl ServerRequestExt for Request<Body> {
    fn path_param(&self, key: &str) -> Option<&str> {
        self.extensions()
            .get::<PathMatch>()
            .and_then(|m| m.get_param(key))
    }

    fn path_param_as<T: FromStr>(&self, key: &str) -> Option<T> {
        self.path_param(key).and_then(|v| v.parse().ok())
    }

    fn path_params(&self) -> Vec<(&str, &str)> {
        self.extensions()
            .get::<PathMatch>()
            .map(|m| m.all_params())
            .unwrap_or_else(|| vec![])
    }

    fn charset_decode(self, enable: bool) -> Self {
        let (mut parts, body) = self.into_parts();
        let params = parts.extensions.get_mut::<HReqParams>().expect("");

        params.charset_rx.toggle_target(enable);

        let mut body = body.unconfigure();
        body.configure(params, &parts.headers, true);

        http::Request::from_parts(parts, body)
    }

    fn charset_decode_target(self, encoding: &str) -> Self {
        let (mut parts, body) = self.into_parts();
        let params = parts.extensions.get_mut::<HReqParams>().expect("");

        if let Some(enc) = Encoding::for_label(encoding.as_bytes()) {
            params.charset_rx.target = AutoCharset::Set(enc);
        } else {
            warn!("Unknown character encoding: {}", encoding);
        }

        let mut body = body.unconfigure();
        body.configure(params, &parts.headers, true);

        http::Request::from_parts(parts, body)
    }

    fn content_decode(self, enable: bool) -> Self {
        let (mut parts, body) = self.into_parts();
        let params = parts.extensions.get_mut::<HReqParams>().expect("");

        params.content_decode = enable;

        let mut body = body.unconfigure();
        body.configure(params, &parts.headers, true);

        http::Request::from_parts(parts, body)
    }
}
