use super::path::PathMatch;
use crate::params::{AutoCharset, HReqParams};
use crate::Body;
use encoding_rs::Encoding;
use http::Request;
use std::str::FromStr;

pub trait ServerRequestExt {
    fn charset_decode(self, enable: bool) -> Self;
    fn charset_decode_target(self, encoding: &str) -> Self;
    fn content_decode(self, enable: bool) -> Self;
    fn path_param(&self, key: &str) -> Option<&str>;
    fn path_param_as<T: FromStr>(&self, key: &str) -> Option<T>;
    fn path_parms(&self) -> Vec<(&str, &str)>;
}

impl ServerRequestExt for Request<Body> {
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

    fn path_param(&self, key: &str) -> Option<&str> {
        self.extensions()
            .get::<PathMatch>()
            .and_then(|m| m.get_param(key))
    }

    fn path_param_as<T: FromStr>(&self, key: &str) -> Option<T> {
        self.path_param(key).and_then(|v| v.parse().ok())
    }

    fn path_parms(&self) -> Vec<(&str, &str)> {
        self.extensions()
            .get::<PathMatch>()
            .map(|m| m.all_params())
            .unwrap_or_else(|| vec![])
    }
}
