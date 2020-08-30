use crate::Stream;
use crate::{AsyncRead, AsyncWrite};
use futures_util::io::AsyncReadExt;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Helper to peek a Stream.
pub(crate) struct Peekable<S> {
    stream: S,
    buf: Vec<u8>,
}

impl<S> Peekable<S> {
    pub fn new(stream: S, capacity: usize) -> Self {
        Peekable {
            stream,
            buf: Vec::with_capacity(capacity),
        }
    }
}

impl<S: AsyncRead + Unpin> Peekable<S> {
    pub async fn peek(&mut self, len: usize) -> Result<&[u8], io::Error> {
        let cur = self.buf.len();
        if cur >= len {
            return Ok(&self.buf[0..len]);
        }
        self.buf.resize(len, 0);
        self.stream.read_exact(&mut self.buf[cur..]).await?;
        Ok(&self.buf[0..len])
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Peekable<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if !this.buf.is_empty() {
            let max = this.buf.len().min(buf.len());
            (&mut buf[0..max]).copy_from_slice(&this.buf[0..max]);
            // TODO: this is not efficient, if we were making a general purpose
            // peekable we would have some index into self.buf to indicate how
            // much of the buffer has been read. however, we are only expecting
            // to read the buf once (for peeking the http2 preface).
            let split = this.buf.split_off(max);
            this.buf = split;
            return Ok(max).into();
        }
        Pin::new(&mut this.stream).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Peekable<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.stream).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.stream).poll_flush(cx)
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.stream).poll_close(cx)
    }
}

impl<S: Stream> Stream for Peekable<S> {}
