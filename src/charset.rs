use crate::AsyncBufRead;
use encoding_rs::{Decoder, Encoder, Encoding};
use futures_util::ready;
use std::fmt;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Charset transcoder
pub struct CharCodec {
    dec: Decoder,
    enc: Option<Encoder>,
    decoded: String,
    is_end: bool,
}

impl CharCodec {
    pub fn new(from: &'static Encoding, to: &'static Encoding) -> CharCodec {
        CharCodec {
            dec: from.new_decoder(),
            enc: if to == encoding_rs::UTF_8 {
                None
            } else {
                Some(to.new_encoder())
            },
            decoded: String::new(),
            is_end: false,
        }
    }

    pub fn remove_encoder(&mut self) {
        self.enc = None;
    }

    pub fn poll_codec<R: AsyncBufRead + Unpin>(
        &mut self,
        cx: &mut Context,
        from: &mut R,
        dst: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        // get some incoming bytes from source
        let src = ready!(Pin::new(&mut *from).poll_fill_buf(cx))?;

        let mut consumed = 0;
        let ret = self.decode_from_buf(src, dst, &mut consumed);

        Pin::new(&mut *from).consume(consumed);

        Poll::Ready(ret)
    }

    pub fn decode_from_buf(
        &mut self,
        src: &[u8],
        dst: &mut [u8],
        consumed: &mut usize,
    ) -> Result<usize, io::Error> {
        // true once when we reach EOF first time
        let mut became_end = false;

        if !self.is_end && src.is_empty() {
            became_end = true;
            self.is_end = true;
        }

        // decode when there's not many chars left in the decoded
        if (!self.is_end && self.decoded.len() < 128) || became_end {
            let mut decode_to = [0_u8; 8_192];

            let (_, decode_read, decode_written, decode_had_errors) =
                self.dec.decode_to_utf8(src, &mut decode_to[..], became_end);

            if decode_had_errors {
                debug!("Character decoder had errors");
            }

            *consumed = decode_read;

            // this unsafe is ok because we trust encoding_rs produces legit utf8.
            let decoded = unsafe { std::str::from_utf8_unchecked(&decode_to[0..decode_written]) };
            self.decoded.push_str(decoded);
        }

        if let Some(enc) = &mut self.enc {
            // transcode to the output encoding
            let (_, encode_read, encode_written, encode_had_errors) =
                enc.encode_from_utf8(&self.decoded[..], dst, self.is_end);
            if encode_had_errors {
                debug!("Character encoder had errors");
            }

            // encode_read is a char offset into the string. we don't need to
            // split this on a byte offset.
            let rest = self.decoded.split_off(encode_read);
            self.decoded = rest;

            Ok(encode_written)
        } else {
            // the output is utf8, and that's what we already have,
            // don't do any additional encoding.
            let decoded_bytes = self.decoded.as_bytes();
            let mut max = decoded_bytes.len().min(dst.len());

            // reduce the amount to copy until we're on a char boundary
            while max > 0 && !self.decoded.is_char_boundary(max) {
                max -= 1;
            }

            (&mut dst[0..max]).copy_from_slice(&decoded_bytes[0..max]);

            // this unsafe is ok, since we moved max to be on a char boundary.
            let vec = unsafe { self.decoded.as_mut_vec() };
            let rest = vec.split_off(max);
            *vec = rest;

            Ok(max)
        }
    }
}

impl fmt::Debug for CharCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CharCodec {{ from: {}, to: {} }}",
            self.dec.encoding().name(),
            self.enc
                .as_ref()
                .map(|e| e.encoding())
                .unwrap_or(encoding_rs::UTF_8)
                .name()
        )
    }
}
