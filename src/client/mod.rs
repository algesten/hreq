use crate::AsyncRuntime;
use crate::Error;
use crate::Stream;
use hreq_h1 as h1;

mod agent;
mod conn;
mod cookies;
mod req_ext;
mod reqb_ext;

pub use agent::{Agent, ResponseFuture};
pub use req_ext::RequestExt;
pub use reqb_ext::RequestBuilderExt;

#[cfg(feature = "server")]
pub(crate) use conn::configure_request;

use crate::proto::Protocol;
use crate::uri_ext::HostPort;
use conn::Connection;

pub(crate) async fn connect(
    host_port: &HostPort<'_>,
    force_http2: bool,
    #[allow(unused_variables)] tls_disable_verify: bool,
) -> Result<Connection, Error> {
    // "host:port"
    let addr = host_port.to_string();

    let (stream, alpn_proto) = {
        // "raw" tcp
        let tcp = AsyncRuntime::connect_tcp(&addr).await?;

        #[cfg(feature = "tls")]
        {
            use crate::async_impl::FakeStream;
            use crate::either::Either;
            use crate::tls::wrap_tls_client;

            if host_port.is_tls() {
                // wrap in tls
                let (tls, proto) =
                    wrap_tls_client(tcp, host_port.host(), tls_disable_verify).await?;
                (Either::A(tls), proto)
            } else {
                // use tcp
                (Either::<_, _, FakeStream>::B(tcp), Protocol::Unknown)
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
        const DEFAULT_CONN_WINDOW: u32 = 5 * 1024 * 1024;
        const DEFAULT_STREAM_WINDOW: u32 = 2 * 1024 * 1024;
        const DEFAULT_MAX_FRAME_SIZE: u32 = 16 * 1024;

        let mut builder = hreq_h2::client::Builder::default();
        builder
            .initial_window_size(DEFAULT_STREAM_WINDOW)
            .initial_connection_window_size(DEFAULT_CONN_WINDOW)
            .max_frame_size(DEFAULT_MAX_FRAME_SIZE);

        let (h2, h2conn) = builder.handshake(stream).await?;

        // drives the connection independently of the h2 api surface.
        let conn_task = async {
            if let Err(err) = h2conn.await {
                // this is expected to happen when the connection disconnects
                trace!("Error in connection: {:?}", err);
            }
        };
        AsyncRuntime::spawn(conn_task);
        Ok(Connection::new_h2(host_port, h2))
    } else {
        let (h1, h1conn) = h1::client::handshake(stream);
        // drives the connection independently of the h1 api surface
        let conn_task = async {
            if let Err(err) = h1conn.await {
                // this is expected to happen when the connection disconnects
                trace!("Error in connection: {:?}", err);
            }
        };
        AsyncRuntime::spawn(conn_task);
        Ok(Connection::new_h1(host_port, h1))
    }
}
