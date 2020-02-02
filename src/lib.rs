#![warn(clippy::all)]
//! hreq is a user first async http client.
//!
//! The goals of this library are:
//!
//! * User first API built on the http crate.
//! * Blocking or async.
//! * Pure rust.
//!
//! ```no_run
//! use hreq::prelude::*;
//!
//! fn main() -> Result<(), hreq::Error> {
//!     // Use plain http API request builder with
//!     // trait extensions for extra convenience
//!     // in handling query parameters and other
//!     // request configurations.
//!     let response = Request::builder()
//!         .uri("https://myapi.acme.com/ingest")
//!         .query("api_key", "secret")
//!         .send(()).block()?;
//!
//!     // More convenience on the http response.
//!     // Like shortcuts to read or parse
//!     // response headers.
//!     let x_req_id =
//!         response.header_as::<usize>("x-req-id")
//!         .unwrap();
//!
//!     // A Body type with easy ways to
//!     // get the content.
//!     let mut body = response.into_body();
//!     let contents = body.read_to_string().block()?;
//!
//!     assert_eq!(contents, "Hello world!");
//!
//!     Ok(())
//! }
//! ```
//!

#[macro_use]
extern crate log;

mod agent;
mod async_impl;
mod block_ext;
mod body;
mod charset;
mod conn;
mod conn_http1;
mod conn_http2;
mod deadline;
mod either;
mod error;
mod h1;
mod proto;
mod req_ext;
mod reqb_ext;
mod res_ext;
mod tokio;
mod uri_ext;

#[cfg(feature = "tls")]
mod tls;

#[cfg(all(test, feature = "async-std"))]
mod test;

pub(crate) use futures_io::{AsyncBufRead, AsyncRead, AsyncWrite};

pub use crate::agent::Agent;
pub use crate::async_impl::AsyncRuntime;
pub use crate::block_ext::BlockExt;
pub use crate::body::Body;
pub use crate::error::Error;
pub use crate::req_ext::RequestExt;
pub use crate::reqb_ext::RequestBuilderExt;
pub use crate::res_ext::ResponseExt;
pub use http;

pub mod prelude {
    //! A "prelude" for users of the hreq crate.
    //!
    //! The idea is that by importing the entire contents of this module to get all the
    //! essentials of the hreq crate.
    //!
    //! ```
    //! # #![allow(warnings)]
    //! use hreq::prelude::*;
    //! ```

    #[doc(no_inline)]
    pub use crate::{BlockExt, RequestBuilderExt, RequestExt, ResponseExt};
    #[doc(no_inline)]
    pub use http::{Request, Response};
}

use crate::conn::Connection;
use crate::conn::ProtocolImpl;
use crate::proto::Protocol;
use crate::tokio::to_tokio;
use crate::uri_ext::UriExt;

pub(crate) trait Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl Stream for Box<dyn Stream> {}

pub(crate) async fn connect(uri: &http::Uri, force_http2: bool) -> Result<Connection, Error> {
    let hostport = uri.host_port()?;
    // "host:port"
    let addr = hostport.to_string();

    let (stream, alpn_proto) = {
        // "raw" tcp
        let tcp = AsyncRuntime::current().connect_tcp(&addr).await?;

        #[cfg(feature = "tls")]
        {
            use crate::either::Either;
            use crate::tls::wrap_tls;
            if hostport.is_tls() {
                // wrap in tls
                let (tls, proto) = wrap_tls(tcp, hostport.host()).await?;
                (Either::A(tls), proto)
            } else {
                // use tcp
                (Either::B(tcp), Protocol::Unknown)
            }
        }

        #[cfg(not(feature = "tls"))]
        (tcp, Protocol::Unknown)
    };

    let proto = if force_http2 {
        Protocol::Http2
    } else {
        alpn_proto
    };

    open_stream(addr, stream, proto).await
}

pub(crate) async fn open_stream(
    addr: String,
    stream: impl Stream,
    proto: Protocol,
) -> Result<Connection, Error> {
    if proto == Protocol::Http2 {
        let (h2, h2conn) = h2::client::handshake(to_tokio(stream)).await?;
        // drives the connection independently of the h2 api surface.
        AsyncRuntime::current().spawn(async {
            if let Err(err) = h2conn.await {
                // this is expected to happen when the connection disconnects
                trace!("Error in connection: {:?}", err);
            }
        });
        Ok(Connection::new(addr, ProtocolImpl::Http2(h2)))
    } else {
        let (h1, h1conn) = h1::handshake(stream);
        // drives the connection independently of the h1 api surface
        AsyncRuntime::current().spawn(async {
            if let Err(err) = h1conn.await {
                // this is expected to happen when the connection disconnects
                trace!("Error in connection: {:?}", err);
            }
        });
        Ok(Connection::new(addr, ProtocolImpl::Http1(h1)))
    }
}
