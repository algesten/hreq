use crate::deadline::Deadline;
use crate::head_ext::HeaderMapExt;
use crate::uri_ext::HostPort;
use encoding_rs::Encoding;
use http::Uri;
use once_cell::sync::Lazy;
use qstring::QString;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub(crate) struct HReqParams {
    pub local_addr: SocketAddr,
    pub remote_addr: SocketAddr,
    pub req_start: Option<Instant>,
    pub timeout: Option<Duration>,
    pub force_http2: bool,
    pub charset_tx: CharsetConfig,
    pub charset_rx: CharsetConfig,
    pub content_encode: bool,
    pub content_decode: bool,
    pub redirect_body_buffer: usize,
    pub with_override: Option<Arc<HostPort<'static>>>,
    pub tls_disable_verify: bool,
}

#[derive(Clone, Debug)]
pub struct CharsetConfig {
    pub source: AutoCharset,
    pub target: AutoCharset,
}

impl CharsetConfig {
    pub fn toggle_source(&mut self, on: bool) {
        if on != self.source.is_on() {
            self.source = if on {
                AutoCharset::Auto
            } else {
                AutoCharset::Off
            };
        }
    }

    pub fn toggle_target(&mut self, on: bool) {
        if on != self.target.is_on() {
            self.target = if on {
                AutoCharset::Auto
            } else {
                AutoCharset::Off
            };
        }
    }

    /// Resolve the from/to encoding to use.
    ///
    /// if `is_incoming`, then headers apply to the source encoding.
    /// if not `is_inoming`, then headers apply to the target encoding.
    pub fn resolve(
        &self,
        is_incoming: bool,
        headers: &http::header::HeaderMap,
        override_source: Option<&'static Encoding>,
    ) -> Option<(&'static Encoding, &'static Encoding)> {
        // nothing to do if either source or target encoding is off.
        if self.source.is_off() && override_source.is_none() || self.target.is_off() {
            return None;
        }

        let header_charset = charset_from_headers(headers)
            .map(|s| s.as_bytes())
            .and_then(Encoding::for_label)
            .unwrap_or(encoding_rs::UTF_8);

        let s_enc = if let Some(enc) = override_source {
            // override takes precedence
            enc
        } else {
            if is_incoming {
                self.source.resolve(header_charset)
            } else {
                self.source.resolve(encoding_rs::UTF_8)
            }
        };

        let t_enc = if is_incoming {
            self.target.resolve(encoding_rs::UTF_8)
        } else {
            self.target.resolve(header_charset)
        };

        Some((s_enc, t_enc))
    }
}

#[derive(Clone, Debug)]
pub enum AutoCharset {
    Off,
    Auto,
    Set(&'static Encoding),
}

impl AutoCharset {
    pub fn is_off(&self) -> bool {
        !self.is_on()
    }

    pub fn is_on(&self) -> bool {
        if let AutoCharset::Off = self {
            return false;
        }
        true
    }

    pub fn resolve(&self, def: &'static Encoding) -> &'static Encoding {
        match self {
            AutoCharset::Off => panic!("resolve on AutoCharset::Off"),
            AutoCharset::Auto => def,
            AutoCharset::Set(val) => val,
        }
    }
}

// placeholder until we set the real ones from request
static DEFAULT_ADDR: Lazy<SocketAddr> = Lazy::new(|| "0.0.0.0:1".parse().unwrap());

impl HReqParams {
    pub fn new() -> Self {
        HReqParams {
            local_addr: DEFAULT_ADDR.clone(),
            remote_addr: DEFAULT_ADDR.clone(),
            req_start: None,
            timeout: None,
            force_http2: false,
            charset_tx: CharsetConfig {
                source: AutoCharset::Auto,
                target: AutoCharset::Auto,
            },
            charset_rx: CharsetConfig {
                source: AutoCharset::Auto,
                target: AutoCharset::Auto,
            },
            content_encode: true,
            content_decode: true,
            redirect_body_buffer: 0,
            with_override: None,
            tls_disable_verify: false,
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

    #[cfg(feature = "server")]
    pub fn copy_from_request(&mut self, req_params: &HReqParams) {
        self.req_start = req_params.req_start;
        self.local_addr = req_params.local_addr;
        self.remote_addr = req_params.remote_addr;
    }
}

fn charset_from_headers(headers: &http::header::HeaderMap) -> Option<&str> {
    // only consider text/ content-types
    fn is_text(s: &&str) -> bool {
        s.starts_with("text/")
    }

    // text/html; charset=utf-8
    fn after_semi(s: &str) -> Option<&str> {
        s.split(';').last()
    }

    // charset=utf-8
    fn after_eq(s: &str) -> Option<&str> {
        s.split('=').last()
    }

    headers
        .get_str("content-type")
        .filter(is_text)
        .and_then(after_semi)
        .and_then(after_eq)
}

/// Apply the query parameters to the request and also ensure there are RequestParams.
/// in the extensions.
pub fn resolve_hreq_params(mut parts: http::request::Parts) -> http::request::Parts {
    if let Some(query_params) = parts.extensions.remove::<QueryParams>() {
        query_params.apply(&mut parts);
    }
    if parts.extensions.get::<HReqParams>().is_none() {
        parts.extensions.insert(HReqParams::new());
    }
    let hreq_params = parts.extensions.get_mut::<HReqParams>().unwrap();
    hreq_params.mark_request_start();
    parts
}

#[derive(Clone, Debug, Default)]
pub(crate) struct QueryParams {
    pub params: Vec<(String, String)>,
}

impl QueryParams {
    pub fn new() -> Self {
        QueryParams {
            ..Default::default()
        }
    }

    fn apply(self, parts: &mut http::request::Parts) {
        let mut uri_parts = parts.uri.clone().into_parts();

        // Construct new instance of PathAndQuery with our modified query.
        let new_path_and_query = {
            //
            let (path, query) = uri_parts
                .path_and_query
                .as_ref()
                .map(|p| (p.path(), p.query().unwrap_or("")))
                .unwrap_or(("", ""));

            let mut qs = QString::from(query);
            for (key, value) in self.params.into_iter() {
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
}
