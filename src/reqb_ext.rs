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
    ///     .send(()).block();
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
    ///     .send(()).block();
    ///
    /// assert!(req.is_err());
    /// assert!(req.unwrap_err().is_timeout());
    /// ```
    ///
    /// [`Error::Io`]: enum.Error.html#variant.Io
    /// [`Error::is_timeout()`]: enum.Error.html#method.is_timeout
    fn timeout(self, duration: Duration) -> Self;

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
    ///     .send(()).block();
    /// ```
    ///
    /// [ALPN]: https://en.wikipedia.org/wiki/Application-Layer_Protocol_Negotiation
    /// ["prior knowledge"]: https://http2.github.io/http2-spec/#known-http
    fn force_http2(self, force: bool) -> Self;

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
    ///     .send(()).block();
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

    fn force_http2(self, enabled: bool) -> Self {
        with_builder_store(self, |store| {
            store.req_params.force_http2 = enabled;
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
}

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
}

impl RequestParams {
    pub fn new() -> Self {
        RequestParams {
            ..Default::default()
        }
    }

    pub fn mark_request_start(&mut self) {
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

const HREQ_EXT_HEADER: &str = "x-hreq-ext";

/// Get the current request parameters associated with the request.
pub(crate) fn with_request_params<T, F: FnOnce(&mut RequestParams) -> T>(
    req: &http::Request<Body>,
    f: F,
) -> Option<T> {
    if let Some(val) = req.headers().get(HREQ_EXT_HEADER) {
        let id = val.to_str().unwrap().parse::<usize>().unwrap();
        let mut lock = BUILDER_STORE.lock().unwrap();
        if let Some(store) = lock.get_mut(&id) {
            let t = f(&mut store.req_params);
            return Some(t);
        }
    }
    None
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

/// Apply the parameters in the separate storage before executing the request.
pub fn resolve_hreq_ext(parts: &mut http::request::Parts) -> Option<RequestParams> {
    if let Some(val) = parts.headers.remove(HREQ_EXT_HEADER) {
        let id = val.to_str().unwrap().parse::<usize>().unwrap();
        let mut lock = BUILDER_STORE.lock().unwrap();
        if let Some(store) = lock.remove(&id) {
            let req_params = store.invoke(parts);
            return Some(req_params);
        }
    }
    None
}
