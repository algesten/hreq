//! TLS stream conversion.

use crate::proto::Protocol;
use crate::proto::{ALPN_H1, ALPN_H2};
use crate::Error;
use crate::Stream;
use crate::{AsyncRead, AsyncWrite};
use futures_util::future::poll_fn;
use futures_util::ready;
use rustls::Session;
use rustls::{ClientConfig, ClientSession};
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use webpki::DNSNameRef;
use webpki_roots::TLS_SERVER_ROOTS;

/// Creates a TLS stream from any underlying stream.
///
/// The TLS certificate will be validated against the (DNS) domain name provided.
/// Negotiates ALPN and we prefer http2 over http11. The [`protocol`] resulting from
/// the negotiation is returned with the wrapped stream.
///
/// [`protocol`]: ../proto/enum.Protocol.html
#[instrument(skip(stream, domain))]
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

    let client = ClientSession::new(&config, dnsname);

    let mut tls = TlsStream::new(stream, client);

    let ret = poll_fn(|cx| Pin::new(&mut tls).poll_handshake(cx)).await;
    trace!("tls handshake: {:?}", ret);
    ret?;

    let proto = Protocol::from_alpn(tls.tls.get_alpn_protocol());

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
#[instrument(skip(stream, config))]
pub(crate) async fn wrap_tls_server(
    stream: impl Stream,
    config: Arc<ServerConfig>,
) -> Result<(impl Stream, Protocol), Error> {
    use rustls::ServerSession;

    let server = ServerSession::new(&config);

    let mut tls = TlsStream::new(stream, server);

    let ret = poll_fn(|cx| Pin::new(&mut tls).poll_handshake(cx)).await;
    trace!("tls handshake: {:?}", ret);
    ret?;

    let proto = Protocol::from_alpn(tls.tls.get_alpn_protocol());

    Ok((tls, proto))
}

/// Wrapper stream which encapsulates the rustls TLS session.
///
/// rustls is sync, so there's some trickery with internal buffers to work around that.
struct TlsStream<S, E> {
    stream: S,
    tls: E,
    read_buf: Vec<u8>, // TODO use a ring buffer or similar here
    write_buf: Vec<u8>,
    wants_flush: bool,
    plaintext: Vec<u8>,
    plaintext_idx: usize,
}

impl<S: Stream, E: Session + Unpin + 'static> TlsStream<S, E> {
    pub fn new(stream: S, tls: E) -> Self {
        TlsStream {
            stream,
            tls,
            read_buf: Vec::new(),
            write_buf: Vec::new(),
            wants_flush: false,
            plaintext: Vec::new(),
            plaintext_idx: 0,
        }
    }

    fn plaintext_left(&self) -> usize {
        self.plaintext.len() - self.plaintext_idx
    }

    /// Poll for TLS completeness. rustls calls the equivalent (blocking) function
    /// [`complete_io`].
    ///
    /// This is the main translation between the async and sync. The idea is that rustls passes
    /// `io::ErrorKind::WouldBlock` straight through. Hence we can create a sync wrapper
    /// around two internal buffers (`read_buf`, `write_buf`) and instead of ever letting them
    /// read to end we create a fake io::Error that passes through rustls and we can capture
    /// on the "other side".
    ///
    /// [`complete_io`]: https://docs.rs/rustls/latest/rustls/trait.Session.html#method.complete_io
    #[allow(clippy::useless_let_if_seq)]
    fn poll_tls(&mut self, cx: &mut Context, poll_for_read: bool) -> Poll<io::Result<()>> {
        loop {
            // anything to flush out shortcuts here. this will register
            // a wakeup if we're blocking on write.
            ready!(self.try_write_buf(cx))?;

            // if the write buffer is exhausted, we might have a follow-up flush.
            if self.wants_flush {
                ready!(Pin::new(&mut self.stream).poll_flush(cx))?;
                self.wants_flush = false;
            }

            // if we have no read_buf bytes, we need to poll read the underlying stream
            // in two scenarios.
            //   * we are handshaking
            //   * the user has asked to read (plaintext), but there is none such decrypted.
            if self.read_buf.is_empty()
                && (poll_for_read && self.plaintext_left() == 0 || self.tls.is_handshaking())
            {
                // we want to read something new
                let _ = self.try_read_buf(cx);
            }

            let mut did_tls_read_or_write = false;

            if self.tls.wants_read() && !self.read_buf.is_empty() {
                let mut sync = SyncStream::new(
                    &mut self.read_buf,
                    &mut self.write_buf,
                    &mut self.wants_flush,
                );
                // If the client reads to end, we "block", but actually use the waker straight away.
                let _ = ready!(blocking_to_poll(self.tls.read_tls(&mut sync), cx))?;
                // potential TLS errors will arise here.
                self.tls
                    .process_new_packets()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

                if !self.tls.is_handshaking() {
                    let rest = self.plaintext.split_off(self.plaintext_idx);
                    self.plaintext = rest;
                    self.plaintext_idx = 0;
                    let _ = self.tls.read_to_end(&mut self.plaintext)?;
                }

                did_tls_read_or_write = true;
            }

            if self.tls.wants_write() {
                let mut sync = SyncStream::new(
                    &mut self.read_buf,
                    &mut self.write_buf,
                    &mut self.wants_flush,
                );
                let _ = ready!(blocking_to_poll(self.tls.write_tls(&mut sync), cx))?;

                did_tls_read_or_write = true;
            }

            // any writing or reading on the tls level, means we start over to check
            // the ready state of the incoming/outgoing.
            if did_tls_read_or_write {
                continue;
            }

            if poll_for_read && self.plaintext_left() == 0 {
                // if we are waiting for a read, there's not plaintext and the tls level
                // didn't do any reads or writes, we must wait for some notification of
                // the future.
                return Poll::Pending;
            } else {
                // we are here in two cases:
                //   * we are poll_for_read and there is plaintext to use up
                //   * we are writing and there is no more activity on the tls level.
                return Poll::Ready(Ok(()));
            }
        }
    }

    /// Complete writing of write_buf to the underlying stream.
    fn try_write_buf(&mut self, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        // complete write of data held in buffer first
        if !self.write_buf.is_empty() {
            let to_write = &self.write_buf[..];
            let amount = ready!(Pin::new(&mut self.stream).poll_write(cx, to_write))?;
            let rest = self.write_buf.split_off(amount);
            self.write_buf = rest;
        }
        Ok(()).into()
    }

    /// Attempt to read some more bytes into read_buf from the underlying stream.
    fn try_read_buf(&mut self, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let mut tmp = [0; 8_192];
        let amount = ready!(Pin::new(&mut self.stream).poll_read(cx, &mut tmp[..]))?;
        self.read_buf.extend_from_slice(&tmp[0..amount]);
        Ok(()).into()
    }

    /// Poll for handshake to finish.
    fn poll_handshake(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.poll_tls(cx, false))?;
        if this.tls.is_handshaking() {
            Poll::Pending
        } else {
            Ok(()).into()
        }
    }
}

impl<S: Stream, E: Session + Unpin + 'static> AsyncRead for TlsStream<S, E> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        ready!(this.poll_tls(cx, true))?;

        let idx = this.plaintext_idx;
        let amount = buf.len().min(this.plaintext_left());
        (&mut buf[0..amount]).copy_from_slice(&this.plaintext[idx..(idx + amount)]);
        this.plaintext_idx += amount;

        Ok(amount).into()
    }
}

impl<S: Stream, E: Session + Unpin + 'static> AsyncWrite for TlsStream<S, E> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();

        ready!(this.poll_tls(cx, false))?;

        let amount = this.tls.write(buf)?;

        Ok(amount).into()
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();

        ready!(this.poll_tls(cx, false))?;

        this.tls.flush()?;

        ready!(this.poll_tls(cx, false))?;

        Ok(()).into()
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();

        ready!(this.poll_tls(cx, false))?;

        this.tls.send_close_notify();

        ready!(this.poll_tls(cx, false))?;

        Pin::new(&mut this.stream).poll_close(cx)
    }
}

impl<S: Stream, E: Session + Unpin + 'static> Stream for TlsStream<S, E> {}

/// Helper struct to adapt some buffers into a blocking `io::Read` and `io::Write`.
///
/// If we attempt to `.read()` when `read_buf` is empty, we return an `io::ErrorKind::WouldBlock`.
///
/// TODO: Writes currently never block, which could potentially lead to a large output buffer.
struct SyncStream<'a> {
    read_buf: &'a mut Vec<u8>,
    write_buf: &'a mut Vec<u8>,
    wants_flush: &'a mut bool,
}

impl<'a> SyncStream<'a> {
    fn new(
        read_buf: &'a mut Vec<u8>,
        write_buf: &'a mut Vec<u8>,
        wants_flush: &'a mut bool,
    ) -> Self {
        SyncStream {
            read_buf,
            write_buf,
            wants_flush,
        }
    }
}

impl<'a> io::Read for SyncStream<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let from = &mut self.read_buf;
        if from.is_empty() {
            return would_block();
        }
        let max = buf.len().min(from.len());
        (&mut buf[0..max]).copy_from_slice(&from[0..max]);
        let rest = from.split_off(max);
        *self.read_buf = rest;
        Ok(max)
    }
}

impl<'a> io::Write for SyncStream<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let to = &mut self.write_buf;
        to.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        *self.wants_flush = true;
        Ok(())
    }
}

/// Create an `io::ErrorKind::WouldBlock` error.
fn would_block() -> io::Result<usize> {
    Err(io::Error::new(io::ErrorKind::WouldBlock, "block"))
}

/// Convert `io::ErrorKind::WouldBlock` to a `Poll::Pending`
fn blocking_to_poll<T>(result: io::Result<T>, cx: &mut Context) -> Poll<io::Result<T>> {
    match result {
        Ok(v) => Poll::Ready(Ok(v)),
        Err(e) => {
            if e.kind() == io::ErrorKind::WouldBlock {
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(Err(e))
            }
        }
    }
}
