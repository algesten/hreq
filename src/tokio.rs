use crate::{AsyncRead, AsyncSeek, AsyncWrite};
use futures_util::ready;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio_lib::io::{
    AsyncRead as TokioAsyncRead, AsyncSeek as TokioAsyncSeek, AsyncWrite as TokioAsyncWrite,
};

#[cfg(feature = "tokio")]
pub(crate) fn from_tokio<Z>(adapted: Z) -> FromAdapter<Z>
where
    Z: TokioAsyncRead + TokioAsyncWrite + Unpin + Send + 'static,
{
    FromAdapter {
        adapted,
        waiting_for_seek: false,
    }
}

pub(crate) struct FromAdapter<Z> {
    adapted: Z,
    waiting_for_seek: bool,
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
    // TokioAsyncWrite doesn't have a poll_write_vectored. This will affect
    // write performance when using a tokio runtime. :(
    // fn poll_write_vectored(
    //     self: Pin<&mut Self>,
    //     cx: &mut Context,
    //     bufs: &[io::IoSlice],
    // ) -> Poll<Result<usize, io::Error>> {
    //     Pin::new(&mut self.get_mut().adapted).poll_write_vectored(cx, bufs)
    // }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().adapted).poll_flush(cx)
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().adapted).poll_shutdown(cx)
    }
}

impl<Z> AsyncSeek for FromAdapter<Z>
where
    Z: TokioAsyncSeek + Unpin,
{
    fn poll_seek(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        pos: io::SeekFrom,
    ) -> Poll<io::Result<u64>> {
        let this = self.get_mut();

        if !this.waiting_for_seek {
            ready!(Pin::new(&mut this.adapted).start_seek(cx, pos))?;
            this.waiting_for_seek = true;
        }

        let rx = ready!(Pin::new(&mut this.adapted).poll_complete(cx));
        this.waiting_for_seek = false;
        rx.into()
    }
}
