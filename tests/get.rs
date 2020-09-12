use hreq::prelude::*;
use hreq::Error;
use std::io;

mod common;

#[test]
fn sane_headers() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("transfer-encoding"), None);
            // TODO should we really have content-length 0 here?
            assert_eq!(req.header_as("content-length"), Some(0));
            assert_eq!(req.header("content-encoding"), None);
            assert_eq!(
                req.header("user-agent").map(|s| s.to_string()),
                Some(format!("rust/hreq/{}", hreq::VERSION))
            );
            assert_eq!(req.header("accept"), Some("*/*"));
            assert_eq!(req.version(), http::Version::HTTP_11);
            let x = req.header("host").unwrap();
            let re = regex::Regex::new("127.0.0.1:\\d+").unwrap();
            assert!(re.is_match(x));
            "ok"
        });

    let (shut, addr) = server.listen(0).block()?;

    let uri = format!("http://127.0.0.1:{}/path", addr.port());

    let req = http::Request::post(&uri).body(())?;
    let res = req.send().block()?;

    assert_eq!(res.status(), 200);
    shut.shutdown().block();
    Ok(())
}

#[test]
fn res_body1kb_no_size_prebuf() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    server.at("/path").all(|_: http::Request<Body>| async move {
        // length None provokes a chunked transfer-encoding
        Body::from_sync_read(io::Cursor::new(vec![42; 1024]), None)
    });

    let req = http::Request::post("/path").body("")?;
    let res = server.handle(req).block()?;
    assert_eq!(res.status(), 200);
    assert_eq!(res.header("transfer-encoding"), None);
    assert_eq!(res.header("content-length"), Some("1024"));

    let bytes = res.into_body().read_to_vec().block()?;
    assert_eq!(bytes.len(), 1024);

    Ok(())
}

#[test]
fn res_body1kb_no_size_no_prebuf() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    server.at("/path").all(|_: http::Request<Body>| async move {
        // length None provokes a chunked transfer-encoding
        Response::builder()
            .prebuffer_response_body(false)
            .body(Body::from_sync_read(io::Cursor::new(vec![42; 1024]), None))
            .unwrap()
    });

    let req = http::Request::post("/path").body("")?;
    let res = server.handle(req).block()?;
    assert_eq!(res.status(), 200);
    assert_eq!(res.header("transfer-encoding"), Some("chunked"));
    assert_eq!(res.header("content-length"), None);

    let bytes = res.into_body().read_to_vec().block()?;
    assert_eq!(bytes.len(), 1024);

    Ok(())
}

#[test]
fn res_body10mb_with_size() -> Result<(), Error> {
    common::setup_logger();

    const AMOUNT: usize = 10 * 1024 * 1024;
    let mut server = Server::new();

    server.at("/path").all(|_: http::Request<Body>| async move {
        // will set content-length header
        vec![42_u8; AMOUNT]
    });

    let (shut, addr) = server.listen(0).block()?;

    let uri = format!("http://127.0.0.1:{}/path", addr.port());

    let req = http::Request::post(&uri).body("")?;
    let res = req.send().block()?;
    assert_eq!(res.status(), 200);
    assert_eq!(res.header("transfer-encoding"), None);

    let bytes = res.into_body().read_to_vec().block()?;
    assert_eq!(bytes.len(), AMOUNT);

    shut.shutdown().block();
    Ok(())
}
