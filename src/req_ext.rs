use crate::deadline::Deadline;
use crate::res_ext::HeaderMapExt;
use crate::Agent;
use crate::Body;
use crate::Error;
use async_trait::async_trait;
use http::request;
use http::Uri;
use http::{Request, Response};
use once_cell::sync::Lazy;
use qstring::QString;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(feature = "tlsapi")]
use tls_api::TlsConnector;

#[async_trait]
pub trait RequestBuilderExt
where
    Self: Sized,
{
    fn query(self, key: &str, value: &str) -> Self;
    fn timeout(self, duration: Duration) -> Self;
    fn force_http2(self, force: bool) -> Self;
    fn with_body<B: Into<Body>>(self, body: B) -> http::Result<Request<Body>>;

    async fn send<B: Into<Body> + Send>(self, body: B) -> Result<Response<Body>, Error>;

    #[cfg(feature = "tlsapi")]
    async fn send_tls<B, Tls>(self, body: B) -> Result<Response<Body>, Error>
    where
        B: Into<Body> + Send,
        Tls: TlsConnector;
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

    async fn send<B: Into<Body> + Send>(self, body: B) -> Result<Response<Body>, Error> {
        let req = self.with_body(body)?;
        Ok(req.send().await?)
    }

    #[cfg(feature = "tlsapi")]
    async fn send_tls<B, Tls>(self, body: B) -> Result<Response<Body>, Error>
    where
        B: Into<Body> + Send,
        Tls: TlsConnector,
    {
        let req = self.with_body(body)?;
        Ok(req.send_tls::<Tls>().await?)
    }
}

#[async_trait]
pub trait RequestExt {
    //

    /// Get a header, ignore incorrect header values.
    fn header(&self, key: &str) -> Option<&str>;

    fn header_as<T: FromStr>(&self, key: &str) -> Option<T>;

    async fn send(self) -> Result<Response<Body>, Error>;

    #[cfg(feature = "tlsapi")]
    async fn send_tls<Tls: TlsConnector>(self) -> Result<Response<Body>, Error>;
}

#[async_trait]
impl<B: Into<Body> + Send> RequestExt for Request<B> {
    //
    fn header(&self, key: &str) -> Option<&str> {
        self.headers().get_str(key)
    }

    fn header_as<T: FromStr>(&self, key: &str) -> Option<T> {
        self.headers().get_as(key)
    }

    async fn send(self) -> Result<Response<Body>, Error> {
        //
        #[cfg(feature = "rustls")]
        let mut agent = Agent::<tls_api_rustls::TlsConnector>::new();
        #[cfg(not(feature = "rustls"))]
        let mut agent = Agent::<crate::tls_pass::TlsConnector>::new();

        let (parts, body) = self.into_parts();
        let req = Request::from_parts(parts, body.into());
        agent.send(req).await
    }

    #[cfg(feature = "tlsapi")]
    async fn send_tls<Tls: TlsConnector>(self) -> Result<Response<Body>, Error> {
        let mut agent = Agent::<Tls>::new();
        let (parts, body) = self.into_parts();
        let req = Request::from_parts(parts, body.into());
        agent.send(req).await
    }
}

static ID_COUNTER: Lazy<AtomicUsize> = Lazy::new(|| AtomicUsize::new(0));
static BUILDER_STORE: Lazy<Mutex<HashMap<usize, BuilderStore>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

struct BuilderStore {
    query_params: Vec<(String, String)>,
    req_params: RequestParams,
}

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

pub(crate) fn with_request_params<T, F: FnOnce(&RequestParams) -> T>(
    req: &http::Request<Body>,
    f: F,
) -> Option<T> {
    if let Some(val) = req.headers().get(HREQ_EXT_HEADER) {
        let id = val.to_str().unwrap().parse::<usize>().unwrap();
        let lock = BUILDER_STORE.lock().unwrap();
        if let Some(store) = lock.get(&id) {
            let t = f(&store.req_params);
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
