use crate::res_ext::HeaderMapExt;
use crate::Agent;
use crate::Body;
use crate::Error;
use async_trait::async_trait;
use http::{Request, Response};
use std::str::FromStr;

#[async_trait]
pub trait RequestExt {
    //

    /// Get a header, ignore incorrect header values.
    fn header(&self, key: &str) -> Option<&str>;

    fn header_as<T: FromStr>(&self, key: &str) -> Option<T>;

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
