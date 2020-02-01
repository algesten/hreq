use std::error::Error as std_Error;
use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tls_api::Result;
use tls_api::TlsStream;

use tokio_traits::{TokioAsyncRead, TokioAsyncWrite};

pub struct TlsConnectorBuilder(PassThrough);
pub struct TlsConnector(PassThrough);

pub struct TlsAcceptorBuilder(PassThrough);
pub struct TlsAcceptor(PassThrough);

#[derive(Debug)]
struct Error;

impl std_Error for Error {
    fn description(&self) -> &str {
        "pass through implementation"
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "pass through implementation")
    }
}

pub struct PassThrough();

impl tls_api::TlsConnectorBuilder for TlsConnectorBuilder {
    type Connector = TlsConnector;

    type Underlying = PassThrough;

    fn underlying_mut(&mut self) -> &mut PassThrough {
        &mut self.0
    }

    fn supports_alpn() -> bool {
        false
    }

    fn set_alpn_protocols(&mut self, _protocols: &[&[u8]]) -> Result<()> {
        Ok(())
    }

    fn set_verify_hostname(&mut self, _verify: bool) -> Result<()> {
        Ok(())
    }

    fn add_root_certificate(&mut self, _cert: tls_api::Certificate) -> Result<&mut Self> {
        Ok(self)
    }

    fn build(self) -> Result<TlsConnector> {
        Ok(TlsConnector(PassThrough()))
    }
}

impl tls_api::TlsConnector for TlsConnector {
    type Builder = TlsConnectorBuilder;

    fn builder() -> Result<TlsConnectorBuilder> {
        Ok(TlsConnectorBuilder(PassThrough()))
    }

    fn connect<'a, S>(
        &'a self,
        _domain: &'a str,
        _stream: S,
    ) -> Pin<Box<dyn Future<Output = tls_api::Result<TlsStream<S>>> + Send + 'a>>
    where
        S: TokioAsyncRead + TokioAsyncWrite + fmt::Debug + Unpin + Send + Sync + 'static,
    {
        Box::pin(async { Ok(TlsStream::new(TlsStreamImpl(Box::new(_stream)))) })
    }
}

// pub trait TlsStreamImpl<S>:
//     TokioAsyncRead + TokioAsyncWrite + Unpin + fmt::Debug + Send + Sync + 'static
// {
//     /// Get negotiated ALPN protocol.
//     fn get_alpn_protocol(&self) -> Option<Vec<u8>>;
//     fn get_mut(&mut self) -> &mut S;
//     fn get_ref(&self) -> &S;
// }

#[derive(Debug)]
struct TlsStreamImpl<S>(Box<S>)
where
    S: TokioAsyncRead + TokioAsyncWrite + fmt::Debug + Unpin + Send + Sync + 'static;

impl<S> tls_api::TlsStreamImpl<S> for TlsStreamImpl<S>
where
    S: TokioAsyncRead + TokioAsyncWrite + fmt::Debug + Unpin + Send + Sync + 'static,
{
    fn get_alpn_protocol(&self) -> Option<Vec<u8>> {
        None
    }
    fn get_mut(&mut self) -> &mut S {
        &mut self.0
    }
    fn get_ref(&self) -> &S {
        &self.0
    }
}

impl tls_api::TlsAcceptorBuilder for TlsAcceptorBuilder {
    type Acceptor = TlsAcceptor;

    type Underlying = PassThrough;

    fn supports_alpn() -> bool {
        false
    }

    fn set_alpn_protocols(&mut self, _protocols: &[&[u8]]) -> Result<()> {
        Err(tls_api::Error::new(Error))
    }

    fn underlying_mut(&mut self) -> &mut PassThrough {
        &mut self.0
    }

    fn build(self) -> Result<TlsAcceptor> {
        Err(tls_api::Error::new(Error))
    }
}

impl tls_api::TlsAcceptor for TlsAcceptor {
    type Builder = TlsAcceptorBuilder;

    fn accept<'a, S>(
        &'a self,
        _stream: S,
    ) -> Pin<Box<dyn Future<Output = Result<TlsStream<S>>> + Send + 'a>>
    where
        S: TokioAsyncRead + TokioAsyncWrite + fmt::Debug + Unpin + Send + Sync + 'static,
    {
        Box::pin(async { Err(tls_api::Error::new(Error)) })
    }
}

impl<S> TokioAsyncRead for TlsStreamImpl<S>
where
    S: TokioAsyncRead + TokioAsyncWrite + fmt::Debug + Unpin + Send + Sync + 'static,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

impl<S> TokioAsyncWrite for TlsStreamImpl<S>
where
    S: TokioAsyncRead + TokioAsyncWrite + fmt::Debug + Unpin + Send + Sync + 'static,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
}
