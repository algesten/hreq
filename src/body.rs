//! Request and response body. content-encoding, charset etc.

use crate::charset::CharCodec;
use crate::head_ext::HeaderMapExt;
use crate::params::HReqParams;
use crate::peek::Peekable;
use crate::AsyncRead;
use crate::AsyncRuntime;
use crate::Error;
use bytes::Bytes;
use encoding_rs::Encoding;
use futures_util::future::poll_fn;
use futures_util::io::BufReader;
use futures_util::ready;
use hreq_h1::RecvStream as H1RecvStream;
use hreq_h2::RecvStream as H2RecvStream;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt;
use std::future::Future;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

#[cfg(feature = "gzip")]
use async_compression::futures::bufread::{GzipDecoder, GzipEncoder};

const BUF_SIZE: usize = 16_384;
const CT_TEXT: &str = "text/plain; charset=utf-8";
const CT_BIN: &str = "application/octet-stream";
const CT_JSON: &str = "application/json; charset=utf-8";

/// Body of an http request or response.
///
/// # Creating a body
///
/// Bodies are created either by using a constructor function, or the `Into` trait. The
/// into trait can be used where rust knows the result should be `Body` such as in the request
/// builder `.send()`.
///
/// ```no_run
/// use hreq::prelude::*;
///
/// let res = Request::post("https://post-to-server/")
///     // send has Into<Body> type, which means we can
///     // provide the Into type straight up
///     .send("some body content")
///     .block().unwrap();
/// ```
///
/// Or if we use `.into()` explcitly.
///
/// ```no_run
/// use hreq::Body;
///
/// // call into() to get a Body
/// let body: Body = "some body content".into();
/// ```
///
/// The constructor with corresponding expression usable with `Into`.
///
/// | Constructor                            | Into                 |
/// |----------------------------------------|----------------------|
/// | `Body::empty()`                        | `()`                 |
/// | `Body::from_str("abc")`                | `"abc"`              |
/// | `Body::from_string("abc".to_string())` | `"abc".to_string()`  |
/// | `Body::from_bytes(&[42_u8, 43_u8])`    | `&[42_u8, 43_u8]`    |
/// | `Body::from_vec(vec![42_u8, 43_u8])`   | `vec![42_u8, 43_u8]` |
/// | `Body::from_file(file)`                | `file`               |
/// | `Body::from_async_read(reader, None)`  | -                    |
/// | `Body::from_sync_read(reader, None)`   | -                    |
///
/// ## Readers and performance
///
/// The most performant way provide a large body is as an `AsyncRead`.
/// It will be streamed through hreq without using up too much memory.
///
/// Sync readers risks blocking the async runtime. This is not a big
/// concern if the reader is something like a `std::io::Cursor` over
/// a slice of memory, or maybe even a `std::fs::File` with a fast
/// disk. Choice of runtime also matters; `async-std` tries to
/// automatically "parry" blocking operations. Use sync readers
/// with caution and prefer async readers.
///
/// ## charset encoding
///
/// hreq automatically encodes the request body's character encoding
/// for MIME types starting `text/`.
///
/// The mechanic is triggered by setting a `content-type` request header
/// with the charset that is wanted:
///
///   * `content-type: text/html charset=iso8859-1`
///
/// The source material encoding is assumed to be `utf-8` unless
/// changed by [`charset_encode_source`].
///
/// The behavior can be completely disabled using [`charset_encode`].
///
/// ### compression
///
/// hreq can compress the request body. The mechanic is triggered by setting
/// a `content-encoding` header with the compression algorithm.
///
///   * `content-encoding: gzip`
///
/// The only supported algorithm is `gzip`.
///
/// # Reading a body
///
/// hreq provides a number of ways to read the contents of a body.
///
///   * [`Body.read()`]
///   * [`Body.read_to_vec()`]
///   * [`Body.read_to_string()`]
///   * [`Body.read_to_end()`]
///
/// Finaly `Body` implements `AsyncRead`, which means that in many cases, it can be used
/// as is in rust's async ecosystem.
///
/// ```no_run
/// use hreq::prelude::*;
/// use futures_util::io::AsyncReadExt;
///
/// let res = Request::get("https://my-special-host/")
///     .call().block().unwrap();
///
/// let mut body = res.into_body();
/// let mut first_ten = vec![0_u8; 10];
/// // read_exact comes from AsyncReadExt
/// body.read_exact(&mut first_ten[..]).block().unwrap();
/// ```
///
/// ## charset decoding
///
/// hreq automatically decodes the response body's character encoding
/// for MIME types starting `text/`.
///
/// The mechanic is triggered by receving a `content-type` response header
/// with the charset of the incoming body:
///
///   * `content-type: text/html charset=iso8859-1`
///
/// The wanted charset is assumed to be `utf-8` unless changed by [`charset_decode_target`].
///
/// The function can be disabled by using [`charset_decode`].
///
/// ## compression
///
/// hreq decompresses the request body. The mechanic is triggered by the presence
/// of a `content-encoding: gzip` response header.
///
/// One can "ask" the server to compress the response by providing a header like
/// `accept-encoding: gzip`. There's however no guarantee the server will provide compression.
///
/// The only supported algorithm is currently `gzip`.
///
/// [`Body.read()`]: struct.Body.html#method.read
/// [`Body.read_to_vec()`]: struct.Body.html#method.read_to_vec
/// [`Body.read_to_string()`]: struct.Body.html#method.read_to_string
/// [`Body.read_to_end()`]: struct.Body.html#method.read_to_end
/// [`charset_encode_source`]: trait.RequestBuilderExt.html#tymethod.charset_encode_source
/// [`charset_encode`]: trait.RequestBuilderExt.html#tymethod.charset_encode
/// [`charset_decode_target`]: trait.RequestBuilderExt.html#tymethod.charset_decode_target
/// [`charset_decode`]: trait.RequestBuilderExt.html#tymethod.charset_decode
pub struct Body {
    codec: BufReader<BodyCodec>,
    length: Option<u64>, // incoming length if given with reader
    content_typ: Option<String>,
    override_source_enc: Option<&'static Encoding>,
    has_read: bool,
    char_codec: Option<CharCodec>,
    deadline_fut: Option<Pin<Box<dyn Future<Output = io::Error> + Send + Sync>>>,
    unfinished_recs: Option<Arc<()>>,
}

impl Body {
    /// Constructs an empty request body.
    ///
    /// The `content-length` is know to be `0` and will be set for requests where a body
    /// is expected.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use hreq::Body;
    ///
    /// // The are the same.
    /// let body1: Body = Body::empty();
    /// let body2: Body = ().into();
    /// ```
    ///
    /// In `Request.send()` we can skip the `into()`
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// Request::get("https://get-from-here")
    ///     .call().block().unwrap();
    /// ```
    pub fn empty() -> Self {
        Self::new(BodyImpl::RequestEmpty, Some(0)).ctype(CT_TEXT)
    }

    /// Creates a body from a `&str` by cloning the data.
    ///
    /// Will automatically set a `content-length` header unless compression or
    /// chunked encoding is used.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use hreq::Body;
    ///
    /// // The are the same.
    /// let body1: Body = Body::from_str("Hello world");
    /// let body2: Body = "Hello world".into();
    /// ```
    ///
    /// In `Request.send()` we can skip the `into()`
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// Request::post("https://post-to-here")
    ///     .send("Hello world").block().unwrap();
    /// ```
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Self {
        Self::from_string(text.to_owned()).ctype(CT_TEXT)
    }

    /// Creates a body from a `String`.
    ///
    /// Will automatically set a `content-length` header unless compression or
    /// chunked encoding is used.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use hreq::Body;
    ///
    /// // The are the same.
    /// let body1: Body = Body::from_string("Hello world".to_string());
    /// let body2: Body = "Hello world".to_string().into();
    /// ```
    ///
    /// In `Request.send()` we can skip the `into()`
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// Request::post("https://post-to-here")
    ///     .send("Hello world".to_string()).block().unwrap();
    /// ```
    pub fn from_string(text: String) -> Self {
        let mut new = Self::from_vec(text.into_bytes()).ctype(CT_TEXT);
        // any string source is definitely UTF-8
        new.override_source_enc = Some(encoding_rs::UTF_8);
        new
    }

    /// Creates a body from a `&[u8]` by cloning the data.
    ///
    /// Will automatically set a `content-length` header unless compression or
    /// chunked encoding is used.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use hreq::Body;
    ///
    /// let data = [0x42, 0x43];
    ///
    /// // The are the same.
    /// let body1: Body = Body::from_bytes(&data[..]);
    /// let body2: Body = (&data[..]).into();
    /// ```
    ///
    /// In `Request.send()` we can skip the `into()`
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// let data = [0x42, 0x43];
    ///
    /// Request::post("https://post-to-here")
    ///     .send(&data[..]).block().unwrap();
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_vec(bytes.to_owned()).ctype(CT_BIN)
    }

    /// Creates a body from a `Vec<u8>`.
    ///
    /// Will automatically set a `content-length` header unless compression or
    /// chunked encoding is used.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use hreq::Body;
    ///
    /// // The are the same.
    /// let body1: Body = Body::from_vec(vec![0x42, 0x43]);
    /// let body2: Body = vec![0x42, 0x43].into();
    /// ```
    ///
    /// In `Request.send()` we can skip the `into()`
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    ///
    /// Request::post("https://post-to-here")
    ///     .send(vec![0x42, 0x43]).block().unwrap();
    /// ```
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        let len = bytes.len() as u64;
        Self::from_sync_read(io::Cursor::new(bytes), Some(len)).ctype(CT_BIN)
    }

    /// Creates a body from a `std::fs::File`.
    ///
    /// Despite the `std` origins, hreq will send this efficiently by reading
    /// the file in a non-blocking way.
    ///
    /// The request will have a `content-length` header unless compression or
    /// chunked encoding is used. Uses `content-type` from the headers if set , and
    /// falls back to `application/octet-stream`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use hreq::Body;
    /// use std::fs::File;
    ///
    /// // The are the same.
    /// let body1: Body = Body::from_file(File::open("myfile.txt").unwrap());
    /// let body2: Body = File::open("myfile.txt").unwrap().into();
    /// ```
    ///
    /// In `Request.send()` we can skip the `into()`
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    /// use std::fs::File;
    ///
    /// Request::post("https://post-to-here")
    ///     .send(File::open("myfile.txt").unwrap()).block().unwrap();
    /// ```
    pub fn from_file(file: std::fs::File) -> Self {
        let length = file.metadata().ok().map(|m| m.len());
        let reader = AsyncRuntime::file_to_reader(file);
        Body::from_async_read(reader, length).ctype(CT_BIN)
    }

    // pub(crate) async fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
    //     let body = path_to_body(path.as_ref()).await?;
    //     Ok(body)
    // }

    /// Creates a body from a JSON encodable type.
    ///
    /// This also sets the `content-type` and `content-length` headers.
    ///
    /// # Example
    ///
    /// ```
    /// use hreq::Body;
    /// use serde_derive::Serialize;
    ///
    /// #[derive(Serialize)]
    /// struct MyJsonThing {
    ///   name: String,
    ///   age: u8,
    /// }
    ///
    /// let json = MyJsonThing {
    ///   name: "Karl Kajal".to_string(),
    ///   age: 32,
    /// };
    ///
    /// let body = Body::from_json(&json);
    /// ```
    pub fn from_json<B: Serialize + ?Sized>(json: &B) -> Self {
        let vec = serde_json::to_vec(json).expect("Failed to encode JSON");
        Self::from_vec(vec).ctype(CT_JSON)
    }

    /// Creates a body from anything implementing the `AsyncRead` trait.
    ///
    /// This is a very efficient way of sending bodies since the content
    //  will be streamed through hreq without taking up much memory.
    ///
    /// The `content-length` header will be set depending on whether a
    /// `length` is provided. Combinations of charset and compression might
    /// make it so `content-length` is not known despite being provided.
    pub fn from_async_read<R>(reader: R, length: Option<u64>) -> Self
    where
        R: AsyncRead + Unpin + Send + Sync + 'static,
    {
        let boxed = Box::new(reader);
        Self::new(BodyImpl::RequestAsyncRead(boxed), length).ctype(CT_BIN)
    }

    /// Creates a body from anything implementing the (blocking) `std::io::Read` trait.
    ///
    /// Might block the async runtime, so whether using this is a good idea depends on
    /// circumstances. If the `Read` is just an `std::io::Cursor` over some memory or
    /// very fast file system, it might be ok. Some runtimes like `async-std` further
    /// have ways of detecting blocking operations.
    ///
    /// Use with care.
    ///
    /// The `content-length` header will be set depending on whether a
    /// `length` is provided. Combinations of charset and compression might
    /// make it so `content-length` is not known despite being provided.
    pub fn from_sync_read<R>(reader: R, length: Option<u64>) -> Self
    where
        R: io::Read + Send + Sync + 'static,
    {
        let boxed = Box::new(reader);
        Self::new(BodyImpl::RequestRead(boxed), length).ctype(CT_BIN)
    }

    /// Creates a new Body
    pub(crate) fn new(bimpl: BodyImpl, length: Option<u64>) -> Self {
        let reader = BodyReader::new(bimpl);
        let codec = BufReader::new(BodyCodec::deferred(reader));
        Body {
            codec,
            length,
            content_typ: None,
            override_source_enc: None,
            has_read: false,
            char_codec: None,
            deadline_fut: None,
            unfinished_recs: None,
        }
    }

    fn ctype(mut self, c: &'static str) -> Self {
        self.content_typ = Some(c.to_string());
        self
    }

    pub(crate) fn set_unfinished_recs(&mut self, unfin: Arc<()>) {
        self.unfinished_recs = Some(unfin);
    }

    /// Tells if we know _for sure_, there is no body.
    pub(crate) fn is_definitely_no_body(&self) -> bool {
        self.length.map(|l| l == 0).unwrap_or(false)
    }

    /// Tells if we know _for sure_, there is a body. Sized or unsized.
    pub(crate) fn is_definitely_a_body(&self) -> bool {
        self.length.map(|l| l > 0).unwrap_or(true)
    }

    /// Tells the length of the body _with content encoding_. This could
    /// take both gzip and charset into account, or just bail if we don't know.
    pub(crate) fn content_encoded_length(&self) -> Option<u64> {
        if self.codec.get_ref().affects_content_size() || self.char_codec.is_some() {
            // things like gzip will affect self.length
            None
        } else {
            self.length
        }
    }

    /// The content type set by the body, if any.
    pub(crate) fn content_type(&self) -> Option<&str> {
        self.content_typ.as_ref().map(|s| &s[..])
    }

    pub(crate) fn is_configurable(&self) -> bool {
        !self.has_read
    }

    /// Undo the effects of configure()
    #[cfg(feature = "server")]
    pub(crate) fn unconfigure(self) -> Self {
        let reader = self.codec.into_inner().into_inner();
        Body {
            codec: BufReader::new(BodyCodec::Deferred(Some(reader))),
            char_codec: None,
            ..self
        }
    }

    /// Configures the codecs in the body as part of the request or response.
    ///
    /// When calling this "content-encoding" and "content-type" must be settled.
    #[allow(clippy::collapsible_if)]
    pub(crate) fn configure(
        &mut self,
        params: &HReqParams,
        headers: &http::header::HeaderMap,
        is_incoming: bool,
    ) {
        if self.has_read {
            panic!("configure after body started reading");
        }

        self.deadline_fut = Some(params.deadline().delay_fut());

        let mut new_codec = None;
        if let BodyCodec::Deferred(reader) = self.codec.get_mut() {
            if let Some(reader) = reader.take() {
                let use_enc =
                    !is_incoming && params.content_encode || is_incoming && params.content_decode;
                new_codec = if use_enc {
                    let encoding = headers.get_str("content-encoding");
                    Some(BodyCodec::from_encoding(reader, encoding, is_incoming))
                } else {
                    Some(BodyCodec::Pass(reader))
                };
            }
        }

        if let Some(new_codec) = new_codec {
            // to avoid creating another BufReader
            *self.codec.get_mut() = new_codec;
        }

        let charset_config = if is_incoming {
            &params.charset_rx
        } else {
            &params.charset_tx
        };

        // TODO sniff charset from html pages like
        // <meta content="text/html; charset=UTF-8" http-equiv="Content-Type">
        if let Some((from, to)) =
            charset_config.resolve(is_incoming, headers, self.override_source_enc)
        {
            // don't use a codec if this is pass-thru
            if from == to {
                trace!("Charset codec pass through: {:?}", from);
            } else {
                self.char_codec = Some(CharCodec::new(from, to));
                trace!(
                    "Charset codec ({}): {:?}",
                    if is_incoming { "incoming" } else { "outgoing" },
                    self.char_codec
                );
            }
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn set_codec_pass(&mut self) {
        if let BodyCodec::Deferred(reader) = self.codec.get_mut() {
            if let Some(reader) = reader.take() {
                let new_codec = BodyCodec::Pass(reader);
                *self.codec.get_mut() = new_codec;
            }
        }
    }

    /// Read some bytes from this body into the specified buffer,
    /// returning how many bytes were read.
    ///
    /// If the returned amount is `0`, the end of the body has been reached.
    ///
    /// See [`charset_decode`] and [`charset_decode_target`] of headers and options that will
    /// affect `text/` MIME types.
    ///
    /// # Examples
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// let mut resp = Request::get("http://httpbin.org/html")
    ///     .call().block().unwrap();
    ///
    /// let mut data = vec![0_u8; 100];
    ///
    /// let amount = resp.body_mut().read(&mut data[..]).block().unwrap();
    ///
    /// assert!(amount >= 15);
    /// assert_eq!(&data[..15], b"<!DOCTYPE html>");
    /// ```
    ///
    /// [`charset_decode`]: trait.RequestBuilderExt.html#tymethod.charset_decode
    /// [`charset_decode_target`]: trait.RequestBuilderExt.html#tymethod.charset_decode_target
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        Ok(poll_fn(|cx| Pin::new(&mut *self).poll_read(cx, buf)).await?)
    }

    /// Reads to body to end into a new `Vec`.
    ///
    /// This can potentially take up a lot of memory (or even exhaust your RAM), depending on
    /// how big the response body is.
    ///
    /// See [`charset_decode`] and [`charset_decode_target`] of headers and options that will
    /// affect `text/` MIME types.
    ///
    /// # Examples
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// let mut resp = Request::get("http://httpbin.org/html")
    ///     .call().block().unwrap();
    ///
    /// let data = resp.body_mut().read_to_vec().block().unwrap();
    ///
    /// assert_eq!(&data[..15], b"<!DOCTYPE html>");
    /// ```
    ///
    /// [`charset_decode`]: trait.RequestBuilderExt.html#tymethod.charset_decode
    /// [`charset_decode_target`]: trait.RequestBuilderExt.html#tymethod.charset_decode_target
    pub async fn read_to_vec(&mut self) -> Result<Vec<u8>, Error> {
        let mut vec = Vec::with_capacity(8192);
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

    /// Reads to body to end into a new `String`.
    ///
    /// This can potentially take up a lot of memory (or even exhaust your RAM), depending on
    /// how big the response body is.
    ///
    /// Since a rust string is always `utf-8`, [`charset_decode_target`] is ignored.
    ///
    /// Panics if [`charset_decode`] is disabled and incoming data is not valid UTF-8.
    ///
    /// # Examples
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// let mut resp = Request::get("http://httpbin.org/html")
    ///     .call().block().unwrap();
    ///
    /// let data = resp.body_mut().read_to_string().block().unwrap();
    ///
    /// assert_eq!(&data[..15], "<!DOCTYPE html>");
    /// ```
    ///
    /// [`charset_decode`]: trait.RequestBuilderExt.html#tymethod.charset_decode
    /// [`charset_decode_target`]: trait.RequestBuilderExt.html#tymethod.charset_decode_target
    pub async fn read_to_string(&mut self) -> Result<String, Error> {
        // Remove any user set char encoder since we're reading to a rust string.
        if let Some(char_codec) = &mut self.char_codec {
            char_codec.remove_encoder();
        }
        let vec = self.read_to_vec().await?;
        Ok(String::from_utf8(vec).expect("Incoming body is not valid utf-8"))
    }

    /// Reads to body to end as a JSON string into a deserialized object.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use hreq::Body;
    /// use serde_derive::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct MyJsonThing {
    ///   name: String,
    ///   age: String,
    /// }
    ///
    /// let req: MyJsonThing = Request::get("http://foo")
    ///   .call().block().unwrap()
    ///   .read_json().unwrap();
    /// ```
    pub async fn read_to_json<T: DeserializeOwned>(&mut self) -> Result<T, Error> {
        let s = self.read_to_string().await?;
        Ok(serde_json::from_str(&s)?)
    }

    /// Reads to body to end and discards it.
    ///
    /// HTTP/1.1 has no "multiplexing" of several concurrent request over the same socket;
    /// One must read the previous request's body to end before being able to
    /// read the next response header.
    ///
    /// For pooled connections we can't reuse the connection until the previous body has
    /// been exhausted.
    ///
    /// # Examples
    ///
    /// ```
    /// use hreq::prelude::*;
    ///
    /// let mut resp = Request::get("https://httpbin.org/get")
    ///     .call().block().unwrap();
    ///
    /// resp.body_mut().read_to_end();
    /// ```
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
}

#[allow(clippy::large_enum_variant)]
enum BodyCodec {
    Deferred(Option<BodyReader>),
    Pass(BodyReader),
    #[cfg(feature = "gzip")]
    GzipDecoder(GzipDecoder<BufReader<BodyReader>>),
    #[cfg(feature = "gzip")]
    GzipEncoder(GzipEncoder<BufReader<BodyReader>>),
}

impl BodyCodec {
    fn deferred(reader: BodyReader) -> Self {
        BodyCodec::Deferred(Some(reader))
    }

    #[cfg(feature = "server")]
    fn into_inner(self) -> BodyReader {
        match self {
            BodyCodec::Deferred(_) => panic!("into_inner() on Deferred"),
            BodyCodec::Pass(b) => b,
            #[cfg(feature = "gzip")]
            BodyCodec::GzipDecoder(z) => z.into_inner().into_inner(),
            #[cfg(feature = "gzip")]
            BodyCodec::GzipEncoder(z) => z.into_inner().into_inner(),
        }
    }

    fn from_encoding(reader: BodyReader, encoding: Option<&str>, is_incoming: bool) -> Self {
        trace!("Body codec from encoding: {:?}", encoding);
        match (encoding, is_incoming) {
            (None, _) => BodyCodec::Pass(reader),
            #[cfg(feature = "gzip")]
            (Some("gzip"), true) => {
                let buf = BufReader::new(reader);
                BodyCodec::GzipDecoder(GzipDecoder::new(buf))
            }
            #[cfg(feature = "gzip")]
            (Some("gzip"), false) => {
                let buf = BufReader::new(reader);
                BodyCodec::GzipEncoder(GzipEncoder::new(buf))
            }
            _ => {
                warn!("Unknown content-encoding: {:?}", encoding);
                BodyCodec::Pass(reader)
            }
        }
    }

    fn reader_ref(&self) -> Option<&BodyReader> {
        match self {
            BodyCodec::Deferred(_) => None,
            BodyCodec::Pass(r) => Some(r),
            #[cfg(feature = "gzip")]
            BodyCodec::GzipDecoder(r) => Some(r.get_ref().get_ref()),
            #[cfg(feature = "gzip")]
            BodyCodec::GzipEncoder(r) => Some(r.get_ref().get_ref()),
        }
    }

    fn affects_content_size(&self) -> bool {
        match self {
            BodyCodec::Deferred(_) => false,
            BodyCodec::Pass(_) => false,
            #[cfg(feature = "gzip")]
            BodyCodec::GzipDecoder(_) => true,
            #[cfg(feature = "gzip")]
            BodyCodec::GzipEncoder(_) => true,
        }
    }
}

pub struct BodyReader {
    imp: BodyImpl,
    leftover_bytes: Option<Bytes>,
    is_finished: bool,
}

pub enum BodyImpl {
    RequestEmpty,
    RequestAsyncRead(Box<dyn AsyncRead + Unpin + Send + Sync>),
    RequestRead(Box<dyn io::Read + Send + Sync>),
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
            BodyImpl::Http1(recv) => ready!(Pin::new(recv).poll_read(cx, buf))?,
            BodyImpl::Http2(recv) => {
                if let Some(data) = ready!(recv.poll_data(cx)) {
                    let data = data.map_err(|e| {
                        let other = format!("Other h2 error (poll_data): {}", e);
                        e.into_io()
                            .unwrap_or_else(|| io::Error::new(io::ErrorKind::Other, other))
                    })?;
                    recv.flow_control()
                        .release_capacity(data.len())
                        .map_err(|e| {
                            let other = format!("Other h2 error (release_capacity): {}", e);
                            e.into_io()
                                .unwrap_or_else(|| io::Error::new(io::ErrorKind::Other, other))
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
        Body::from_str(s)
    }
}

impl<'a> From<&'a String> for Body {
    fn from(s: &'a String) -> Self {
        Body::from_string(s.clone())
    }
}

impl From<String> for Body {
    fn from(s: String) -> Self {
        Body::from_string(s)
    }
}

impl<'a> From<&'a [u8]> for Body {
    fn from(bytes: &'a [u8]) -> Self {
        Body::from_bytes(bytes)
    }
}

impl From<Vec<u8>> for Body {
    fn from(bytes: Vec<u8>) -> Self {
        Body::from_vec(bytes)
    }
}

impl<'a> From<&'a Vec<u8>> for Body {
    fn from(bytes: &'a Vec<u8>) -> Self {
        Body::from_vec(bytes.clone())
    }
}

impl From<std::fs::File> for Body {
    fn from(file: std::fs::File) -> Self {
        Body::from_file(file)
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

        // use deadline if it's present
        let deadl = this.deadline_fut.as_mut();
        if let Some(deadl) = deadl {
            if let Poll::Ready(err) = deadl.as_mut().poll(cx) {
                return Poll::Ready(Err(err));
            }
        }

        let amount = ready!(if let Some(char_codec) = &mut this.char_codec {
            char_codec.poll_codec(cx, &mut this.codec, buf)
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
            BodyCodec::Pass(r) => Pin::new(r).poll_read(cx, buf),
            #[cfg(feature = "gzip")]
            BodyCodec::GzipDecoder(r) => Pin::new(r).poll_read(cx, buf),
            #[cfg(feature = "gzip")]
            BodyCodec::GzipEncoder(r) => Pin::new(r).poll_read(cx, buf),
        }
    }
}

impl fmt::Debug for BodyCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BodyCodec::Deferred(_) => write!(f, "defer"),
            BodyCodec::Pass(_) => write!(f, "pass"),
            #[cfg(feature = "gzip")]
            BodyCodec::GzipDecoder(_) => write!(f, "gzip_dec"),
            #[cfg(feature = "gzip")]
            BodyCodec::GzipEncoder(_) => write!(f, "gzip_enc"),
        }
    }
}

impl fmt::Debug for BodyReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.imp)
    }
}

impl fmt::Debug for BodyImpl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BodyImpl::RequestEmpty => write!(f, "empty"),
            BodyImpl::RequestAsyncRead(_) => write!(f, "async"),
            BodyImpl::RequestRead(_) => write!(f, "sync"),
            BodyImpl::Http1(_) => write!(f, "http1"),
            BodyImpl::Http2(_) => write!(f, "http2"),
        }
    }
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Body {{ reader: ")?;
        match self.codec.get_ref().reader_ref() {
            Some(v) => write!(f, "{:?}", v),
            None => write!(f, "none"),
        }?;
        write!(f, ", codec: {:?}", self.codec.get_ref())?;
        if let Some(char_codec) = &self.char_codec {
            write!(f, ", char_codec: {:?}", char_codec)?;
        }
        write!(f, ", len: ")?;
        match self.content_encoded_length() {
            Some(v) => write!(f, "{}", v),
            None => write!(f, "unknown"),
        }?;
        write!(f, " }}")
    }
}

pub(crate) async fn path_to_body(absolute: &Path) -> Result<Body, io::Error> {
    let file = std::fs::File::open(&absolute)?;

    let length = file.metadata()?.len();

    let guess = mime_guess::from_path(&absolute);
    let mut content_type = if let Some(mime) = guess.first() {
        mime.to_string()
    } else {
        "application/octet-stream".to_string()
    };

    let read = AsyncRuntime::file_to_reader(file);

    const PEEK_LEN: usize = 1024;

    let mut peek = Peekable::new(read, PEEK_LEN);

    // For text files, we try to guess the character encoding.
    if content_type.starts_with("text/") {
        // attempt to guess charset
        let max = (PEEK_LEN as u64).min(length);

        let buf = peek.peek(max as usize).await?;

        let mut det = chardetng::EncodingDetector::new();
        det.feed(buf, length < PEEK_LEN as u64);

        let enc = det.guess(None, true);

        content_type.push_str(&format!("; charset={}", enc.name()));
    }

    let mut body = Body::from_async_read(peek, Some(length));
    body.content_typ = Some(content_type);

    Ok(body)
}
