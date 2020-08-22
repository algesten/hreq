//! Extension trait for `http::request::Builder`

use super::req_ext::RequestExt;
use crate::params::QueryParams;
use crate::params::{AutoCharset, HReqParams};
use crate::uri_ext::HostPort;
use crate::Body;
use crate::Error;
use async_trait::async_trait;
use encoding_rs::Encoding;
use http::request;
use http::{Request, Response};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

/// Extends [`http::request::Builder`] with ergonomic extras for hreq.
///
/// These extensions are part of the primary goal of hreq to provide a "User first API".
///
/// [`http::request::Builder`]: https://docs.rs/http/latest/http/request/struct.Builder.html
#[async_trait]
pub trait RequestBuilderExt
where
    Self: Sized,
{
    /// Set a query parameter to be appended at the end the request URI.
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// Request::get("http://my-api/query")
    ///     .query("api-key", "secret sauce")
    ///     .call().block();
    /// ```
    ///
    /// Same name query parameters are appended, not replaced. I.e.
    /// `.query("x", "1").query("x", "2")` will result in a uri with `?x=1&x=2`.
    ///
    /// Due to limitations in the http API, the provided URI is only amended upon calling
    /// some variant of hreq `.send()`.
    fn query(self, key: &str, value: &str) -> Self;

    /// Set a timeout for the entire request, including reading the body.
    ///
    /// If the timeout is reached, the current operation is aborted with an [`Error::Io`]. To
    /// easily distinguish the timeout errors, there's a convenience [`Error::is_timeout()`] call.
    ///
    /// ```
    /// use hreq::prelude::*;
    /// use std::time::Duration;
    ///
    /// let req = Request::get("https://httpbin.org/get/")
    ///     .timeout(Duration::from_nanos(1))
    ///     .call().block();
    ///
    /// assert!(req.is_err());
    /// assert!(req.unwrap_err().is_timeout());
    /// ```
    ///
    /// [`Error::Io`]: enum.Error.html#variant.Io
    /// [`Error::is_timeout()`]: enum.Error.html#method.is_timeout
    fn timeout(self, duration: Duration) -> Self;

    /// This is an alias for `.timeout()` without having to construct a `Duration`.
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// let req = Request::get("https://httpbin.org/get/")
    ///     .timeout_millis(10_000)
    ///     .call().block();
    ///
    /// assert!(req.is_err());
    /// assert!(req.unwrap_err().is_timeout());
    /// ```
    fn timeout_millis(self, millis: u64) -> Self;

    /// Force the request to use http2.
    ///
    /// Normally whether to use http2 is negotiated as part of TLS (https). The TLS feature is
    /// called [ALPN]. In some situations you might want to force the use of http2, such as
    /// when there is no TLS involved. The http2 spec calls this having ["prior knowledge"].
    ///
    /// Forcing http2 when the server only talks http1.1 is doomed to fail.
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    /// use std::time::Duration;
    ///
    /// let req = Request::get("http://my-insecure-http2-server/")
    ///     .force_http2(true)
    ///     .call().block();
    /// ```
    ///
    /// [ALPN]: https://en.wikipedia.org/wiki/Application-Layer_Protocol_Negotiation
    /// ["prior knowledge"]: https://http2.github.io/http2-spec/#known-http
    fn force_http2(self, force: bool) -> Self;

    /// Toggle automatic request body charset encoding. Defaults to `true`.
    ///
    /// hreq encodes the request body of text MIME types according to the `charset` in
    /// the `content-type` request header:
    ///
    ///   * `content-type: text/html; charset=iso8859-1`
    ///
    /// The behavior is triggered for any MIME type starting with `text/`. Because we're in rust,
    /// there's an underlying assumption that the source of the request body is in `utf-8`,
    /// but this can be changed using [`charset_encode_source`].
    ///
    /// Setting this to `false` disables any automatic charset encoding of the request body.
    ///
    /// # Examples
    ///
    /// You have plain text in a rust String (which is always utf-8) and you want to
    /// POST it as `iso8859-1` (aka `latin-1`) request body. The default assumption
    /// is that the source is in `utf-8`. You only need a `content-type` header.
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// // This is a &str in rust default utf-8
    /// let content = "Und in die Bäumen hängen Löwen und Bären";
    ///
    /// let req = Request::post("https://my-euro-server/")
    ///     // This header converts the body to iso8859-1
    ///     .header("content-type", "text/plain; charset=iso8859-1")
    ///     .send(content).block();
    /// ```
    ///
    /// Or if you have a plain text file in utf-8.
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    /// use std::fs::File;
    ///
    /// let req = Request::post("https://my-euro-server/")
    ///     // This header converts the body to iso8859-1
    ///     .header("content-type", "text/plain; charset=iso8859-1")
    ///     .send(File::open("my-utf8-file.txt").unwrap()).block();
    /// ```
    ///
    /// If you want to disable automatic conversion of the request body.
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    /// use std::fs::File;
    ///
    /// let req = Request::post("https://my-euro-server/")
    ///     // Disable outgoing charset encoding.
    ///     .charset_encode(false)
    ///     // This header has no effect now.
    ///     .header("content-type", "text/plain; charset=iso8859-1")
    ///     .send(File::open("my-iso8859-1-file.txt").unwrap()).block();
    /// ```
    ///
    /// [`charset_encode_source`]: trait.RequestBuilderExt.html#tymethod.charset_encode_source
    fn charset_encode(self, enable: bool) -> Self;

    /// Sets how to interpret request body source. Defaults to `utf-8`.
    ///
    /// When doing charset conversion of the request body, this set how to interpret the
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
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// // おはよう世界 in EUC-JP.
    /// let euc_jp = [164_u8, 170, 164, 207, 164, 232, 164, 166, 192, 164, 179, 166];
    ///
    /// let req = Request::post("https://my-japan-server/")
    ///     // This header converts the body from EUC-JP to Shift-JIS
    ///     .charset_encode_source("EUC-JP")
    ///     .header("content-type", "text/plain; charset=Shift_JIS")
    ///     .send(&euc_jp[..]).block();
    /// ```
    ///
    /// [`charset_encode`]: trait.RequestBuilderExt.html#tymethod.charset_encode
    /// [`Body`]: struct.Body.html
    fn charset_encode_source(self, encoding: &str) -> Self;

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
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// let mut resp = Request::get("https://my-euro-server/")
    ///     .call().block().unwrap();
    ///
    /// assert_eq!(resp.header("content-type"), Some("text/html; charset=iso8859-1"));
    ///
    /// // this is now automatically converted to utf-8.
    /// let string = resp.body_mut().read_to_string().block().unwrap();
    /// ```
    ///
    /// [`charset_decode_target`]: trait.RequestBuilderExt.html#tymethod.charset_decode_target
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
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// // Originating server sends content in Shift_JIS
    /// let mut resp = Request::get("https://my-shift-jis-server/")
    ///      // I want content in EUC-JP
    ///     .charset_decode_target("EUC-JP")
    ///     .call().block().unwrap();
    ///
    /// assert_eq!(resp.header("content-type"), Some("text/html; charset=Shift_JIS"));
    ///
    /// // this is now converted to EUC_JP
    /// let vec = resp.body_mut().read_to_vec().block().unwrap();
    /// ```
    ///
    /// [`charset_decode`]: trait.RequestBuilderExt.html#tymethod.charset_decode
    /// [`Body.read_to_string()`]: struct.Body.html#method.read_to_string
    fn charset_decode_target(self, encoding: &str) -> Self;

    /// Whether to use the `content-encoding` request header. Defaults to `true`.
    ///
    /// By default hreq encodes compressed body data automatically. The behavior is
    /// triggered by setting the request header `content-encoding: gzip`.
    ///
    /// If the body data provided to hreq is already compressed we might need turn off
    /// the default behavior.
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// // imagine we got some already gzip compressed data
    /// let already_compressed: Vec<u8> = vec![];
    ///
    /// let mut resp = Request::post("https://server-for-compressed/")
    ///     .header("content-encoding", "gzip")
    ///     .content_encode(false) // do not do extra encoding
    ///     .send(already_compressed).block().unwrap();
    /// ```
    fn content_encode(self, enabled: bool) -> Self;

    /// Whether to use the `content-encoding` response header. Defaults to `true`.
    ///
    /// By default hreq decodes compressed body data automatically. The behavior is
    /// triggered by when hreq encounters the response header `content-encoding: gzip`.
    ///
    /// If we want to keep the body data compressed, we can turn off the default behavior.
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// let mut resp = Request::get("https://server-for-compressed/")
    ///     .header("accept-encoding", "gzip")
    ///     .content_decode(false) // do not do decompress
    ///     .call().block().unwrap();
    ///
    /// // this content is still compressed
    /// let compressed = resp.body_mut().read_to_vec();
    /// ```
    fn content_decode(self, enabled: bool) -> Self;

    /// Buffer size to enable resending body on 307 and 308 redirects.
    ///
    /// A POST/PUT request encountering 301 and 302 redirects will by
    /// de-facto standard follow the redirect with GET + empty body
    /// instead of the original method.
    ///
    /// The 307 and 308 responses are explicitly for preserving the
    /// original request method and they should also re-send the
    /// original body to the redirected location.
    ///
    /// For body re-send hreq must be able to repeat the body data sent
    /// when encountering a 307/308. However hreq can't hold on to
    /// indefinitely large amounts of body data _just in case_ it gets
    /// one of these redirect codes.
    ///
    /// This parameter sets how much body data we should retain in memory
    /// in case of a re-send before "letting go" and not being able to
    /// respond with a body to 307/308.
    ///
    /// The default value is `0` because solutions relying on 307/308 are
    /// not the norm and we don't want the extra memory footprint for all
    /// cases where it is not used.
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// Request::post("https://my-redirect-server/")
    ///     .redirect_body_buffer(1024) // up to 1kb buffer for resend
    ///     .send("This body will be re-sent on 307")
    ///     .block().unwrap();
    /// ```
    ///
    /// hreq does a "best effort" in not using up the entire buffer.
    /// Imagine sending a 2GB large file, the remote server would
    /// most likely respond with 307/308 long before the entire body has
    /// been uploaded.
    ///
    /// This can further be improved using by setting a `Expect: 100-continue`
    /// header, which would build in a small delay before sending the body
    /// letting the server respond with the redirect first.
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// let file = std::fs::File::open("my-big-movie.m4v").unwrap();
    ///
    /// Request::post("https://my-redirect-server/")
    ///     .redirect_body_buffer(1024 * 1024) // up to 1mb buffer for resend
    ///     .header("expect", "100-continue")  // delay for 100-continue or redirect
    ///     .send(file)
    ///     .block().unwrap();
    /// ```
    fn redirect_body_buffer(self, size: usize) -> Self;

    /// Override the host, port and TLS setting of where to connect to.
    ///
    /// This is mostly used for testing.
    ///
    /// With this, hreq will ignore the scheme, host and port provided
    /// in the [`Uri`] when opening the TCP connection. The rest of the request
    /// handling will still use the [`Uri`] (cookies etc).
    ///
    /// The override is only used for connections to the host/port found in [`Uri`],
    /// and not when following redirects to other host/ports.
    ///
    /// The override host name is also used for TLS certificate matching.
    ///
    /// [`Uri`]: https://docs.rs/http/latest/http/uri/struct.Uri.html
    fn with_override(self, host: &str, port: u16, tls: bool) -> Self;

    /// Disables verification of server certificate.
    ///
    /// This is generally a bad idea. With verification turned off, anyone can intercept
    /// the TLS traffic, present a random certificate and pretend to be the server.
    /// In today's world, no production code should disable this.
    ///
    /// However it might be appropriate to use in some localhost developer scenarios,
    /// unit tests, etc.
    fn tls_disable_server_cert_verify(self, disable: bool) -> Self;

    /// Finish building the request by providing something as [`Body`].
    ///
    /// [`Body`] implements a number of conventient `From` traits. We can trivially construct
    /// a body from a `String`, `&str`, `Vec<u8>`, `&[u8]`, `File` and more (see the [`From`
    /// traits] in the body doc).
    ///
    /// `with_body` is just a shorthand. The following ways the construct a `Request`
    /// ends up with exactly the same result.
    ///
    /// ```
    /// use hreq::prelude::*;
    /// use hreq::Body;
    ///
    /// let req1 = Request::post("http://foo")
    ///   .with_body("Hello world");
    ///
    /// let body2 = Body::from_str("Hello world");
    ///
    /// let req2 = Request::post("http://foo")
    ///   .body(body2);
    ///
    /// let body3: Body = "Hello world".into();
    ///
    /// let req3 = Request::post("http://foo")
    ///   .body(body3);
    /// ```
    ///
    /// [`Body`]: struct.Body.html
    /// [`From` traits]: struct.Body.html#implementations
    fn with_body<B: Into<Body>>(self, body: B) -> http::Result<Request<Body>>;

    /// Send the built request with provided [`Body`].
    ///
    /// Note: The type signature of this function is complicated because rust doesn't yet
    /// support the `async` keyword in traits. You can think of this function as:
    ///
    /// ```ignore
    /// async fn send<B>(self, body: B) -> Result<Response<Body>, Error>
    /// where
    ///     B: Into<Body> + Send;
    /// ```
    ///
    /// This is a shortcut to both provide a body and send the request. The following
    /// statements are roughly equivalent.
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// let res1 = Request::get("https://httpbin.org/get")
    ///     .call().block();
    ///
    /// let res2 = Request::get("https://httpbin.org/get")
    ///     .with_body(()) // constructs the Request
    ///     .unwrap()
    ///     .send().block();
    /// ```
    ///
    /// Creates a default configured [`Agent`] used for this request only. The agent will
    /// follow redirects and provide some retry-logic for idempotent request methods.
    ///
    /// If you need connection pooling over several requests or finer grained control over
    /// retries or redirects, instantiate an [`Agent`] and send the request through it.
    ///
    /// [`Body`]: struct.Body.html
    /// [`Agent`]: struct.Agent.html
    async fn send<B>(self, body: B) -> Result<Response<Body>, Error>
    where
        B: Into<Body> + Send;

    /// Alias for sending an empty body and is the same as doing `.call()`.
    ///
    /// Typically used for get requests.
    ///
    /// Note: The type signature of this function is complicated because rust doesn't yet
    /// support the `async` keyword in traits. You can think of this function as:
    ///
    /// ```ignore
    /// async fn call(self) -> Result<Response<Body>, Error>;
    /// ```
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// let res = Request::get("https://httpbin.org/get")
    ///     .call().block();
    /// ```
    async fn call(self) -> Result<Response<Body>, Error>;

    /// Finish building the request by providing an object serializable to JSON.
    ///
    /// Objects made serializable with serde_derive can be automatically turned into
    /// bodies. This sets both `content-type` and `content-length`.
    ///
    /// # Example
    ///
    /// ```
    /// use serde_derive::Serialize;
    /// use hreq::prelude::*;
    /// use hreq::Body;
    ///
    /// #[derive(Serialize)]
    /// struct MyJsonThing {
    ///   name: String,
    ///   age: String,
    /// }
    ///
    /// let json = MyJsonThing {
    ///   name: "Karl Kajal".into(),
    ///   age: "32".into(),
    /// };
    ///
    /// let req = http::Request::post("http://foo")
    ///   .with_json(&json);
    /// ```
    fn with_json<B: Serialize + ?Sized>(self, body: &B) -> http::Result<Request<Body>>;

    /// Send the built request with provided JSON object serialized to a body.
    ///
    /// Note: The type signature of this function is complicated because rust doesn't yet
    /// support the `async` keyword in traits. You can think of this function as:
    ///
    /// ```ignore
    /// async fn send_json<B>(self, body: &B) -> Result<Response<Body>, Error>
    /// where
    ///     B: Serialize + ?Sized + Send + Sync;
    /// ```
    ///
    /// This is a shortcut to both provide a JSON body and send the request.
    async fn send_json<B>(self, body: &B) -> Result<Response<Body>, Error>
    where
        B: Serialize + ?Sized + Send + Sync;
}

#[async_trait]
impl RequestBuilderExt for request::Builder {
    //
    fn query(self, key: &str, value: &str) -> Self {
        let mut this = self;
        let qparams = get_or_insert(&mut this, QueryParams::new);
        qparams.params.push((key.into(), value.into()));
        this
    }

    fn timeout(self, duration: Duration) -> Self {
        with_hreq_params(self, |params| {
            params.timeout = Some(duration);
        })
    }

    fn timeout_millis(self, millis: u64) -> Self {
        self.timeout(Duration::from_millis(millis))
    }

    fn force_http2(self, enabled: bool) -> Self {
        with_hreq_params(self, |params| {
            params.force_http2 = enabled;
        })
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
                return;
            }
            params.charset_tx.source = AutoCharset::Set(enc.unwrap());
        })
    }

    fn charset_decode(self, enable: bool) -> Self {
        with_hreq_params(self, |params| {
            params.charset_rx.toggle_source(enable);
        })
    }

    fn charset_decode_target(self, encoding: &str) -> Self {
        with_hreq_params(self, |params| {
            let enc = Encoding::for_label(encoding.as_bytes());
            if enc.is_none() {
                warn!("Unknown character encoding: {}", encoding);
                return;
            }
            params.charset_rx.target = AutoCharset::Set(enc.unwrap());
        })
    }

    fn content_encode(self, enable: bool) -> Self {
        with_hreq_params(self, |params| {
            params.content_encode = enable;
        })
    }

    fn content_decode(self, enable: bool) -> Self {
        with_hreq_params(self, |params| {
            params.content_decode = enable;
        })
    }

    fn redirect_body_buffer(self, size: usize) -> Self {
        with_hreq_params(self, |params| {
            params.redirect_body_buffer = size;
        })
    }

    fn with_override(self, host: &str, port: u16, tls: bool) -> Self {
        with_hreq_params(self, |params| {
            params.with_override = Some(Arc::new(HostPort::new(host, port, tls)));
        })
    }

    fn tls_disable_server_cert_verify(self, disable: bool) -> Self {
        with_hreq_params(self, |params| {
            params.tls_disable_verify = disable;
        })
    }

    fn with_body<B: Into<Body>>(self, body: B) -> http::Result<Request<Body>> {
        self.body(body.into())
    }

    async fn send<B>(self, body: B) -> Result<Response<Body>, Error>
    where
        B: Into<Body> + Send,
    {
        let req = self.with_body(body)?;
        Ok(req.send().await?)
    }

    async fn call(self) -> Result<Response<Body>, Error> {
        Ok(self.send(()).await?)
    }

    fn with_json<B: Serialize + ?Sized>(self, body: &B) -> http::Result<Request<Body>> {
        let body = Body::from_json(body);
        self.with_body(body)
    }

    async fn send_json<B>(self, body: &B) -> Result<Response<Body>, Error>
    where
        B: Serialize + ?Sized + Send + Sync,
    {
        let req = self.with_json(body)?;
        Ok(req.send().await?)
    }
}

fn get_or_insert<T: Send + Sync + 'static, F: FnOnce() -> T>(
    builder: &mut request::Builder,
    f: F,
) -> &mut T {
    let ext = builder.extensions_mut().expect("Unwrap extensions");
    if ext.get::<T>().is_none() {
        ext.insert(f());
    }
    ext.get_mut::<T>().unwrap()
}

fn with_hreq_params<F: FnOnce(&mut HReqParams)>(
    mut builder: request::Builder,
    f: F,
) -> request::Builder {
    let params = get_or_insert(&mut builder, HReqParams::new);
    f(params);
    builder
}
