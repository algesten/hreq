use super::Error;
use super::RecvReader;
use futures_util::ready;
use std::io;
use std::io::Write;
use std::task::{Context, Poll};

pub(crate) struct ChunkedDecoder {
    amount_left: usize,
    state: DecoderState,
    chunk_size_buf: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecoderState {
    ChunkSize,
    ChunkSizeLf,
    Chunk,
    ChunkLf,
    End,
}

impl ChunkedDecoder {
    pub fn new() -> Self {
        ChunkedDecoder {
            amount_left: 0,
            state: DecoderState::ChunkSize,
            chunk_size_buf: Vec::with_capacity(32),
        }
    }

    pub fn poll_read(
        &mut self,
        cx: &mut Context,
        recv: &mut RecvReader,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            match self.state {
                DecoderState::ChunkSize => {
                    ready!(self.poll_chunk_size(cx, recv))?;
                    let chunk_size_s = String::from_utf8_lossy(&self.chunk_size_buf[..]);
                    self.amount_left = usize::from_str_radix(chunk_size_s.trim(), 16)
                        .ok()
                        .ok_or_else(|| {
                            io::Error::new(io::ErrorKind::InvalidData, "Not a number in chunk size")
                        })?;

                    trace!("Chunk size: {}", self.amount_left);

                    // reset for next time.
                    self.chunk_size_buf.resize(0, 0);

                    if self.amount_left == 0 {
                        self.state = DecoderState::End;
                    } else {
                        self.state = DecoderState::ChunkSizeLf;
                    }
                }
                DecoderState::ChunkSizeLf => {
                    ready!(self.poll_skip_until_lf(cx, recv)?);
                    self.state = DecoderState::Chunk;
                }
                DecoderState::Chunk => {
                    let to_read = self.amount_left.min(buf.len());
                    let amount_read = ready!(recv.poll_read(cx, &mut buf[0..to_read]))?;
                    self.amount_left -= amount_read;
                    trace!("Chunk read: {} left: {}", amount_read, self.amount_left);
                    if self.amount_left == 0 {
                        // chunk is over, read next chunk
                        self.state = DecoderState::ChunkLf;
                    }
                    return Ok(amount_read).into();
                }
                DecoderState::ChunkLf => {
                    ready!(self.poll_skip_until_lf(cx, recv)?);
                    self.state = DecoderState::ChunkSize;
                }
                DecoderState::End => return Ok(0).into(),
            }
        }
    }

    // 3\r\nhel\r\nb\r\nlo world!!!\r\n0\r\n\r\n
    fn poll_chunk_size(&mut self, cx: &mut Context, recv: &mut RecvReader) -> Poll<io::Result<()>> {
        // read until we get a non-numeric character. this could be
        // either \r or maybe a ; if we are using "extensions"
        let mut one = [0_u8; 1];
        loop {
            let amount = ready!(recv.poll_read(cx, &mut one[..]))?;
            if amount == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "EOF while reading chunk size",
                ))
                .into();
            }
            let c: char = one[0].into();
            // keep reading until we get ; or \r
            if c == ';' || c == '\r' {
                break;
            }
            if c == '0'
                || c == '1'
                || c == '2'
                || c == '3'
                || c == '4'
                || c == '5'
                || c == '6'
                || c == '7'
                || c == '8'
                || c == '9'
                || c == 'a'
                || c == 'b'
                || c == 'c'
                || c == 'd'
                || c == 'e'
                || c == 'f'
                || c == 'A'
                || c == 'B'
                || c == 'C'
                || c == 'D'
                || c == 'E'
                || c == 'F'
            {
                // good
            } else {
                let m = format!("Unexpected char in chunk size: {:?}", c);
                return Err(io::Error::new(io::ErrorKind::InvalidData, m)).into();
            }
            self.chunk_size_buf.push(one[0]);
            if self.chunk_size_buf.len() > 10 {
                // something is wrong.
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Too many chars in chunk size",
                ))
                .into();
            }
        }
        Ok(()).into()
    }

    // skip until we get a \n
    fn poll_skip_until_lf(
        &mut self,
        cx: &mut Context,
        recv: &mut RecvReader,
    ) -> Poll<io::Result<()>> {
        // skip until we get a \n
        let mut one = [0_u8; 1];
        loop {
            let amount = ready!(recv.poll_read(cx, &mut one[..]))?;
            if amount == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "EOF before finding lf",
                ))
                .into();
            }
            if one[0] == b'\n' {
                break;
            }
        }
        Ok(()).into()
    }
}

pub struct ChunkedEncoder;

impl ChunkedEncoder {
    pub fn write_chunk(buf: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
        let mut cur = io::Cursor::new(out);
        let header = format!("{}\r\n", buf.len()).into_bytes();
        cur.write_all(&header[..])?;
        cur.write_all(&buf[..])?;
        const CRLF: &[u8] = b"\r\n";
        cur.write_all(CRLF)?;
        Ok(())
    }
    pub fn write_finish(out: &mut Vec<u8>) -> Result<(), Error> {
        const END: &[u8] = b"0\r\n\r\n";
        let mut cur = io::Cursor::new(out);
        cur.write_all(END)?;
        Ok(())
    }
}
