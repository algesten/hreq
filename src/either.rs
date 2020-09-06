use crate::{AsyncRead, AsyncSeek, AsyncWrite};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(feature = "server")]
use futures_core::stream::Stream as FutStream;

#[allow(unused)]
pub(crate) enum Either<A, B> {
    A(A),
    B(B),
}

impl<A: AsyncRead + Unpin, B: AsyncRead + Unpin> AsyncRead for Either<A, B> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_read(cx, buf),
            Either::B(b) => Pin::new(b).poll_read(cx, buf),
        }
    }
}

impl<A: AsyncSeek + Unpin, B: AsyncSeek + Unpin> AsyncSeek for Either<A, B> {
    fn poll_seek(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        pos: io::SeekFrom,
    ) -> Poll<io::Result<u64>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_seek(cx, pos),
            Either::B(b) => Pin::new(b).poll_seek(cx, pos),
        }
    }
}

impl<A: AsyncWrite + Unpin, B: AsyncWrite + Unpin> AsyncWrite for Either<A, B> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_write(cx, buf),
            Either::B(b) => Pin::new(b).poll_write(cx, buf),
        }
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context,
        bufs: &[io::IoSlice],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_write_vectored(cx, bufs),
            Either::B(b) => Pin::new(b).poll_write_vectored(cx, bufs),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_flush(cx),
            Either::B(b) => Pin::new(b).poll_flush(cx),
        }
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_close(cx),
            Either::B(b) => Pin::new(b).poll_close(cx),
        }
    }
}

#[cfg(feature = "server")]
impl<A, B, T> FutStream for Either<A, B>
where
    A: FutStream<Item = T> + Unpin,
    B: FutStream<Item = T> + Unpin,
{
    type Item = T;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_next(cx),
            Either::B(b) => Pin::new(b).poll_next(cx),
        }
    }
}
