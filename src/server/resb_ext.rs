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
    fn timeout(self, duration: Duration) -> Self;

    fn timeout_millis(self, millis: u64) -> Self;

    fn charset_encode(self, enable: bool) -> Self;

    fn charset_encode_source(self, encoding: &str) -> Self;

    fn content_encode(self, enabled: bool) -> Self;

    fn with_body<B: Into<Body>>(self, body: B) -> http::Result<Response<Body>>;

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

    fn with_body<B: Into<Body>>(self, body: B) -> http::Result<Response<Body>> {
        self.body(body.into())
    }

    fn with_json<B: Serialize + ?Sized>(self, body: &B) -> http::Result<Response<Body>> {
        let body = Body::from_json(body);
        self.with_body(body)
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
