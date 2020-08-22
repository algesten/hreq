use crate::AsyncRuntime;
use crate::Error;
use crate::Stream;
use hreq_h1 as h1;
use tracing_futures::Instrument;

mod agent;
mod conn;
mod cookies;
mod req_ext;
mod reqb_ext;

pub use agent::Agent;
pub use req_ext::RequestExt;
pub use reqb_ext::RequestBuilderExt;

#[cfg(feature = "server")]
pub(crate) use conn::configure_request;

use crate::proto::Protocol;
use crate::uri_ext::HostPort;
use conn::Connection;
use conn::ProtocolImpl;

pub(crate) async fn connect(
    host_port: &HostPort<'_>,
    force_http2: bool,
    tls_disable_verify: bool,
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
    host_port: HostPort<'static>,
    stream: impl Stream,
    proto: Protocol,
) -> Result<Connection, Error> {
    if proto == Protocol::Http2 {
        let (h2, h2conn) = hreq_h2::client::handshake(stream).await?;
        // drives the connection independently of the h2 api surface.
        let conn_task = async {
            if let Err(err) = h2conn.await {
                // this is expected to happen when the connection disconnects
                trace!("Error in connection: {:?}", err);
            }
        }
        .instrument(trace_span!("conn_task"));
        AsyncRuntime::spawn(conn_task);
        Ok(Connection::new(host_port, ProtocolImpl::Http2(h2)))
    } else {
        let (h1, h1conn) = h1::client::handshake(stream);
        // drives the connection independently of the h1 api surface
        let conn_task = async {
            if let Err(err) = h1conn.await {
                // this is expected to happen when the connection disconnects
                trace!("Error in connection: {:?}", err);
            }
        }
        .instrument(trace_span!("conn_task"));
        AsyncRuntime::spawn(conn_task);
        Ok(Connection::new(host_port, ProtocolImpl::Http1(h1)))
    }
}
