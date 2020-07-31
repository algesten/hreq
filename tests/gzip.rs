use futures_util::io::AsyncReadExt;
use futures_util::io::BufReader;
use futures_util::io::Cursor;
use hreq::prelude::*;
use hreq::Error;

use async_compression::futures::bufread::GzipDecoder;

mod common;

#[test]
fn gzip_response() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    server.at("/path").all(|_: http::Request<Body>| async move {
        let data = b"Ok";
        let curs = Cursor::new(data);
        http::Response::builder()
            .header("content-encoding", "gzip")
            .body(Body::from_async_read(curs, None))
            .unwrap()
    });

    let req = http::Request::get("/path")
        .header("accept-encoding", "gzip")
        .body(())?;
    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    assert_eq!(res.header("content-encoding"), Some("gzip"));
    let v = res.into_body().read_to_vec().block()?;
    let s = String::from_utf8_lossy(&v);

    assert_eq!(s, "Ok");
    Ok(())
}

#[test]
fn gzip_response_no_decode() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    server.at("/path").all(|_: http::Request<Body>| async move {
        let data = b"Ok";
        let curs = Cursor::new(data);
        http::Response::builder()
            .header("content-encoding", "gzip")
            .body(Body::from_async_read(curs, None))
            .unwrap()
    });

    let req = http::Request::get("/path")
        .header("accept-encoding", "gzip")
        .content_decode(false)
        .body(())?;
    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    assert_eq!(res.header("content-encoding"), Some("gzip"));
    let vec = res.into_body().read_to_vec().block()?;

    let mut decoder = GzipDecoder::new(BufReader::new(Cursor::new(vec)));
    let mut s = String::new();
    decoder.read_to_string(&mut s).block()?;

    assert_eq!(s, "Ok");
    Ok(())
}

#[test]
fn gzip_request() -> Result<(), Error> {
    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("content-encoding"), Some("gzip"));
            let s = req.into_body().read_to_string().await?;
            assert_eq!(s, "request that is compressed");
            Ok::<_, Error>("Ok")
        });

    let req = http::Request::post("/path")
        .header("content-encoding", "gzip")
        .body("request that is compressed")?;

    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    Ok(())
}

#[test]
fn gzip_request_no_encode() -> Result<(), Error> {
    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            let req = req.content_decode(false);
            assert_eq!(req.header("content-encoding"), Some("gzip"));
            let s = req.into_body().read_to_string().await?;
            assert_eq!(s, "not compressed");
            Ok::<_, Error>("Ok")
        });

    let req = http::Request::post("/path")
        .header("content-encoding", "gzip")
        .content_encode(false)
        .body("not compressed")?;

    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    Ok(())
}
