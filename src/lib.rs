#![warn(clippy::all)]
#![warn(missing_docs, missing_debug_implementations)]

//! hreq is a user first async http client and server.
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
//! * async (or blocking via minimal runtime).
//! * Pure Rust.
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
//! Many rust http client/servers use some variant of the [http crate].
//! It's often copied into the local source tree and extended from there.
//!
//! When writing a service that uses both a web server and
//! client crate, one often ends up with similar, but not exactly the
//! same versions of types like `http::Request` and `http::Response`.
//!
//! hreq works using extension traits only. It re-exports the http
//! crate, but does not copy or modify it. It therefore adheres
//! strictly to the exact API definition as set out by the
//! http crate as well as avoids furthering the confusion of having
//! multiple types with the same name.
//!
//! # Blocking and async
//!
//! Rust's async story is fantastic, but not every situation requires
//! async.  hreq "fakes" being a blocking library by default having a
//! very minimal tokio runtime ([`rt-core`]) combined with a `.block()`
//! call that is placed where we expect an `.await` in an async
//! situation.
//!
//! ```
//! use hreq::prelude::*;
//!
//! let res = Request::get("https://httpbin.org/get")
//!     .call().block();
//! ```
//!
//! ## Why?
//!
//! hreq is async through-and-through and ultimately relies on an
//! async variant of [`TcpStream`] for it to function. Because the
//! TCP socket is one of those things that is tightly coupled to
//! the async event loop, `TcpStream` in turn needs to be provided
//! by the runtime (tokio or async-std)
//!
//! There are talks of rust providing a simple single threaded
//! executor as part of the std lib. This only solves half of the
//! problem since `TcpStream` is coupled with the runtime.
//!
//! # Async runtime
//!
//! The async runtime is "pluggable" and comes in some different
//! flavors.
//!
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
//! let res1 = Request::get("https://httpbin.org/get")
//!     .call().block();  // creates a new agent
//!
//! // this call doesn't reuse any cookies or connections.
//! let res2 = Request::get("https://httpbin.org/get")
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
//! let req1 = Request::get("https://httpbin.org/get")
//!     .with_body(()).unwrap();
//!
//! let res1 = agent.send(req1).block();
//!
//! let req2 = Request::get("https://httpbin.org/get")
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
//! let req = Request::get("https://httpbin.org/get")
//!     .with_body(()).unwrap();
//!
//! let res = agent.send(req).block();
//! ```
//!
//! ## Redirects
//!
//! By default hreq follows up to 5 redirects. Redirects can be turned off
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
//! # Server
//!
//! hreq started as a client but now also got a simple server mechanism. It
//! can route requests, use middleware, handle state and serve TLS.
//!
//! See the [`server module doc`] for more details.
//!
//! ```no_run
//! // ignore this example if not feature server
//! #[cfg(feature = "server")] {
//!
//! use hreq::prelude::*;
//!
//!
//! async fn start_server() {
//!     let mut server = Server::new();
//!     server.at("/hello/:name").get(hello_there);
//!     let (shut, addr) = server.listen(0).await.expect("Failed to listen");
//!     println!("Listening to: {}", addr);
//!     shut.shutdown().await;
//! }
//!
//! async fn hello_there(req: http::Request<Body>) -> String {
//!     let name = req.path_param("name").unwrap();
//!     format!("Hello there {}!\n", name)
//! }
//!
//! }
//! ```
//!
//! # Capabilities
//!
//! * Async or blocking
//! * Pure rust
//! * HTTP/2 and HTTP/1.1
//! * TLS (https)
//! * Timeout for entire request and reading the response
//! * Switchable async runtime (`tokio`, `async-std`)
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
//! [`server module doc`]: https://docs.rs/hreq/latest/hreq/server/index.html
#[macro_use]
extern crate log;

mod async_impl;
mod block_ext;
mod body;
mod body_codec;
mod body_send;
mod charset;
mod client;
mod deadline;
mod either;
mod error;
mod head_ext;
mod params;
mod proto;
mod psl;
mod res_ext;
mod uri_ext;

pub use client::{Agent, ResponseFuture};

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "tls")]
mod tls;

#[cfg(feature = "tokio")]
mod tokio;

#[doc(hidden)]
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use once_cell::sync::Lazy;

pub(crate) const AGENT_IDENT: Lazy<String> = Lazy::new(|| format!("rust/hreq/{}", crate::VERSION));

pub(crate) use futures_io::{AsyncBufRead, AsyncRead, AsyncSeek, AsyncWrite};

pub use crate::async_impl::AsyncRuntime;
pub use crate::block_ext::BlockExt;
pub use crate::body::Body;
pub use crate::client::RequestBuilderExt;
pub use crate::client::RequestExt;
pub use crate::error::Error;
pub use crate::res_ext::ResponseExt;
pub use http;

pub mod cookie {
    //! Re-export of the [cookie crate].
    //!
    //! [cookie crate]: https://docs.rs/cookie/latest/cookie/
    pub use cookie::Cookie;
}

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
    pub use crate::{BlockExt, Body, RequestBuilderExt, RequestExt, ResponseExt};

    #[doc(no_inline)]
    pub use http::{Request, Response};

    #[cfg(feature = "server")]
    #[doc(no_inline)]
    pub use crate::server::{ResponseBuilderExt, Router, Server, ServerRequestExt};
}

pub(crate) trait Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<Z: AsyncRead + AsyncWrite + Unpin + Send + 'static> Stream for Z {}

pub(crate) trait AsyncReadSeek: AsyncRead + AsyncSeek {}
impl<Z: AsyncRead + AsyncSeek> AsyncReadSeek for Z {}
