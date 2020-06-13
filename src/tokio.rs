use crate::Stream;
use crate::{AsyncRead, AsyncWrite};
// use std::fmt;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio_lib::io::{AsyncRead as TokioAsyncRead, AsyncWrite as TokioAsyncWrite};

#[cfg(feature = "tokio")]
pub(crate) fn from_tokio<Z>(adapted: Z) -> FromAdapter<Z>
where
    Z: TokioAsyncRead + TokioAsyncWrite + Unpin + Send + 'static,
{
    FromAdapter { adapted }
}

pub(crate) struct FromAdapter<Z> {
    adapted: Z,
}

impl<Z: TokioAsyncRead + Unpin> AsyncRead for FromAdapter<Z> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().adapted).poll_read(cx, buf)
    }
}

impl<Z: TokioAsyncWrite + Unpin> AsyncWrite for FromAdapter<Z> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.get_mut().adapted).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().adapted).poll_flush(cx)
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().adapted).poll_shutdown(cx)
    }
}

impl<Z> Stream for FromAdapter<Z> where Z: TokioAsyncRead + TokioAsyncWrite + Unpin + Send + 'static {}
