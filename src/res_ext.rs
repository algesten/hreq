use http::Response;
use std::str::FromStr;

pub trait ResponseExt {
    /// Get a header, ignore incorrect header values.
    fn header(&self, key: &str) -> Option<&str>;

    fn status_code(&self) -> u16;

    fn header_as<T: FromStr>(&self, key: &str) -> Option<T>;
}

impl<B> ResponseExt for Response<B> {
    fn header(&self, key: &str) -> Option<&str> {
        self.headers().get_str(key)
    }

    fn status_code(&self) -> u16 {
        self.status().as_u16()
    }

    fn header_as<T: FromStr>(&self, key: &str) -> Option<T> {
        self.headers().get_as(key)
    }
}

pub trait HeaderMapExt {
    /// Get a header, ignore incorrect header values.
    fn get_str(&self, key: &str) -> Option<&str>;

    fn get_as<T: FromStr>(&self, key: &str) -> Option<T>;
}

impl HeaderMapExt for http::HeaderMap {
    //
    fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.to_str().ok())
    }

    fn get_as<T: FromStr>(&self, key: &str) -> Option<T> {
        self.get_str(key).and_then(|v| v.parse().ok())
    }
}
