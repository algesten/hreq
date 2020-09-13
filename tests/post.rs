use hreq::prelude::*;
use hreq::Error;
use std::io;

mod common;

#[test]
fn sane_headers_with_size10() -> Result<(), Error> {
    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("transfer-encoding"), None);
            assert_eq!(req.header_as("content-length"), Some(10));
            assert_eq!(req.header("content-encoding"), None);
            let v = req.into_body().read_to_vec().await.unwrap();
            assert_eq!(v.len(), 10);
            "ok"
        });

    let req = http::Request::post("/path").body("abcdefghij")?;
    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    Ok(())
}

#[test]
fn sane_headers_with_size0() -> Result<(), Error> {
    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("transfer-encoding"), None);
            assert_eq!(req.header_as("content-length"), Some(0));
            assert_eq!(req.header("content-encoding"), None);
            let v = req.into_body().read_to_vec().await.unwrap();
            assert_eq!(v.len(), 0);
            "ok"
        });

    let req = http::Request::post("/path").body("")?;
    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    Ok(())
}

#[test]
fn sane_headers_with_no_size() -> Result<(), Error> {
    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("transfer-encoding"), None);
            assert_eq!(req.header("content-length"), Some("10"));
            assert_eq!(req.header("content-encoding"), None);
            let v = req.into_body().read_to_vec().await.unwrap();
            assert_eq!(v.len(), 10);
            "ok"
        });

    let curs = io::Cursor::new(vec![42; 10]);

    let req = http::Request::post("/path").body(Body::from_sync_read(curs, None))?;
    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    Ok(())
}

#[test]
#[cfg(feature = "gzip")]
fn sane_headers_with_content_enc() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            println!("{:?}", req);

            assert_eq!(req.header("transfer-encoding"), None);
            assert_eq!(req.header("content-length"), Some("23"));
            assert_eq!(req.header("content-encoding"), Some("gzip"));
            let v = req.into_body().read_to_vec().await.unwrap();
            assert_eq!(v.len(), 3);
            "ok"
        });

    // gzip triggers transfer-encoding chunked. without gzip support,
    // we will send content-length instead.
    let req = http::Request::post("/path")
        .header("content-encoding", "gzip")
        .body("abc")?;
    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    Ok(())
}

#[test]
fn req_body1kb_with_size() -> Result<(), Error> {
    const SIZE: u64 = 1024;
    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("transfer-encoding"), None);
            assert_eq!(req.header_as("content-length"), Some(SIZE));
            assert_eq!(req.header("content-encoding"), None);
            let v = req.into_body().read_to_vec().await.unwrap();
            assert_eq!(v.len(), SIZE as usize);
            "ok"
        });

    let curs = io::Cursor::new(vec![42; SIZE as usize]);

    let req = http::Request::post("/path").body(Body::from_sync_read(curs, Some(SIZE)))?;
    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    Ok(())
}

#[test]
fn req_body100mb_no_size() -> Result<(), Error> {
    common::setup_logger();

    const SIZE: u64 = 100 * 1024 * 1024;
    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("transfer-encoding"), Some("chunked"));
            assert_eq!(req.header("content-length"), None);
            assert_eq!(req.header("content-encoding"), None);

            let v = req.into_body().read_to_vec().await.unwrap();
            assert_eq!(v.len(), SIZE as usize);

            "ok"
        });

    let (handle, addr) = server.listen(0).block()?;

    let uri = format!("http://localhost:{}/path", addr.port());

    let curs = io::Cursor::new(vec![42; SIZE as usize]);

    let req = http::Request::post(uri)
        // .force_http2(true)
        .body(Body::from_sync_read(curs, None))?;

    let res = req.send().block()?;

    assert_eq!(res.status(), 200);

    handle.shutdown().block();
    Ok(())
}

#[test]
fn req_body100mb_with_size() -> Result<(), Error> {
    const SIZE: u64 = 100 * 1024 * 1024;
    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("transfer-encoding"), None);
            assert_eq!(req.header_as("content-length"), Some(SIZE));
            assert_eq!(req.header("content-encoding"), None);
            let v = req.into_body().read_to_vec().await.unwrap();
            assert_eq!(v.len(), SIZE as usize);
            "ok"
        });

    let curs = io::Cursor::new(vec![42; SIZE as usize]);

    let req = http::Request::post("/path").body(Body::from_sync_read(curs, Some(SIZE)))?;
    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    Ok(())
}
