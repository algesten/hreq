//! Extension trait for `http::request::Request`

use crate::client::Agent;
use crate::head_ext::HeaderMapExt;
use crate::Body;
use crate::Error;
use async_trait::async_trait;
use http::{Request, Response};
use std::str::FromStr;

/// Extends [`http::request::Request`] with ergonomic extras for hreq.
///
/// These extensions are part of the primary goal of hreq to provide a "User first API".
///
/// [`http::request::Request`]: https://docs.rs/http/latest/http/request/struct.Request.html
#[async_trait]
pub trait RequestExt {
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
    /// let req = Request::get("https://www.google.com")
    ///     .header("x-my-head", "whatnow")
    ///     .with_body(()).unwrap();
    ///
    /// let value = req.header("x-my-head").unwrap();
    ///
    /// assert_eq!(value, "whatnow");
    /// ```
    fn header(&self, key: &str) -> Option<&str>;

    /// Quickly parse a header value into _something else_.
    ///
    /// Rust fabulous `FromStr` trait means we can quickly parse a value into something else.
    /// For example, if we know a header `x-req-id` is supposed to have a  numeric 64 bit value
    /// and we want that number, we can do:
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// let req = Request::get("https://my-api")
    ///     .header("x-req-id", 42)
    ///     .with_body(()).unwrap();
    ///
    /// let req_id: u64 = req.header_as("x-req-id").unwrap();
    ///
    /// assert_eq!(req_id, 42);
    /// ```
    fn header_as<T: FromStr>(&self, key: &str) -> Option<T>;

    /// Send this request.
    ///
    /// Note: The type signature of this function is complicated because rust doesn't yet
    /// support the `async` keyword in traits. You can think of this function as:
    ///
    /// ```ignore
    /// async fn send(self) -> Result<Response<Body>, Error>;
    /// ```
    ///
    /// Creates a default configured [`Agent`] used for this request only. The agent will
    /// follow redirects and provide some retry-logic for idempotent request methods.
    ///
    /// If you need connection pooling over several requests or finer grained control over
    /// retries or redirects, instantiate an [`Agent`] and send the request through it.
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// let req = Request::get("https://www.google.com")
    ///     .with_body(()).unwrap();
    ///
    /// req.send().block();
    /// ```
    ///
    /// [`Agent`]: struct.Agent.html
    async fn send(self) -> Result<Response<Body>, Error>;
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
        let mut agent = Agent::new();

        let (parts, body) = self.into_parts();
        let req = Request::from_parts(parts, body.into());
        agent.send(req).await
    }
}
