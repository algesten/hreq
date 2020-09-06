use super::Reply;
use crate::head_ext::HeaderMapExt;
use crate::server::handler::Handler;
use crate::server::limit::ContentLengthRead;
use crate::server::peek::Peekable;
use crate::server::{ResponseBuilderExt, ServerRequestExt};
use crate::AsyncReadSeek;
use crate::AsyncRuntime;
use crate::Body;
use crate::Error;
use futures_util::io::AsyncSeekExt;
use http::Request;
use http::StatusCode;
use httpdate::{fmt_http_date, parse_http_date};
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::SystemTime;

/// Serve static files.
///
/// * Supports `HEAD` requests.
/// * Directory default "index.html" (on windows "index.htm").
/// * Caching using [`if-modified-since`] and [`must-revalidate`].
/// * Maps file extension to `content-type` using [mime-guess].
/// * Guesses character encoding of `text/*` mime types using [chardetng].
/// * Supports [range requests].
///
/// # Example
///
/// A request for `http://localhost:3000/static/foo.html` would attempt to read a file
/// from disk `/www/static/foo.html`.
///
/// ```no_run
/// use hreq::prelude::*;
/// use hreq::server::Static;
///
/// async fn start_server() {
///    let mut server = Server::new();
///
///    server.at("/static/*file").all(Static::dir("/www/static"));
///
///    let (handle, addr) = server.listen(3000).await.unwrap();
///
///    handle.keep_alive().await;
/// }
/// ```
///
/// [`if-modified-since`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Conditional_requests
/// [`must-revalidate`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Cache-Control
/// [mime-guess]: https://crates.io/crates/mime_guess
/// [chardetng]: https://crates.io/crates/chardetng
/// [range requests]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Range_requests
#[derive(Debug)]
pub struct Static {
    root: PathBuf,
    use_path_param: bool,
    index_file: Option<String>,
}

impl Static {
    /// Creates a handler that serves files from a directory.
    ///
    /// * Must be used with a path parameter `/path/*name`.
    /// * Cannot serve files from parent paths. I.e. `/path/../foo.txt`.
    /// * Path is either absolute, or made absolute by using [`current_dir`] upon creation.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    /// use hreq::server::Static;
    ///
    /// async fn start_server() {
    ///    let mut server = Server::new();
    ///
    ///    server.at("/static/*file").all(Static::dir("/www/static"));
    ///
    ///    let (handle, addr) = server.listen(3000).await.unwrap();
    ///
    ///    handle.keep_alive().await;
    /// }
    /// ```
    ///
    /// [`current_dir`]: https://doc.rust-lang.org/std/env/fn.current_dir.html
    pub fn dir(path: impl AsRef<Path>) -> Self {
        Static::new(path, true)
    }

    /// Creates a handler that serves the same file for every request.
    ///
    /// * Path is either absolute, or made absolute by using [`current_dir`] upon creation.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    /// use hreq::server::Static;
    ///
    /// async fn start_server() {
    ///    let mut server = Server::new();
    ///
    ///    server.at("/*any").all(Static::file("/www/single-page-app.html"));
    ///
    ///    let (handle, addr) = server.listen(3000).await.unwrap();
    ///
    ///    handle.keep_alive().await;
    /// }
    /// ```
    ///
    /// [`current_dir`]: https://doc.rust-lang.org/std/env/fn.current_dir.html
    pub fn file(path: impl AsRef<Path>) -> Self {
        Static::new(path, false)
    }

    /// Send a file as part of a handler.
    ///
    /// Inspired by express js [`res.sendFile`].
    ///
    /// ```no_run
    /// use hreq::prelude::*;
    /// use hreq::server::Static;
    ///
    /// async fn start_server() {
    ///    let mut server = Server::new();
    ///
    ///    server.at("/do/something").get(do_something);
    ///
    ///    let (handle, addr) = server.listen(3000).await.unwrap();
    ///
    ///    handle.keep_alive().await;
    /// }
    ///
    /// async fn do_something(
    ///   req: http::Request<Body>
    /// ) -> Result<http::Response<Body>, hreq::Error> {
    ///   // do stuff with req.
    ///
    ///   Static::send_file(&req, "/www/my-file.txt").await
    /// }
    /// ```
    ///
    /// [`res.sendFile`]: http://expressjs.com/en/api.html#res.sendFile
    pub async fn send_file(
        req: &http::Request<Body>,
        path: impl AsRef<Path>,
    ) -> Result<http::Response<Body>, Error> {
        let st = Static::new("", false);
        st.handle(req, Some(path.as_ref())).await
    }

    fn new(path: impl AsRef<Path>, use_path_param: bool) -> Self {
        let path = path.as_ref();

        let root = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap().join(path)
        };

        let index_file = Some(
            if cfg!(target_os = "windows") {
                "index.htm"
            } else {
                "index.html"
            }
            .to_string(),
        );

        Static {
            root,
            use_path_param,
            index_file,
        }
    }

    /// Change directory index file.
    ///
    /// This defaults to "index.html" and on windows "index.htm".
    ///
    /// Set `None` to turn off index files (and respond with 404 instead).
    pub fn index_file(mut self, file: Option<&str>) -> Self {
        self.index_file = file.map(|v| v.to_string());
        self
    }

    fn resolve_path(&self, path: Option<&Path>) -> io::Result<PathBuf> {
        // Use the segment from the /*name appended to the dir we use.
        // This could be relative such as `"/path/to/serve"` + `"blah/../foo.txt"`
        let mut root = self.root.clone();

        // Canonicalized form of root. This must exist,
        let root_canon = root.canonicalize()?;

        if let Some(path) = path {
            root.push(&path);
        }

        // By canonicalizing we remove any `..`. This errors if the file doesn't exist.
        let absolute = root.canonicalize()?;

        // This is a security check that the resolved doesn't go to a parent dir/file.
        // "/path/to/serve" + "../../../etc/passwd". It works because self.0 is canonicalized.
        if !absolute.starts_with(&root_canon) {
            debug!("Path not under base path: {:?}", path);
            return Err(io::Error::new(io::ErrorKind::NotFound, "Base path"));
        }

        Ok(absolute)
    }

    async fn handle(
        &self,
        req: &Request<Body>,
        path: Option<&Path>,
    ) -> Result<http::Response<Body>, Error> {
        // only accept HEAD or GET, all other we error on.
        if req.method() != http::Method::GET && req.method() != http::Method::HEAD {
            return Ok(err(http::StatusCode::METHOD_NOT_ALLOWED, "Use GET or HEAD"));
        }

        let mut absolute = match self.resolve_path(path) {
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    return Ok(err(StatusCode::NOT_FOUND, "Not found"));
                } else {
                    warn!("Failed to canonicalize ({:?}): {:?}", path, e);
                    return Ok(err(StatusCode::BAD_REQUEST, "Bad request"));
                }
            }
            Ok(v) => v,
        };

        if absolute.is_dir() {
            if let Some(index) = &self.index_file {
                absolute.push(index);
            } else {
                return Ok(err(StatusCode::NOT_FOUND, "Not found"));
            }
        }

        let d = Dispatch::new(absolute, req);

        Ok(d.into_response().await?)
    }
}

impl Handler for Static {
    fn call<'a>(&'a self, req: Request<Body>) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>> {
        if self.use_path_param {
            let params = req.path_params();

            if params.is_empty() || params.len() > 1 {
                let msg = "serve_dir() must be used with none path param. Example: /dir/*file";

                warn!("{}", msg);

                Box::pin(async move { err(StatusCode::INTERNAL_SERVER_ERROR, msg).into() })
            } else {
                let path: PathBuf = params[0].1.to_owned().into();

                Box::pin(async move {
                    let ret = self.handle(&req, Some(&path)).await;
                    ret.into()
                })
            }
        } else {
            Box::pin(async move {
                let ret = self.handle(&req, None).await;
                ret.into()
            })
        }
    }
}

fn err(status: http::StatusCode, msg: &str) -> http::Response<Body> {
    http::Response::builder()
        .status(status)
        .body(msg.into())
        .unwrap()
}
struct Dispatch {
    file: PathBuf,
    if_modified_since: Option<SystemTime>,
    is_head: bool,
    range: Option<(u64, u64)>,
}

impl Dispatch {
    fn new(file: PathBuf, req: &http::Request<Body>) -> Self {
        let if_modified_since = req
            .headers()
            .get_as::<String>("if-modified-since")
            .and_then(|v| parse_http_date(&v).ok());

        let is_head = req.method() == http::Method::HEAD;

        let is_get = req.method() == http::Method::GET;

        // https://tools.ietf.org/html/rfc7233#section-3.1
        // A server MUST ignore a Range header field received with a request method other than GET.
        //
        // Range: bytes=0-1023
        let range = if is_get {
            req.headers()
                .get("range")
                .and_then(|v| v.to_str().ok())
                .filter(|v| v.starts_with("bytes="))
                .map(|v| &v[6..])
                .and_then(|v| {
                    if let Some(i) = v.find('-') {
                        Some((&v[0..i], &v[i + 1..]))
                    } else {
                        None
                    }
                })
                .and_then(|(s, e)| match (s.parse::<u64>(), e.parse::<u64>()) {
                    (Ok(s), Ok(e)) => Some((s, e)),
                    _ => None,
                })
                // incoming range is end inclusive, internal arithmetic is exclusive.
                .map(|(s, e)| (s, e + 1))
        } else {
            None
        };

        Dispatch {
            file,
            if_modified_since,
            is_head,
            range,
        }
    }

    async fn into_response(self) -> Result<http::Response<Body>, Error> {
        match self.into_response_io().await {
            Ok(v) => Ok(v),
            Err(e) => match e.kind() {
                io::ErrorKind::NotFound => Ok(err(StatusCode::NOT_FOUND, "File not found")),
                io::ErrorKind::PermissionDenied => {
                    Ok(err(StatusCode::FORBIDDEN, "File permission denied"))
                }
                _ => Err(e.into()),
            },
        }
    }

    async fn into_response_io(self) -> io::Result<http::Response<Body>> {
        let file = std::fs::File::open(&self.file)?;
        let meta = file.metadata()?;
        let length = meta.len();
        let modified = meta.modified()?;

        if let Some(since) = self.if_modified_since {
            // for files that updated, since will be earlier than modified.
            if let Ok(diff) = modified.duration_since(since) {
                // The web format has a resultion of seconds: Fri, 15 May 2015 15:34:21 GMT
                // So the diff must be less than a second.
                if diff.as_secs_f32() < 1.0 {
                    return Ok(http::Response::builder()
                        // https://tools.ietf.org/html/rfc7232#section-4.1
                        //
                        // The server generating a 304 response MUST generate any of the
                        // following header fields that would have been sent in a 200 (OK)
                        // response to the same request: Cache-Control, Content-Location, Date,
                        // ETag, Expires, and Vary.
                        .status(http::StatusCode::NOT_MODIFIED)
                        .header("cache-control", "must-revalidate")
                        .header("last-modified", fmt_http_date(modified))
                        .body(Body::empty())
                        .unwrap());
                }
            }
        }

        let guess = mime_guess::from_path(&self.file);
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

        let res = http::Response::builder()
            .header("cache-control", "must-revalidate")
            .header("accept-ranges", "bytes")
            .header("content-type", content_type)
            .charset_encode(false) // serve text files as is
            .header("last-modified", httpdate::fmt_http_date(modified));

        let (body, res) = self.create_body(length, peek, res).await?;

        Ok(res.body(body).unwrap())
    }

    async fn create_body<Z: AsyncReadSeek + Unpin + Send + Sync + 'static>(
        &self,
        length: u64,
        mut reader: Z,
        mut res: http::response::Builder,
    ) -> io::Result<(Body, http::response::Builder)> {
        let body = if self.is_head {
            res = res.header("content-length", length.to_string());

            Body::empty()
        } else if let Some((start, end)) = self.range {
            if end <= start || start >= length || end > length {
                debug!("Bad range [{}..{}] of {}", start, end, length);

                res = res.status(http::StatusCode::RANGE_NOT_SATISFIABLE);

                Body::empty()
            } else {
                debug!("Serve range [{}..{}] of {}", start, end, length);

                reader.seek(io::SeekFrom::Start(start)).await?;

                let sub = end - start;

                let limit = ContentLengthRead::new(reader, sub);

                res = res.status(http::StatusCode::PARTIAL_CONTENT).header(
                    "content-range",
                    format!("bytes {}-{}/{}", start, end - 1, length),
                );

                Body::from_async_read(limit, Some(sub))
            }
        } else {
            Body::from_async_read(reader, Some(length))
        };

        Ok((body, res))
    }
}
