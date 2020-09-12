//! Extension trait for `http::request::Builder`

use crate::params::{AutoCharset, HReqParams};
use crate::Body;
use encoding_rs::Encoding;
use http::response;
use http::Response;
use serde::Serialize;
use std::time::Duration;

/// Extends [`http::response::Builder`] with ergonomic extras for hreq.
///
/// These extensions are part of the primary goal of hreq to provide a "User first API".
///
/// [`http::response::Builder`]: https://docs.rs/http/latest/http/response/struct.Builder.html
pub trait ResponseBuilderExt
where
    Self: Sized,
{
    /// Set a timeout for the response, including sending the body.
    ///
    /// If the timeout is reached, the current operation is aborted with a 500.
    ///
    /// ```
    /// use hreq::prelude::*;
    /// use std::time::Duration;
    ///
    /// async fn handle(req: http::Request<hreq::Body>) -> http::Response<&'static str> {
    ///     http::Response::builder()
    ///         .timeout(Duration::from_nanos(1))
    ///         .body("Hello World!")
    ///         .unwrap()
    /// }
    /// ```
    ///
    /// [`Error::Io`]: enum.Error.html#variant.Io
    /// [`Error::is_timeout()`]: enum.Error.html#method.is_timeout
    fn timeout(self, duration: Duration) -> Self;

    /// This is an alias for `.timeout()` without having to construct a `Duration`.
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// async fn handle(req: http::Request<hreq::Body>) -> http::Response<&'static str> {
    ///     http::Response::builder()
    ///         .timeout_millis(10_000)
    ///         .body("Hello World!")
    ///         .unwrap()
    /// }
    /// ```
    fn timeout_millis(self, millis: u64) -> Self;

    /// Toggle automatic response body charset encoding. Defaults to `true`.
    ///
    /// hreq encodes the response body of text MIME types according to the `charset` in
    /// the `content-type` response header:
    ///
    ///   * `content-type: text/html; charset=iso8859-1`
    ///
    /// The behavior is triggered for any MIME type starting with `text/`. Because we're in rust,
    /// there's an underlying assumption that the source of the response body is in `utf-8`,
    /// but this can be changed using [`charset_encode_source`].
    ///
    /// Setting this to `false` disables any automatic charset encoding of the response body.
    ///
    /// # Examples
    ///
    /// You have plain text in a rust String (which is always utf-8) and you want an
    /// http response with `iso8859-1` (aka `latin-1`). The default assumption
    /// is that the source is in `utf-8`. You only need a `content-type` header.
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// async fn handle(req: http::Request<hreq::Body>) -> http::Response<&'static str> {
    ///     // This is a &str in rust default utf-8
    ///     let content = "Und in die Bäumen hängen Löwen und Bären";
    ///
    ///     http::Response::builder()
    ///         .header("content-type", "text/html; charset=iso8859-1")
    ///         .body(content)
    ///         .unwrap()
    /// }
    /// ```
    ///
    /// Or if you have a plain text file in utf-8.
    ///
    /// ```
    /// use hreq::prelude::*;
    /// use std::fs::File;
    ///
    /// #[cfg(feature = "tokio")]
    /// async fn handle(req: http::Request<hreq::Body>) -> http::Response<std::fs::File> {
    ///     http::Response::builder()
    ///         // This header converts the body to iso8859-1
    ///         .header("content-type", "text/plain; charset=iso8859-1")
    ///         .body(File::open("my-utf8-file.txt").unwrap())
    ///         .unwrap()
    /// }
    /// ```
    ///
    /// [`charset_encode_source`]: trait.ResponseBuilderExt.html#tymethod.charset_encode_source
    fn charset_encode(self, enable: bool) -> Self;

    /// Sets how to interpret response body source. Defaults to `utf-8`.
    ///
    /// When doing charset conversion of the response body, this set how to interpret the
    /// source of the body.
    ///
    /// The setting works together with the mechanic described in [`charset_encode`], i.e.
    /// it is triggered by the presence of a `charset` part in a `content-type` request header
    /// with a `text` MIME.
    ///
    ///   * `content-type: text/html; charset=iso8859-1`
    ///
    /// Notice if the [`Body`] is a rust `String` or `&str`, this setting is ignored since
    /// the internal represenation is always `utf-8`.
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// async fn handle(req: http::Request<hreq::Body>) -> http::Response<Vec<u8>> {
    ///     // おはよう世界 in EUC-JP.
    ///     let euc_jp = vec![164_u8, 170, 164, 207, 164, 232, 164, 166, 192, 164, 179, 166];
    ///
    ///     http::Response::builder()
    ///         // This header converts the body from EUC-JP to Shift-JIS
    ///         .charset_encode_source("EUC-JP")
    ///         .header("content-type", "text/html; charset=Shift_JIS")
    ///         .body(euc_jp)
    ///         .unwrap()
    /// }
    /// ```
    ///
    /// [`charset_encode`]: trait.ResponseBuilderExt.html#tymethod.charset_encode
    /// [`Body`]: struct.Body.html
    fn charset_encode_source(self, encoding: &str) -> Self;

    /// Whether to use the `content-encoding` response header. Defaults to `true`.
    ///
    /// By default hreq encodes compressed body data automatically. The behavior is
    /// triggered by setting the response header `content-encoding: gzip`.
    ///
    /// If the body data provided to hreq is already compressed we might need turn off
    /// this default behavior.
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// async fn handle(req: http::Request<hreq::Body>) -> http::Response<Vec<u8>> {
    ///     // imagine we got some already gzip compressed data
    ///     let already_compressed: Vec<u8> = vec![];
    ///
    ///     http::Response::builder()
    ///         .header("content-encoding", "gzip")
    ///         .content_encode(false) // do not do extra encoding
    ///         .body(already_compressed)
    ///         .unwrap()
    /// }
    /// ```
    fn content_encode(self, enabled: bool) -> Self;

    /// Toggle ability to read the response body into memory.
    ///
    /// When sending a response body, it's usually a good idea to read the entire body
    /// (up to some limit) into memory. Doing so avoids using transfer-encoding chunked
    /// when the content length can be determined.
    ///
    /// By default, hreq will attempt to prebuffer up to 256kb response body.
    ///
    /// Use this toggle to turn this behavior off.
    fn prebuffer_response_body(self, enable: bool) -> Self;

    /// Finish building the response by providing an object serializable to JSON.
    ///
    /// Objects made serializable with serde_derive can be automatically turned into
    /// bodies. This sets both `content-type` and `content-length`.
    ///
    /// # Example
    ///
    /// ```
    /// use hreq::prelude::*;
    /// use hreq::Body;
    /// use serde_derive::Serialize;
    ///
    /// #[derive(Serialize)]
    /// struct MyJsonThing {
    ///   name: String,
    ///   age: String,
    /// }
    ///
    /// async fn handle(req: http::Request<Body>) -> http::Response<Body> {
    ///     let json = MyJsonThing {
    ///         name: "Karl Kajal".into(),
    ///         age: "32".into(),
    ///     };
    ///
    ///     http::Response::builder()
    ///         .with_json(&json)
    ///         .unwrap()
    /// }
    /// ```
    fn with_json<B: Serialize + ?Sized>(self, body: &B) -> http::Result<Response<Body>>;
}

impl ResponseBuilderExt for response::Builder {
    fn timeout(self, duration: Duration) -> Self {
        with_hreq_params(self, |params| {
            params.timeout = Some(duration);
        })
    }

    fn timeout_millis(self, millis: u64) -> Self {
        self.timeout(Duration::from_millis(millis))
    }

    fn charset_encode(self, enable: bool) -> Self {
        with_hreq_params(self, |params| {
            params.charset_tx.toggle_target(enable);
        })
    }

    fn charset_encode_source(self, encoding: &str) -> Self {
        with_hreq_params(self, |params| {
            let enc = Encoding::for_label(encoding.as_bytes());
            if enc.is_none() {
                warn!("Unknown character encoding: {}", encoding);
            }
            params.charset_tx.source = AutoCharset::Set(enc.unwrap());
        })
    }

    fn content_encode(self, enable: bool) -> Self {
        with_hreq_params(self, |params| {
            params.content_encode = enable;
        })
    }

    fn prebuffer_response_body(self, enable: bool) -> Self {
        with_hreq_params(self, |params| {
            params.prebuffer = enable;
        })
    }

    fn with_json<B: Serialize + ?Sized>(self, body: &B) -> http::Result<Response<Body>> {
        let body = Body::from_json(body);
        self.body(body)
    }
}

fn get_or_insert<T: Send + Sync + 'static, F: FnOnce() -> T>(
    builder: &mut response::Builder,
    f: F,
) -> &mut T {
    let ext = builder.extensions_mut().expect("Unwrap extensions");
    if ext.get::<T>().is_none() {
        ext.insert(f());
    }
    ext.get_mut::<T>().unwrap()
}

fn with_hreq_params<F: FnOnce(&mut HReqParams)>(
    mut builder: response::Builder,
    f: F,
) -> response::Builder {
    let params = get_or_insert(&mut builder, HReqParams::new);
    f(params);
    builder
}
