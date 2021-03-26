mod common;

#[test]
#[cfg(feature = "tls")]
fn tls_client_to_server() -> Result<(), hreq::Error> {
    use hreq::prelude::*;

    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/path")
        .all(|_: http::Request<Body>| async move { "ok" });

    let config = hreq::server::TlsConfig::new()
        .key_path("tests/data/tls_cert.pem")
        .cert_path("tests/data/tls_cert.pem");

    let (handle, addr) = server.listen_tls(0, config).block()?;

    let uri = format!("https://localhost:{}/path", addr.port());

    let res = http::Request::get(uri)
        .tls_disable_server_cert_verify(true)
        .call()
        .block()?;

    assert_eq!(res.status(), 200);

    handle.shutdown().block();
    Ok(())
}

#[test]
#[cfg(feature = "tls")]
fn tls_req_body100mb_with_size() -> Result<(), hreq::Error> {
    use hreq::prelude::*;
    use std::io;

    common::setup_logger();

    const SIZE: u64 = 100 * 1024 * 1024;
    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("transfer-encoding"), None);
            assert_eq!(req.header_as("content-length"), Some(SIZE));
            assert_eq!(req.header("content-encoding"), None);
            assert_eq!(req.header("content-type"), Some("application/octet-stream"));

            let v = req.into_body().read_to_vec(SIZE as usize).await.unwrap();
            assert_eq!(v.len(), SIZE as usize);

            "ok"
        });

    let config = hreq::server::TlsConfig::new()
        .key_path("tests/data/tls_cert.pem")
        .cert_path("tests/data/tls_cert.pem");

    let (handle, addr) = server.listen_tls(0, config).block()?;

    let uri = format!("https://localhost:{}/path", addr.port());

    let curs = io::Cursor::new(vec![42; SIZE as usize]);

    let req = http::Request::post(uri)
        .tls_disable_server_cert_verify(true)
        .body(Body::from_sync_read(curs, Some(SIZE)))?;

    req.send().block()?;

    handle.shutdown().block();
    Ok(())
}
