#![warn(clippy::all)]

#[macro_use]
extern crate log;

mod agent;
mod async_impl;
mod body;
mod charset;
mod conn;
mod conn_http1;
mod conn_http2;
mod deadline;
mod either;
mod error;
pub mod h1;
mod proto;
mod req_ext;
mod res_ext;
mod tls;
mod tls_pass;
mod tokio;
mod uri_ext;

#[cfg(all(test, feature = "async-std"))]
mod test;

pub(crate) use futures_io::{AsyncBufRead, AsyncRead, AsyncWrite};

pub use crate::agent::Agent;
pub use crate::async_impl::AsyncRuntime;
pub use crate::body::Body;
pub use crate::conn::Connection;
pub use crate::error::Error;
pub use crate::req_ext::{RequestBuilderExt, RequestExt};
pub use crate::res_ext::ResponseExt;
pub use http;

pub mod prelude {
    pub use crate::{BlockExt, RequestBuilderExt, RequestExt, ResponseExt};
    pub use http::{Request, Response};
}

use crate::conn::ProtocolImpl;
use crate::either::Either;
use crate::proto::Protocol;
use crate::tls::wrap_tls;
use crate::tokio::to_tokio;
use crate::uri_ext::UriExt;
use std::future::Future;
use tls_api::TlsConnector;

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl Stream for Box<dyn Stream> {}

pub async fn connect<Tls: TlsConnector>(uri: &http::Uri) -> Result<Connection, Error> {
    let hostport = uri.host_port()?;
    // "host:port"
    let addr = hostport.to_string();

    let (stream, alpn_proto) = {
        // "raw" tcp
        let tcp = AsyncRuntime::current().connect_tcp(&addr).await?;

        if hostport.is_tls() {
            // wrap in tls
            let (tls, proto) = wrap_tls::<Tls, _>(tcp, hostport.host()).await?;
            (Either::A(tls), proto)
        } else {
            // use tcp
            (Either::B(tcp), Protocol::Unknown)
        }
    };

    open_stream(addr, stream, alpn_proto).await
}

pub async fn open_stream(
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

pub trait BlockExt {
    fn block(self) -> Self::Output
    where
        Self: Future;
}

impl<F: Future> BlockExt for F {
    fn block(self) -> F::Output {
        AsyncRuntime::current().block_on(self)
    }
}
