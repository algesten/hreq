use hreq::prelude::*;
mod common;

#[test]
fn static_file_get() -> Result<(), hreq::Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/my/cert")
        .get(hreq::server::Static::file("tests/data/tls_cert.pem"));

    let (handle, addr) = server.listen(0).block()?;

    {
        let uri = format!("http://localhost:{}/my/cert", addr.port());
        let res = http::Request::get(uri).call().block()?;

        assert_eq!(res.status(), 200);
        assert_eq!(
            res.header("content-type"),
            Some("application/x-x509-ca-cert")
        );

        let s = res.into_body().read_to_string().block()?;
        assert_eq!(&s[0..10], "-----BEGIN");
    }

    handle.shutdown().block();
    Ok(())
}

#[test]
fn static_send_file() -> Result<(), hreq::Error> {
    common::setup_logger();

    let mut server = Server::new();
    server.at("/do/something").get(|req| async move {
        // do stuff
        hreq::server::Static::send_file(&req, "tests/data/tls_cert.pem").await
    });

    let (handle, addr) = server.listen(0).block()?;

    {
        let uri = format!("http://localhost:{}/do/something", addr.port());
        let res = http::Request::get(uri).call().block()?;

        assert_eq!(res.status(), 200);
        assert_eq!(
            res.header("content-type"),
            Some("application/x-x509-ca-cert")
        );

        let s = res.into_body().read_to_string().block()?;
        assert_eq!(&s[0..10], "-----BEGIN");
    }

    handle.shutdown().block();
    Ok(())
}
