//! Helper to handle buffer with uninitialized memory.

use crate::AsyncRead;
use futures_util::future::poll_fn;
use futures_util::ready;
use std::io;
use std::io::Read;
use std::pin::Pin;
use std::task::Poll;

/// Helper to manage a buffer that read to unitialized bytes.
///
/// Reading into the buffer is done by providing delegates in  read_from_sync,
/// read_from_async or poll_delegate. Each temporarily sets the len of the
/// wrapped Vec<u8> to full capacity and then resets back.
///
/// If the delegated read panics, the internal buffer will be left in a "not safe" state
/// where buf.len() might contain unitialized bytes. This does not matter cause
/// the only way to get data out is via the Deref trait, and that will only
/// ever allow a safe length of bytes out.
#[derive(Debug, Clone)]
pub struct UninitBuf {
    buf: Vec<u8>,
    len: usize,
    expand: bool,
}

impl UninitBuf {
    pub fn new() -> Self {
        Self::with_capacity(16_384)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        UninitBuf {
            buf: Vec::with_capacity(capacity),
            len: 0,
            expand: false,
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl UninitBuf {
    /// Set the full Vec capacity size on the wrapped buffer. We call this the
    /// "unsafe" state, where there are unitialized bytes exposed in the Vec.
    fn set_unsafe_size(&mut self) {
        // this is ok, because there is no way to read data out of UninitBuf that has
        // unitialized bytes..
        unsafe { self.buf.set_len(self.buf.capacity()) }
    }

    /// Reset wrapped buffer back to a known safe length.
    fn set_safe_size(&mut self) {
        unsafe { self.buf.set_len(self.len) }
    }

    /// If a read exhausts the wrapped buffer by reading to full capacity, there's an
    /// opportunity to improve efficiency by expanding the buffer next time we use it.
    /// The typical pattern is read(), clear(), read(), clear(). This fn "remembers"
    /// whether the previous read exhausted the buffer.
    fn mark_expand(&mut self) {
        self.expand = self.len == self.buf.capacity();
    }

    pub fn read_from_sync(&mut self, r: &mut impl Read) -> io::Result<usize> {
        self.reserve_if_needed();
        self.set_unsafe_size();

        let buf = &mut self.buf[self.len..];
        let amt = r.read(buf)?;

        self.len += amt;
        self.set_safe_size();

        self.mark_expand();

        Ok(amt)
    }

    pub async fn read_from_async<R>(&mut self, r: &mut R) -> io::Result<usize>
    where
        R: AsyncRead + Unpin,
    {
        self.reserve_if_needed();
        self.set_unsafe_size();

        let buf = &mut self.buf[self.len..];
        let amt = poll_fn(|cx| Pin::new(&mut *r).poll_read(cx, buf)).await?;

        self.len += amt;
        self.set_safe_size();

        self.mark_expand();

        Ok(amt)
    }

    pub fn poll_delegate(
        &mut self,
        r: impl FnOnce(&mut [u8]) -> Poll<io::Result<usize>>,
    ) -> Poll<io::Result<usize>> {
        self.reserve_if_needed();
        self.set_unsafe_size();

        let buf = &mut self.buf[self.len..];
        let amt = ready!(r(buf)?);

        self.len += amt;
        self.set_safe_size();

        self.mark_expand();

        Ok(amt).into()
    }

    fn reserve_if_needed(&mut self) {
        // we must reserve if there is no headroom to read into.
        let reserve_needed = self.len == self.buf.capacity();

        if self.expand || reserve_needed {
            // Vec has this wonderful built in features that grows exponentially
            // every time we need to re-allocate.
            self.buf.reserve(32);
            self.expand = false;
        }
    }
}

impl Drop for UninitBuf {
    fn drop(&mut self) {
        self.set_safe_size();
    }
}

impl std::ops::Deref for UninitBuf {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &(self.buf)[..self.len]
    }
}
