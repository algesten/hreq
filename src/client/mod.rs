use crate::AsyncRuntime;
use crate::Error;
use crate::Stream;
use hreq_h1 as h1;
use tokio_util::compat::FuturesAsyncReadCompatExt;

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

use crate::bw::BandwidthMonitor;
use crate::proto::Protocol;
use crate::uri_ext::HostPort;
use conn::Connection;
use futures_util::future::poll_fn;
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

pub(crate) async fn connect(
    host_port: &HostPort,
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
            use crate::either::Either;
            use crate::tls::wrap_tls_client;

            if host_port.is_tls() {
                // wrap in tls
                let (tls, proto) =
                    wrap_tls_client(tcp, host_port.host(), tls_disable_verify).await?;
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
    host_port: HostPort,
    stream: impl Stream,
    proto: Protocol,
) -> Result<Connection, Error> {
    if proto == Protocol::Http2 {
        const DEFAULT_CONN_WINDOW: u32 = 5 * 1024 * 1024;
        const DEFAULT_STREAM_WINDOW: u32 = 2 * 1024 * 1024;
        const DEFAULT_MAX_FRAME_SIZE: u32 = 16 * 1024;

        let mut builder = h2::client::Builder::default();
        builder
            .initial_window_size(DEFAULT_STREAM_WINDOW)
            .initial_connection_window_size(DEFAULT_CONN_WINDOW)
            .max_frame_size(DEFAULT_MAX_FRAME_SIZE);

        let (h2, mut h2conn) = builder.handshake(stream.compat()).await?;

        let pinger = h2conn.ping_pong().expect("Take ping_pong of h2conn");
        let bw = BandwidthMonitor::new(pinger);

        let mut bw_conn = bw.clone();

        // piggy-back the bandwidth monitor on polling the connection
        let conn_and_bw = poll_fn(move |cx| {
            if let Poll::Ready(window_size) = bw_conn.poll_window_update(cx) {
                trace!("Update h2 window size: {}", window_size);
                h2conn.set_target_window_size(window_size);
                h2conn.set_initial_window_size(window_size)?;
            };
            Pin::new(&mut h2conn).poll(cx)
        });

        // drives the connection independently of the h2 api surface.
        let conn_task = async {
            if let Err(err) = conn_and_bw.await {
                // this is expected to happen when the connection disconnects
                trace!("Error in connection: {:?}", err);
            }
        };

        AsyncRuntime::spawn(conn_task);

        Ok(Connection::new_h2(host_port, h2, bw))
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
