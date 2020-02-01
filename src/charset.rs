use crate::AsyncBufRead;
use encoding_rs::{Decoder, Encoder, Encoding};
use futures_util::ready;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct CharCodec(Codec);

enum Codec {
    Encoder(Encoder),
    Decoder(Decoder),
}

impl CharCodec {
    pub fn new(charset: &str, decode: bool) -> CharCodec {
        let enc = Encoding::for_label(charset.as_bytes()).unwrap_or_else(|| {
            warn!("Unrecognized character encoding: {}", charset);
            Encoding::for_label(b"utf-8").unwrap()
        });
        let codec = if decode {
            trace!("CharCodec decoder: {} -> {}", charset, enc.name());
            Codec::Decoder(enc.new_decoder())
        } else {
            trace!("CharCodec encoder: {} -> {}", charset, enc.name());
            Codec::Encoder(enc.new_encoder())
        };
        CharCodec(codec)
    }

    pub fn poll_decode<R: AsyncBufRead + Unpin>(
        &mut self,
        cx: &mut Context,
        from: &mut R,
        dst: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let src = ready!(Pin::new(&mut *from).poll_fill_buf(cx))?;

        let (read, written) = match &mut self.0 {
            Codec::Encoder(_) => {
                // TODO...
                panic!("Missing Codec::Encoder impl");
            }
            Codec::Decoder(dec) => {
                let (_, read, written, had_errors) = dec.decode_to_utf8(src, dst, src.is_empty());
                if had_errors {
                    debug!("Character decoder had errors");
                }
                (read, written)
            }
        };

        Pin::new(&mut *from).consume(read);

        Ok(written).into()
    }
}
