use crate::proto::Protocol;
use crate::proto::{ALPN_H1, ALPN_H2};
use crate::tokio::{from_tokio, to_tokio};
use crate::Error;
use crate::Stream;

use tls_api::TlsConnector;
use tls_api::TlsConnectorBuilder;

// TODO investigate why tls-api require us to have a Sync. It doesn't seem reasonable
unsafe impl<S: Stream> Sync for crate::tokio::TokioStream<S> {}
unsafe impl Sync for crate::body::BodyImpl {}

pub async fn wrap_tls<Tls: TlsConnector, S: Stream>(
    stream: S,
    domain: &str,
) -> Result<(impl Stream, Protocol), Error> {
    let mut builder = Tls::builder().expect("TlsConnectorBuilder");

    let protos = [ALPN_H2, ALPN_H1];
    builder.set_alpn_protocols(&protos)?;

    let connector = builder.build().expect("TlsConnector");

    let tls_stream = connector.connect(domain, to_tokio(stream)).await?;

    let alpn = tls_stream.get_alpn_protocol();
    let proto = Protocol::from_alpn(&alpn);

    Ok((from_tokio(tls_stream), proto))
}
