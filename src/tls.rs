//! TLS stream conversion.

use crate::proto::Protocol;
use crate::proto::{ALPN_H1, ALPN_H2};
use crate::Error;
use crate::Stream;
use async_rustls::webpki::DNSNameRef;
use async_rustls::{TlsAcceptor, TlsConnector};
use rustls::{ClientConfig, Session};
use std::sync::Arc;
use webpki_roots::TLS_SERVER_ROOTS;

/// Creates a TLS stream from any underlying stream.
///
/// The TLS certificate will be validated against the (DNS) domain name provided.
/// Negotiates ALPN and we prefer http2 over http11. The [`protocol`] resulting from
/// the negotiation is returned with the wrapped stream.
///
/// [`protocol`]: ../proto/enum.Protocol.html
pub(crate) async fn wrap_tls_client(
    stream: impl Stream,
    domain: &str,
    tls_disable_verify: bool,
) -> Result<(impl Stream, Protocol), Error> {
    //
    let mut config = ClientConfig::new();

    config
        .root_store
        .add_server_trust_anchors(&TLS_SERVER_ROOTS);

    if tls_disable_verify {
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(DisabledCertVerified));
    }

    config.alpn_protocols = vec![ALPN_H2.to_owned(), ALPN_H1.to_owned()];

    let config = Arc::new(config);
    let dnsname = DNSNameRef::try_from_ascii_str(domain)?;

    let connector: TlsConnector = config.into();

    let tls = connector.connect(dnsname, stream).await?;

    let (_, session) = tls.get_ref();

    let proto = Protocol::from_alpn(session.get_alpn_protocol());

    Ok((tls, proto))
}

struct DisabledCertVerified;

impl rustls::ServerCertVerifier for DisabledCertVerified {
    fn verify_server_cert(
        &self,
        _: &rustls::RootCertStore,
        _: &[rustls::Certificate],
        name: DNSNameRef,
        _: &[u8],
    ) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
        warn!("Ignoring TLS verification for {:?}", name);
        Ok(rustls::ServerCertVerified::assertion())
    }
}

#[cfg(feature = "server")]
use rustls::ServerConfig;

#[cfg(feature = "server")]
pub(crate) fn configure_tls_server(config: &mut ServerConfig) {
    config.alpn_protocols = vec![ALPN_H2.to_owned(), ALPN_H1.to_owned()];
}

#[cfg(feature = "server")]
pub(crate) async fn wrap_tls_server(
    stream: impl Stream,
    config: Arc<ServerConfig>,
) -> Result<(impl Stream, Protocol), Error> {
    let acceptor: TlsAcceptor = config.into();

    let tls = acceptor.accept(stream).await?;

    let (_, session) = tls.get_ref();

    let proto = Protocol::from_alpn(session.get_alpn_protocol());

    Ok((tls, proto))
}
