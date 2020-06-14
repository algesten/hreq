#![warn(clippy::all)]
//! hreq is a user first async http client.
//!
//! ### Early days
//!
//! This library needs road testing. Bug reports and PRs are very welcome!
//!
//! ### Principles
//!
//! The principles of this library are:
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
//!         .call().block()?;
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
//! # User first
//!
//! _User first_ means that in situations where there are trade offs
//! between ergonomics and performance, or ergonomics and correctness,
//! extra weight will be put towards ergonomics. hreq does not attempt
//! to win any performance or benchmark competitions at the same time
//! as it should not be particularly slow or wasteful of system
//! resources.
//!
//! # http crate
//!
//! Most rust http clients use some variant of the [http crate]. The
//! typical way is to copy the http crate source into the local source
//! tree and extend it from there.
//!
//! However copying the source comes with a trade off for the
//! user. When writing a service that uses both a web server and
//! client crate, one often ends up with similar, but not exactly the
//! same versions of types like `http::Request` and `http::Response`.
//!
//! hreq works using extension traits only. It re-exports the http
//! crate, but does not copy it into the source tree. It therefore
//! adheres strictly to the exact API definition as set out by the
//! http crate as well as avoids furthering the confusion of having
//! multiple types with the same name.
//!
//! # Blocking and async
//!
//! Rust's async story is fantastic, but not every situation requires
//! async.  hreq "fakes" being a blocking library by default having a
//! very minial tokio runtime ([`rt-core`]) combined with a `.block()`
//! call that is placed where we expect an `.await` in an async
//! situation.
//!
//! ```
//! use hreq::prelude::*;
//!
//! let res = Request::get("https://www.google.com")
//!     .call().block();
//! ```
//!
//! ## Why?
//!
//! hreq is async through-and-through and ultimately relies on an
//! async variant of [`TcpStream`] for it to function. `TcpStream` in
//! turn needs to be provided by the runtime (tokio or async-std)
//! because the TCP socket is one of those things that is tightly
//! integrated with the async event loop.
//!
//! There are talks of rust providing a simple single threaded
//! executor as part of the std lib. This only solves half of the
//! problem since `TcpStream` is tightly integrated with the event
//! loop and (so far) that is not considered for std.
//!
//! # Async runtime
//!
//! The async runtime is "pluggable" and comes in some different
//! flavors.
//!
//!   * `Smol`. Requires the feature `smol`. Supports `.block()`
//!   * `AsyncStd`. Requires the feature `async-std`. Supports
//!     `.block()`.
//!   * `TokioSingle`. The default option. A minimal tokio `rt-core`
//!     which executes calls in one single thread. It does nothing
//!     until the current thread blocks on a future using `.block()`.
//!   * `TokioShared`. Picks up on a shared runtime by using a
//!     [`Handle`]. This runtime cannot use the `.block()` extension
//!     trait since that requires having a direct connection to the
//!     tokio [`Runtime`].
//!   * `TokioOwned`. Uses a preconfigured tokio [`Runtime`] that is
//!     "handed over" to hreq.
//!
//! How to configure the options is explained in [`AsyncRuntime`].
//!
//! # Agent, redirect and retries
//!
//! All calls in hreq goes through an [`Agent`]. The agent provides
//! three main functions:
//!
//!   * Retries
//!   * Connection pooling
//!   * Cookie handling
//!
//! However the simplest use of hreq creates a new agent for every call, which
//! means connection pooling and cookie handling is only happening to a limited
//! extent (when following redirects).
//!
//! ```
//! use hreq::prelude::*;
//!
//! let res1 = Request::get("https://www.google.com")
//!     .call().block();  // creates a new agent
//!
//! // this call doesn't reuse any cookies or connections.
//! let res2 = Request::get("https://www.google.com")
//!     .call().block();  // creates another new agent
//! ```
//!
//! To use connection pooling and cookies between multiple calls, we need to
//! create an agent.
//!
//! ```
//! use hreq::prelude::*;
//! use hreq::Agent;
//!
//! let mut agent = Agent::new();
//!
//! let req1 = Request::get("https://www.google.com")
//!     .with_body(()).unwrap();
//!
//! let res1 = agent.send(req1).block();
//!
//! let req2 = Request::get("https://www.google.com")
//!     .with_body(()).unwrap();
//!
//! // this call (tries to) reuse the connection in
//! // req1 since we are using the same agent.
//! let res2 = agent.send(req2).block();
//! ```
//!
//! ## Retries
//!
//! The internet is a dangerous place and http requests fail all the time.
//! hreq tries to be helpful and has a built in retries by default. However
//! it will only retry when appropriate.
//!
//! * The default number of retries is 5 with a backoff going 125,
//!   250, 500, 1000 milliseconds.
//! * Only for idempotent methods: GET, HEAD, OPTIONS, TRACE, PUT and DELETE.
//! * Only when the  encountered error is retryable, such as BrokenPipe,
//!   ConnectionAborted, ConnectionReset, Interrupted.
//!
//! To disable retries, one must use a configured agent:
//!
//! ```
//! use hreq::prelude::*;
//! use hreq::Agent;
//!
//! let mut agent = Agent::new();
//! agent.retries(0); // disable all retries
//!
//! let req = Request::get("https://www.google.com")
//!     .with_body(()).unwrap();
//!
//! let res = agent.send(req).block();
//! ```
//!
//! ## Redirects
//!
//! By default hreq follows up to 5 redirects. This currently works only for
//! "standard" redirects such as 301, 302 etc. There are plans to also support
//! 307 and 308 with the [Expect-100] mechanic. Redirects can be turned off
//! by using an explicit agent in the same way as for retries.
//!
//! # Compression
//!
//! hreq supports content compression both for requests and responses. The
//! feature is enabled by receving or setting the `content-encoding` header
//! to `gzip`. Currently hreq only supports `gzip`.
//!
//! ## Example request with gzip body:
//!
//! ```
//! use hreq::prelude::*;
//!
//! let res = Request::post("https://my-special-server/content")
//!   .header("content-encoding", "gzip") // enables gzip compression
//!   .send("request that is compressed".to_string()).block();
//! ```
//!
//! The automatic compression and decompression can be turned off,
//! see [`content_encode`] and [`content_decode`].
//!
//! # Charset
//!
//! Similarly to body compression hreq provides an automatic way of
//! encoding and decoding text in request/response bodies. Rust uses
//! utf-8 for `String` and assumes text bodies should be encoded as
//! utf-8. Using the `content-type` we can change how hreq handles
//! both requests and responses.
//!
//! ## Example sending an iso-8859-1 encoded body.
//!
//! ```no_run
//! use hreq::prelude::*;
//!
//! // This is a &str in rust default utf-8
//! let content = "Und in die Bäumen hängen Löwen und Bären";
//!
//! let req = Request::post("https://my-euro-server/")
//!     // This header converts the body to iso8859-1
//!     .header("content-type", "text/plain; charset=iso8859-1")
//!     .send(content).block();
//! ```
//!
//! Receiving bodies of other charset is mostly transparent to the
//! user. It will decode the body to utf-8 if a `content-type` header
//! is present in the response.
//!
//! Only content types with a mime type `text/*` will be decoded.
//!
//! The charset encoding does not need to work only with utf-8.  It
//! can transcode between different encodings as appropriate.  See
//! [`charset_encode_source`] and [`charset_decode_target`].
//!
//! # Body size
//!
//! Depending on how a body is provided to a request hreq may or may
//! not be able to know the total body size. For example, when the
//! body provided as a string hreq will set the `content-size` header,
//! and when the body is a `Reader`, hreq will not know the content
//! size, but it can be set by the user.
//!
//! If the content size is not known for HTTP1.1, hreq is forced to
//! use `transfer-encoding: chunked`. For HTTP2, this problem never
//! arises.
//!
//! # JSON
//!
//! By default, hreq uses the [serde] crate to send and receive JSON
//! encoded bodies. Because serde is so ubiquitous in Rust, this
//! feature is enabled by default.
//!
//!
//! ```
//! use hreq::Body;
//! use serde_derive::Serialize;
//!
//! #[derive(Serialize)]
//! struct MyJsonThing {
//!   name: String,
//!   age: u8,
//! }
//!
//! let json = MyJsonThing {
//!   name: "Karl Kajal".to_string(),
//!   age: 32,
//! };
//!
//! let body = Body::from_json(&json);
//! ```
//!
//! # Capabilities
//!
//! * Async or blocking
//! * Pure rust
//! * HTTP/2 and HTTP/1.1
//! * TLS (https)
//! * Timeout for entire request and reading the response
//! * Switchable async runtime (`tokio` or `async-std`)
//! * Single threaded by default
//! * Built as an extension to `http` crate.
//! * Query parameter manipulation in request builder
//! * Many ways to create a request body
//! * Follow redirects
//! * Retry on connection problems
//! * HTTP/1.1 transfer-encoding chunked
//! * Gzip encode/decode
//! * Charset encode/decode
//! * Connection pooling
//! * JSON serialize/deserialize
//! * Cookies
//!
//! [http crate]: https://crates.io/crates/http
//! [`rt-core`]: https://docs.rs/tokio/latest/tokio/runtime/index.html#basic-scheduler
//! [`TcpStream`]: https://doc.rust-lang.org/std/net/struct.TcpStream.html
//! [`Handle`]: https://docs.rs/tokio/latest/tokio/runtime/struct.Handle.html
//! [`Runtime`]: https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html
//! [`AsyncRuntime`]: https://docs.rs/hreq/latest/hreq/enum.AsyncRuntime.html
//! [`Agent`]: https://docs.rs/hreq/latest/hreq/struct.Agent.html
//! [Expect-100]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/100
//! [`content_encode`]: https://docs.rs/hreq/latest/hreq/trait.RequestBuilderExt.html#tymethod.content_encode
//! [`content_decode`]: https://docs.rs/hreq/latest/hreq/trait.RequestBuilderExt.html#tymethod.content_decode
//! [`charset_encode_source`]: https://docs.rs/hreq/latest/hreq/trait.RequestBuilderExt.html#tymethod.charset_encode_source
//! [`charset_decode_target`]: https://docs.rs/hreq/latest/hreq/trait.RequestBuilderExt.html#tymethod.charset_decode_target
//! [serde]: https://crates.io/crates/serde
#[macro_use]
extern crate log;

mod agent;
mod async_impl;
mod block_ext;
mod body;
mod charset;
mod conn;
mod cookies;
mod deadline;
mod either;
mod error;
mod h1;
mod head_ext;
mod proto;
mod psl;
mod req_ext;
mod reqb_ext;
mod res_ext;
mod uri_ext;

#[cfg(feature = "tls")]
mod tls;

#[cfg(all(test, feature = "async-std"))]
mod test;

#[cfg(feature = "tokio")]
mod tokio;

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) use futures_io::{AsyncBufRead, AsyncRead, AsyncWrite};

pub use crate::agent::Agent;
pub use crate::async_impl::AsyncRuntime;
pub use crate::block_ext::BlockExt;
pub use crate::body::Body;
pub use crate::error::Error;
pub use crate::req_ext::RequestExt;
pub use crate::reqb_ext::RequestBuilderExt;
pub use crate::res_ext::ResponseExt;
pub use cookie::Cookie;
pub use http;

#[cfg(feature = "fuzz")]
pub use crate::charset::CharCodec;

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
use crate::uri_ext::HostPort;

pub(crate) trait Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl Stream for Box<dyn Stream> {}

pub(crate) async fn connect(
    host_port: &HostPort<'_>,
    force_http2: bool,
) -> Result<Connection, Error> {
    // "host:port"
    let addr = host_port.to_string();

    let (stream, alpn_proto) = {
        // "raw" tcp
        let tcp = AsyncRuntime::connect_tcp(&addr).await?;

        #[cfg(feature = "tls")]
        {
            use crate::either::Either;
            use crate::tls::wrap_tls;
            if host_port.is_tls() {
                // wrap in tls
                let (tls, proto) = wrap_tls(tcp, host_port.host()).await?;
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

    open_stream(host_port.to_owned(), stream, proto).await
}

pub(crate) async fn open_stream(
    host_port: HostPort<'static>,
    stream: impl Stream,
    proto: Protocol,
) -> Result<Connection, Error> {
    if proto == Protocol::Http2 {
        let (h2, h2conn) = hreq_h2::client::handshake(stream).await?;
        // drives the connection independently of the h2 api surface.
        AsyncRuntime::spawn(async {
            if let Err(err) = h2conn.await {
                // this is expected to happen when the connection disconnects
                trace!("Error in connection: {:?}", err);
            }
        });
        Ok(Connection::new(host_port, ProtocolImpl::Http2(h2)))
    } else {
        let (h1, h1conn) = h1::handshake(stream);
        // drives the connection independently of the h1 api surface
        AsyncRuntime::spawn(async {
            if let Err(err) = h1conn.await {
                // this is expected to happen when the connection disconnects
                trace!("Error in connection: {:?}", err);
            }
        });
        Ok(Connection::new(host_port, ProtocolImpl::Http1(h1)))
    }
}
