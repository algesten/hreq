use super::Reply;
use crate::head_ext::HeaderMapExt;
use crate::limit::ContentLengthRead;
use crate::peek::Peekable;
use crate::server::handler::Handler;
use crate::server::{ResponseBuilderExt, ServerRequestExt};
use crate::AsyncReadSeek;
use crate::AsyncRuntime;
use crate::Body;
use crate::Error;
use futures_util::io::AsyncSeekExt;
use http::Request;
use httpdate::{fmt_http_date, parse_http_date};
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::SystemTime;

/// Serve files from a directory.
///
/// Must be used with one path parameter `/path/:name`. Use a rest parameter `/path/*name`, to
/// files also from subdirectories.
///
///
/// Cannot serve files from parent paths. I.e. `/path/../foo.txt`.
///
/// Panics if the path served can not be [`std::fs::canonicalize`].
///
/// [`std::fs::canonicalize`]: https://doc.rust-lang.org/std/fs/fn.canonicalize.html
pub fn serve_dir(path: impl AsRef<Path>) -> impl Handler {
    let path = path.as_ref();

    match path.canonicalize() {
        Err(e) => {
            panic!("Failed to canonicalize path ({:?}): {:?}", path, e);
        }
        Ok(v) => DirHandler(v),
    }
}

struct DirHandler(PathBuf);

impl Handler for DirHandler {
    fn call<'a>(&'a self, req: Request<Body>) -> Pin<Box<dyn Future<Output = Reply> + Send + 'a>> {
        let params = req.path_params();

        if params.is_empty() {
            let msg = "serve_dir() must be used with a path param. Example: /dir/*file";

            warn!("{}", msg);

            Box::pin(async move { err(500, msg).into() })
        } else if params.len() > 1 {
            let msg = "serve_dir() should be used with one path param. Example: /dir/*file";

            warn!("{}", msg);

            Box::pin(async move { err(500, msg).into() })
        } else {
            let to_serve = params[0].1.to_owned();

            Box::pin(async move {
                let ret = self.handle(to_serve, &req).await;
                ret.into()
            })
        }
    }
}

fn err(status: u16, msg: &str) -> http::Response<Body> {
    http::Response::builder()
        .status(status)
        .body(msg.into())
        .unwrap()
}

impl DirHandler {
    async fn handle(
        &self,
        to_serve: String,
        req: &Request<Body>,
    ) -> Result<http::Response<Body>, Error> {
        // Use the segment from the /*name appended to the dir we use.
        // This could be relative such as `"/path/to/serve"` + `"blah/../foo.txt"`
        let mut relative = self.0.clone();
        relative.push(&to_serve);

        // By canonicalizing we remove any `..`.
        let mut absolute = match relative.canonicalize() {
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    return Ok(err(404, "Not found"));
                } else {
                    warn!("Failed to canonicalize ({}): {:?}", to_serve, e);
                    return Ok(err(400, "Bad request"));
                }
            }
            Ok(v) => v,
        };

        // This is a security check that the resolved doesn't go to a parent dir/file.
        // "/path/to/serve" + "../../../etc/passwd". It works because self.0 is canonicalized.
        if !absolute.starts_with(&self.0) {
            debug!("Path not under base path: {}", to_serve);
            return Ok(err(404, "Bad path"));
        }

        // TODO configurable index files.
        if absolute.is_dir() {
            absolute.push("index.html");
        }

        let d = Dispatch::new(absolute, req);

        Ok(d.into_response().await?)
    }
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
                io::ErrorKind::NotFound => Ok(err(404, "File not found")),
                io::ErrorKind::PermissionDenied => Ok(err(403, "File permission denied")),
                _ => Err(e.into()),
            },
        }
    }

    async fn into_response_io(self) -> io::Result<http::Response<Body>> {
        if let Some(since) = self.if_modified_since {
            if let Ok(modified) = self.file.metadata().and_then(|v| v.modified()) {
                // for files that updated, since will be earlier than modified.
                if let Ok(diff) = modified.duration_since(since) {
                    // The web format has a resultion of seconds: Fri, 15 May 2015 15:34:21 GMT
                    // So the diff must be less than a second.
                    if diff.as_secs_f32() < 1.0 {
                        return Ok(http::Response::builder()
                            .status(304)
                            // https://tools.ietf.org/html/rfc7232#section-4.1
                            //
                            // The server generating a 304 response MUST generate any of the
                            // following header fields that would have been sent in a 200 (OK)
                            // response to the same request: Cache-Control, Content-Location, Date,
                            // ETag, Expires, and Vary.
                            .header("cache-control", "must-revalidate")
                            .header("last-modified", fmt_http_date(modified))
                            .body(().into())
                            .unwrap());
                    }
                }
            }
        }

        let file = std::fs::File::open(&self.file)?;
        let meta = file.metadata()?;

        let length = meta.len();
        let modified = meta.modified()?;

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
        mut peek: Z,
        mut res: http::response::Builder,
    ) -> io::Result<(Body, http::response::Builder)> {
        let body = if self.is_head {
            res = res.header("content-length", length.to_string());

            Body::empty()
        } else if let Some((start, end)) = self.range {
            if end <= start || start >= length || end > length {
                debug!("Bad range {}-{} of {}", start, end, length);

                res = res.status(http::StatusCode::RANGE_NOT_SATISFIABLE);

                Body::empty()
            } else {
                debug!("Serve range {}-{}/{}", start, end, length);

                peek.seek(io::SeekFrom::Start(start)).await?;

                let sub = end - start;

                let limit = ContentLengthRead::new(peek, sub);

                res = res.status(http::StatusCode::PARTIAL_CONTENT).header(
                    "content-range",
                    format!("bytes {}-{}/{}", start, end - 1, length),
                );

                Body::from_async_read(limit, Some(sub))
            }
        } else {
            Body::from_async_read(peek, Some(length))
        };

        Ok((body, res))
    }
}
