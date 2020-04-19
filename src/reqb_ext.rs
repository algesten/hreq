//! Extension trait for `http::request::Builder`

use crate::deadline::Deadline;
use crate::req_ext::RequestExt;
use crate::Body;
use crate::Error;
use async_trait::async_trait;
use http::request;
use http::Uri;
use http::{Request, Response};
use once_cell::sync::Lazy;
use qstring::QString;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Extends [`http::request::Builder`] with ergonomic extras for hreq.
///
/// These extensions are part of the primary goal of hreq to provide a "User first API".
///
/// [`http::request::Builder`]: https://docs.rs/http/0.2.0/http/request/struct.Builder.html
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
    /// let req = Request::get("https://www.google.com/")
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
    /// let req = Request::get("https://www.google.com/")
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
    /// let res1 = Request::get("https://www.google.com")
    ///     .call().block();
    ///
    /// let res2 = Request::get("https://www.google.com")
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
    /// ```
    /// use hreq::prelude::*;
    ///
    /// let res = Request::get("https://www.google.com")
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
    /// ```ignore
    /// use hreq::Body;
    /// use serde_derive::Serialize;
    ///
    /// #[derive(Serialize)]
    /// struct MyJsonThing {
    ///   name: String,
    ///   age: String,
    /// }
    ///
    /// let json = MyJsonThing {
    ///   name: "Karl Kajal",
    ///   age: "32",
    /// };
    ///
    /// let req = Request::post("http://foo")
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
        with_builder_store(self, |store| {
            store.query_params.push((key.into(), value.into()));
        })
    }

    fn timeout(self, duration: Duration) -> Self {
        with_builder_store(self, |store| {
            store.req_params.timeout = Some(duration);
        })
    }

    fn timeout_millis(self, millis: u64) -> Self {
        self.timeout(Duration::from_millis(millis))
    }

    fn force_http2(self, enabled: bool) -> Self {
        with_builder_store(self, |store| {
            store.req_params.force_http2 = enabled;
        })
    }

    fn charset_encode(self, enable: bool) -> Self {
        with_builder_store(self, |store| {
            store.req_params.charset_encode = enable;
        })
    }

    fn charset_encode_source(self, encoding: &str) -> Self {
        with_builder_store(self, |store| {
            let enc = Encoding::for_label(encoding.as_bytes());
            if enc.is_none() {
                warn!("Unknown character encoding: {}", encoding);
            }
            store.req_params.charset_encode_source = enc;
        })
    }

    fn charset_decode(self, enable: bool) -> Self {
        with_builder_store(self, |store| {
            store.req_params.charset_decode = enable;
        })
    }

    fn charset_decode_target(self, encoding: &str) -> Self {
        with_builder_store(self, |store| {
            let enc = Encoding::for_label(encoding.as_bytes());
            if enc.is_none() {
                warn!("Unknown character encoding: {}", encoding);
            }
            store.req_params.charset_decode_target = enc;
        })
    }

    fn content_encode(self, enable: bool) -> Self {
        with_builder_store(self, |store| {
            store.req_params.content_encode = enable;
        })
    }

    fn content_decode(self, enable: bool) -> Self {
        with_builder_store(self, |store| {
            store.req_params.content_decode = enable;
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

const HREQ_EXT_HEADER: &str = "x-hreq-ext";
static ID_COUNTER: Lazy<AtomicUsize> = Lazy::new(|| AtomicUsize::new(0));
static BUILDER_STORE: Lazy<Mutex<HashMap<usize, BuilderStore>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

struct BuilderStore {
    query_params: Vec<(String, String)>,
    req_params: RequestParams,
}

/// Extra parameters associated with the request being built.
///
/// In the request we keep a header `x-hreq-ext` with a unique number. That number corresponds
/// to a `BuilderStore` in a shared storage. Upon executing the request, we apply the extra
/// parameters to the request before anything else.
///
/// TODO: Avoid leaking memory for requests that are never sent by using an Arc<BuilderStore>
/// as extension in the Builder together with a Weak<BuilderStore> in the shared Mutex + cleanup.
///
/// TODO: Simplify this by asking the http API guys if we can have a `extensions_mut()`
/// accessor in the Builder.
#[derive(Clone, Copy, Debug, Default)]
pub struct RequestParams {
    pub req_start: Option<Instant>,
    pub timeout: Option<Duration>,
    pub force_http2: bool,
    pub charset_encode: bool,
    pub charset_encode_source: Option<&'static Encoding>,
    pub charset_decode: bool,
    pub charset_decode_target: Option<&'static Encoding>,
    pub content_encode: bool,
    pub content_decode: bool,
}

use encoding_rs::Encoding;

impl RequestParams {
    pub fn new() -> Self {
        RequestParams {
            charset_encode: true,
            charset_decode: true,
            content_encode: true,
            content_decode: true,
            ..Default::default()
        }
    }

    fn mark_request_start(&mut self) {
        if self.req_start.is_none() {
            self.req_start = Some(Instant::now());
        }
    }

    pub fn deadline(&self) -> Deadline {
        Deadline::new(self.req_start, self.timeout)
    }
}

impl BuilderStore {
    fn new() -> Self {
        BuilderStore {
            query_params: vec![],
            req_params: RequestParams::new(),
        }
    }

    fn invoke(self, parts: &mut http::request::Parts) -> RequestParams {
        let mut uri_parts = parts.uri.clone().into_parts();

        // Construct new instance of PathAndQuery with our modified query.
        if !self.query_params.is_empty() {
            let new_path_and_query = {
                //
                let (path, query) = uri_parts
                    .path_and_query
                    .as_ref()
                    .map(|p| (p.path(), p.query().unwrap_or("")))
                    .unwrap_or(("", ""));

                let mut qs = QString::from(query);
                for (key, value) in self.query_params.into_iter() {
                    qs.add_pair((key, value));
                }

                // PathAndQuery has no API for modifying any fields. This seems to be our only
                // option to get a new instance of it using the public API.
                let tmp: Uri = format!("http://fake{}?{}", path, qs).parse().unwrap();
                let tmp_parts = tmp.into_parts();
                tmp_parts.path_and_query.unwrap()
            };

            // This is good. We can change the PathAndQuery field.
            uri_parts.path_and_query = Some(new_path_and_query);

            let new_uri = Uri::from_parts(uri_parts).unwrap();
            parts.uri = new_uri;
        }

        self.req_params
    }
}

fn with_builder_store<F: FnOnce(&mut BuilderStore)>(
    mut builder: http::request::Builder,
    f: F,
) -> http::request::Builder {
    if let Some(headers) = builder.headers_mut() {
        let val = headers
            .entry(HREQ_EXT_HEADER)
            .or_insert_with(|| ID_COUNTER.fetch_add(1, Ordering::Relaxed).into());
        let id = val.to_str().unwrap().parse::<usize>().unwrap();
        let mut lock = BUILDER_STORE.lock().unwrap();
        let hreq_ext = lock.entry(id).or_insert_with(BuilderStore::new);
        f(hreq_ext);
    }
    builder
}

/// Apply the parameters held in the separate storage.
pub fn resolve_hreq_ext(req: http::request::Request<Body>) -> http::request::Request<Body> {
    let (mut parts, body) = req.into_parts();
    let mut req_params = None;
    if let Some(val) = parts.headers.remove(HREQ_EXT_HEADER) {
        let id = val.to_str().unwrap().parse::<usize>().unwrap();
        let mut lock = BUILDER_STORE.lock().unwrap();
        if let Some(store) = lock.remove(&id) {
            req_params = Some(store.invoke(&mut parts))
        }
    }
    let mut req_params = req_params.unwrap_or_else(RequestParams::new);
    req_params.mark_request_start();
    // after this we get the request parameters from the req.extensions()
    parts.extensions.insert(req_params);
    http::Request::from_parts(parts, body)
}
