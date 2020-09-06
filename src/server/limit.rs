use crate::AsyncRead;
use futures_util::ready;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Reader limited by a set length.
#[derive(Debug)]
pub struct ContentLengthRead<R> {
    inner: R,
    left: u64,
}

impl<R> ContentLengthRead<R> {
    pub fn new(inner: R, left: u64) -> Self {
        ContentLengthRead { inner, left }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for ContentLengthRead<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        assert!(!buf.is_empty());

        let this = self.get_mut();

        let max = this.left.min(buf.len() as u64) as usize;

        if max == 0 {
            return Ok(0).into();
        }

        let amount = ready!(Pin::new(&mut this.inner).poll_read(cx, &mut buf[0..max]))?;

        this.left -= amount as u64;

        Ok(amount).into()
    }
}
