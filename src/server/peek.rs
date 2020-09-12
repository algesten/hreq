use crate::{AsyncRead, AsyncSeek, AsyncWrite};
use futures_util::io::AsyncReadExt;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Helper to peek a Stream.
pub(crate) struct Peekable<S> {
    stream: S,
    buf: Vec<u8>,
    idx: usize,
    finished: bool,
}

impl<S> Peekable<S> {
    pub fn new(stream: S, capacity: usize) -> Self {
        Peekable {
            stream,
            buf: Vec::with_capacity(capacity),
            idx: 0,
            finished: false,
        }
    }
}

impl<S: AsyncRead + Unpin> Peekable<S> {
    pub async fn peek(&mut self, len: usize) -> Result<&[u8], io::Error> {
        // peeking will reset the read index if we have one.
        if self.idx > 0 {
            let split = self.buf.split_off(self.idx);
            self.buf = split;
            self.idx = 0;
        }

        // ensure we can hold more elements
        self.buf.reserve(len - self.buf.len());

        loop {
            let cur_len = self.buf.len();

            if cur_len >= len || self.finished {
                let to_return = cur_len.min(len);

                return Ok(&self.buf[0..to_return]);
            }

            // we will set the size down again once read.
            unsafe { self.buf.set_len(len) };

            let x = self.stream.read(&mut self.buf[cur_len..]).await;

            let amt = if let Ok(amt) = &x { *amt } else { 0 };

            // must always run to not have unitialized bytes in buf.
            let new_len = cur_len + amt;
            self.buf.truncate(new_len);

            // error exit here.
            let read = x?;

            if read == 0 {
                self.finished = true;
            }
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Peekable<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        let left = this.buf.len() - this.idx;

        if left > 0 {
            // max amount we can read from the peeked bytes
            let max = left.min(buf.len());

            (&mut buf[0..max]).copy_from_slice(&this.buf[this.idx..(this.idx + max)]);

            this.idx += max;

            if this.idx == this.buf.len() {
                // fully read
                this.buf.clear();
                this.idx = 0;
            }

            return Ok(max).into();
        }
        Pin::new(&mut this.stream).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Peekable<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.stream).poll_write(cx, buf)
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context,
        bufs: &[io::IoSlice],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.stream).poll_write_vectored(cx, bufs)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.stream).poll_flush(cx)
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        Pin::new(&mut this.stream).poll_close(cx)
    }
}

impl<S: AsyncSeek + Unpin> AsyncSeek for Peekable<S> {
    fn poll_seek(
        self: Pin<&mut Self>,
        cx: &mut Context,
        pos: io::SeekFrom,
    ) -> Poll<io::Result<u64>> {
        let this = self.get_mut();

        // repositioning dumps buffered content
        this.buf.clear();
        this.idx = 0;

        Pin::new(&mut this.stream).poll_seek(cx, pos)
    }
}
