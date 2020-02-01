use crate::charset::CharCodec;
use crate::deadline::Deadline;
use crate::h1::RecvStream as H1RecvStream;
use crate::res_ext::HeaderMapExt;
use crate::AsyncRead;
use crate::Error;
use bytes::Bytes;
use futures_util::future::poll_fn;
use futures_util::io::BufReader;
use futures_util::ready;
use h2::RecvStream as H2RecvStream;
use std::fs;
use std::io;
use std::mem;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

#[cfg(feature = "gzip")]
use async_compression::futures::bufread::{GzipDecoder, GzipEncoder};

const BUF_SIZE: usize = 16_384;

pub struct Body {
    codec: BufReader<BodyCodec>,
    length: Option<u64>,
    has_read: bool,
    char_codec: Option<CharCodec>,
    content_length: Option<usize>,
    deadline: Deadline,
    unfinished_recs: Option<Arc<()>>,
}

impl Body {
    pub fn empty() -> Self {
        Self::new(BodyImpl::RequestEmpty, Some(0), None)
    }

    pub fn from_async_read<R>(reader: R, length: Option<u64>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let boxed = Box::new(reader);
        Self::new(BodyImpl::RequestAsyncRead(boxed), length, None)
    }

    pub fn from_sync_read<R>(reader: R, length: Option<u64>) -> Self
    where
        R: io::Read + Send + 'static,
    {
        let boxed = Box::new(reader);
        Self::new(BodyImpl::RequestRead(boxed), length, None)
    }

    pub(crate) fn new(bimpl: BodyImpl, length: Option<u64>, unfin: Option<Arc<()>>) -> Self {
        let reader = BodyReader::new(bimpl);
        let codec = BufReader::new(BodyCodec::deferred(reader));
        Body {
            codec,
            length,
            has_read: false,
            char_codec: None,
            content_length: None,
            deadline: Deadline::inert(),
            unfinished_recs: unfin,
        }
    }

    pub(crate) fn length(&self) -> Option<u64> {
        self.length
    }

    pub(crate) fn is_definitely_no_body(&self) -> bool {
        self.length.map(|l| l == 0).unwrap_or(false)
    }

    pub(crate) fn configure(
        &mut self,
        deadline: Deadline,
        headers: &http::header::HeaderMap,
        is_response: bool,
    ) {
        if self.has_read {
            panic!("configure after body started reading");
        }

        self.deadline = deadline;

        let mut new_codec = None;
        if let BodyCodec::Deferred(reader) = self.codec.get_mut() {
            if let Some(reader) = reader.take() {
                let encoding = headers.get_str("content-encoding");
                new_codec = Some(BodyCodec::from_encoding(reader, encoding, is_response))
            }
        }

        if let Some(new_codec) = new_codec {
            // to avoid creating another BufReader
            mem::replace(self.codec.get_mut(), new_codec);
        }

        // TODO do we want charset conversion for request bodies?
        if is_response {
            // TODO sniff charset from html pages like
            // <meta content="text/html; charset=UTF-8" http-equiv="Content-Type">
            if let Some(charset) = charset_from_headers(headers) {
                self.char_codec = Some(CharCodec::new(charset, is_response));
            }
        }

        self.content_length = headers.get_as("content-length");
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        Ok(poll_fn(|cx| Pin::new(&mut *self).poll_read(cx, buf)).await?)
    }

    pub async fn read_to_end(&mut self) -> Result<(), Error> {
        let mut buf = vec![0_u8; BUF_SIZE];
        loop {
            let read = self.read(&mut buf).await?;
            if read == 0 {
                break;
            }
        }
        Ok(())
    }

    pub async fn read_to_vec(&mut self) -> Result<Vec<u8>, Error> {
        // use content length as capacity if we know it. given the content can be both
        // gzipped and encoded with some charset, it might be wrong.
        // TODO multiply this guess with good values for gzip (and charset?)
        let mut vec = Vec::with_capacity(self.content_length.unwrap_or(8192));
        let mut idx = 0;
        loop {
            if idx == vec.len() {
                vec.resize(idx + 8192, 0);
            }
            let amount = self.read(&mut vec[idx..]).await?;
            if amount == 0 {
                vec.resize(idx, 0);
                break;
            }
            idx += amount;
        }
        Ok(vec)
    }

    pub async fn read_to_string(&mut self) -> Result<String, Error> {
        let vec = self.read_to_vec().await?;
        Ok(String::from_utf8_lossy(&vec).into())
    }
}

#[allow(clippy::large_enum_variant)]
enum BodyCodec {
    Deferred(Option<BodyReader>),
    Plain(BodyReader),
    #[cfg(feature = "gzip")]
    GzipDecoder(GzipDecoder<BufReader<BodyReader>>),
    #[cfg(feature = "gzip")]
    GzipEncoder(GzipEncoder<BufReader<BodyReader>>),
}

impl BodyCodec {
    fn deferred(reader: BodyReader) -> Self {
        BodyCodec::Deferred(Some(reader))
    }

    fn from_encoding(reader: BodyReader, encoding: Option<&str>, is_decode: bool) -> Self {
        trace!("Body codec: {:?}", encoding);
        match (encoding, is_decode) {
            (None, _) => BodyCodec::Plain(reader),
            #[cfg(feature = "gzip")]
            (Some("gzip"), true) => {
                let buf = BufReader::new(reader);
                BodyCodec::GzipDecoder(GzipDecoder::new(buf))
            }
            #[cfg(feature = "gzip")]
            (Some("gzip"), false) => {
                let buf = BufReader::new(reader);
                let comp = flate2::Compression::fast();
                BodyCodec::GzipEncoder(GzipEncoder::new(buf, comp))
            }
            _ => {
                warn!("Unknown content-encoding: {:?}", encoding);
                BodyCodec::Plain(reader)
            }
        }
    }

    // fn reader_mut(&mut self) -> &mut BodyReader {
    //     match self {
    //         BodyCodec::Deferred(_) => panic!("into_inner() on BodyCodec::Deferred"),
    //         BodyCodec::Plain(r) => r,
    //         #[cfg(feature = "gzip")]
    //         BodyCodec::GzipDecoder(r) => r.get_mut().get_mut(),
    //         #[cfg(feature = "gzip")]
    //         BodyCodec::GzipEncoder(r) => r.get_mut().get_mut(),
    //     }
    // }
}

pub struct BodyReader {
    imp: BodyImpl,
    leftover_bytes: Option<Bytes>,
    is_finished: bool,
}

pub enum BodyImpl {
    RequestEmpty,
    RequestAsyncRead(Box<dyn AsyncRead + Unpin + Send>),
    RequestRead(Box<dyn io::Read + Send>),
    Http1(H1RecvStream),
    Http2(H2RecvStream),
}

impl BodyReader {
    fn new(imp: BodyImpl) -> Self {
        BodyReader {
            imp,
            leftover_bytes: None,
            is_finished: false,
        }
    }

    // fn is_http11(&self) -> bool {
    //     match &self.imp {
    //         BodyImpl::Http1(_, _) => true,
    //         _ => false,
    //     }
    // }

    // helper to shuffle Bytes into a &[u8] and handle the remains.
    fn bytes_to_buf(&mut self, mut data: Bytes, buf: &mut [u8]) -> usize {
        let max = data.len().min(buf.len());
        (&mut buf[0..max]).copy_from_slice(&data[0..max]);
        let remain = if max < data.len() {
            Some(data.split_off(max))
        } else {
            None
        };
        self.leftover_bytes = remain;
        max
    }
}

impl AsyncRead for BodyReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.is_finished {
            return Ok(0).into();
        }
        // h2 streams might have leftovers to use up before reading any more.
        if let Some(data) = this.leftover_bytes.take() {
            let amount = this.bytes_to_buf(data, buf);
            return Ok(amount).into();
        }
        let read = match &mut this.imp {
            BodyImpl::RequestEmpty => 0,
            BodyImpl::RequestAsyncRead(reader) => ready!(Pin::new(reader).poll_read(cx, buf))?,
            BodyImpl::RequestRead(reader) => match reader.read(buf) {
                Ok(v) => v,
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        panic!("Body::from_sync_read() failed with ErrorKind::WouldBlock. Use Body::from_async_read()");
                    }
                    return Err(e).into();
                }
            },
            BodyImpl::Http1(recv) => ready!(recv.poll_read(cx, buf))?,
            BodyImpl::Http2(recv) => {
                if let Some(data) = ready!(recv.poll_data(cx)) {
                    let data = data.map_err(|e| {
                        e.into_io().unwrap_or_else(|| {
                            io::Error::new(io::ErrorKind::Other, "Other h2 error")
                        })
                    })?;
                    this.bytes_to_buf(data, buf)
                } else {
                    0
                }
            }
        };
        if read == 0 {
            this.is_finished = true;
        }
        Ok(read).into()
    }
}

impl From<()> for Body {
    fn from(_: ()) -> Self {
        Body::empty()
    }
}

impl<'a> From<&'a str> for Body {
    fn from(s: &'a str) -> Self {
        s.to_owned().into()
    }
}

impl<'a> From<&'a String> for Body {
    fn from(s: &'a String) -> Self {
        s.clone().into()
    }
}

impl From<String> for Body {
    fn from(s: String) -> Self {
        let bytes = s.into_bytes();
        bytes.into()
    }
}

impl<'a> From<&'a [u8]> for Body {
    fn from(bytes: &'a [u8]) -> Self {
        bytes.to_vec().into()
    }
}

impl From<Vec<u8>> for Body {
    fn from(bytes: Vec<u8>) -> Self {
        let len = bytes.len() as u64;
        let cursor = io::Cursor::new(bytes);
        Body::from_sync_read(cursor, Some(len))
    }
}

impl From<fs::File> for Body {
    fn from(file: fs::File) -> Self {
        let len = file.metadata().ok().map(|m| m.len());
        Body::from_sync_read(file, len)
    }
}

impl AsyncRead for Body {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if !this.has_read {
            this.has_read = true;
        }
        if let Some(err) = this.deadline.check_time_left() {
            return Poll::Ready(Err(err));
        }
        let amount = ready!(if let Some(char_codec) = &mut this.char_codec {
            char_codec.poll_decode(cx, &mut this.codec, buf)
        } else {
            Pin::new(&mut this.codec).poll_read(cx, buf)
        })?;
        if amount == 0 {
            // by removing this arc, we reduce the unfinished recs count.
            this.unfinished_recs.take();
        }
        Ok(amount).into()
    }
}

impl AsyncRead for BodyCodec {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        match this {
            BodyCodec::Deferred(_) => panic!("poll_read on BodyCodec::Deferred"),
            BodyCodec::Plain(r) => Pin::new(r).poll_read(cx, buf),
            #[cfg(feature = "gzip")]
            BodyCodec::GzipDecoder(r) => Pin::new(r).poll_read(cx, buf),
            #[cfg(feature = "gzip")]
            BodyCodec::GzipEncoder(r) => Pin::new(r).poll_read(cx, buf),
        }
    }
}

fn charset_from_headers(headers: &http::header::HeaderMap) -> Option<&str> {
    headers
        .get_str("content-type")
        .and_then(|v| {
            // only consider text content
            if v.starts_with("text/") {
                Some(v)
            } else {
                None
            }
        })
        .and_then(|x| {
            // text/html; charset=utf-8
            let s = x.split(';');
            s.last()
        })
        .and_then(|x| {
            // charset=utf-8
            let mut s = x.split('=');
            s.nth(1)
        })
}
