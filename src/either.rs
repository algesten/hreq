use crate::{AsyncRead, AsyncSeek, AsyncWrite};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(feature = "server")]
use futures_core::stream::Stream as FutStream;

#[allow(unused)]
pub(crate) enum Either<A, B, C> {
    A(A),
    B(B),
    C(C),
}

impl<A, B, C> AsyncRead for Either<A, B, C>
where
    A: AsyncRead + Unpin,
    B: AsyncRead + Unpin,
    C: AsyncRead + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_read(cx, buf),
            Either::B(b) => Pin::new(b).poll_read(cx, buf),
            Either::C(c) => Pin::new(c).poll_read(cx, buf),
        }
    }
}

impl<A, B, C> AsyncSeek for Either<A, B, C>
where
    A: AsyncSeek + Unpin,
    B: AsyncSeek + Unpin,
    C: AsyncSeek + Unpin,
{
    fn poll_seek(
        self: Pin<&mut Self>,
        cx: &mut Context,
        pos: io::SeekFrom,
    ) -> Poll<io::Result<u64>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_seek(cx, pos),
            Either::B(b) => Pin::new(b).poll_seek(cx, pos),
            Either::C(c) => Pin::new(c).poll_seek(cx, pos),
        }
    }
}

impl<A, B, C> AsyncWrite for Either<A, B, C>
where
    A: AsyncWrite + Unpin,
    B: AsyncWrite + Unpin,
    C: AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_write(cx, buf),
            Either::B(b) => Pin::new(b).poll_write(cx, buf),
            Either::C(c) => Pin::new(c).poll_write(cx, buf),
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
            Either::C(c) => Pin::new(c).poll_write_vectored(cx, bufs),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_flush(cx),
            Either::B(b) => Pin::new(b).poll_flush(cx),
            Either::C(c) => Pin::new(c).poll_flush(cx),
        }
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_close(cx),
            Either::B(b) => Pin::new(b).poll_close(cx),
            Either::C(c) => Pin::new(c).poll_close(cx),
        }
    }
}

#[cfg(feature = "server")]
impl<A, B, C, T> FutStream for Either<A, B, C>
where
    A: FutStream<Item = T> + Unpin,
    B: FutStream<Item = T> + Unpin,
    C: FutStream<Item = T> + Unpin,
{
    type Item = T;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match self.get_mut() {
            Either::A(a) => Pin::new(a).poll_next(cx),
            Either::B(b) => Pin::new(b).poll_next(cx),
            Either::C(c) => Pin::new(c).poll_next(cx),
        }
    }
}
