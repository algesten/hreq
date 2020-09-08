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
