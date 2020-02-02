use http::Response;
use std::str::FromStr;

/// Extends [`http::request::Response`] with ergonomic extras for hreq.
///
/// These extensions are part of the primary goal of hreq to provide a "User first API".
///
/// [`http::request::Response`]: https://docs.rs/http/0.2.0/http/request/struct.Response.html
pub trait ResponseExt {
    /// Quickly read a header value as a `&str`.
    ///
    /// A header value can in theory contain any byte value 32 to 255 (inclusive), excluding
    /// 127 (DEL). That means all possible header values are not representable as a `&str`.
    ///
    /// In practice it's incredibly rare for any header value to be outside US-ASCII, which
    /// means for the vast majority of cases `&str` is fine.
    ///
    /// This convenience methods treats header values not representable as ascii as `None`.
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// let res = Request::get("https://www.google.com")
    ///     .send(()).block().unwrap();
    ///
    /// let x_frame_opts = res.header("x-frame-options").unwrap();
    ///
    /// assert_eq!(x_frame_opts, "SAMEORIGIN");
    /// ```
    fn header(&self, key: &str) -> Option<&str>;

    /// Quickly parse a header value into _something else_.
    ///
    /// Rust fabulous `FromStr` trait means we can quickly parse a value into something else.
    /// For example, if we know a header `x-req-id` is supposed to have a  numeric 64 bit value
    /// and we want that number, we can do:
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// let res = Request::get("https://my-api")
    ///     .send(()).block().unwrap();
    ///
    /// let req_id: u64 = res.header_as("x-req-id").unwrap();
    /// ```
    fn header_as<T: FromStr>(&self, key: &str) -> Option<T>;

    /// Get the response status code as a `u16`
    ///
    /// These two are equivalent:
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// let res = Request::get("https://www.google.com")
    ///     .send(()).block().unwrap();
    ///
    /// assert_eq!(res.status_code(), 200);
    ///
    /// assert_eq!(res.status().as_u16(), 200);
    /// ```
    fn status_code(&self) -> u16;
}

impl<B> ResponseExt for Response<B> {
    fn header(&self, key: &str) -> Option<&str> {
        self.headers().get_str(key)
    }

    fn header_as<T: FromStr>(&self, key: &str) -> Option<T> {
        self.headers().get_as(key)
    }

    fn status_code(&self) -> u16 {
        self.status().as_u16()
    }
}

/// Internal extension of `HeaderMap`.
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
