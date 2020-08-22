use hreq::prelude::*;
use hreq::Error;
use rustls::internal::pemfile;

mod common;

#[test]
fn tls_client_to_server() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/path")
        .all(|_: http::Request<Body>| async move { "ok" });

    let mut tls = rustls::ServerConfig::new(rustls::NoClientAuth::new());

    const CERT_PEM: &[u8] = include_bytes!("./tls_cert.pem");

    let mut cur = std::io::Cursor::new(CERT_PEM);

    let certs = pemfile::certs(&mut cur).expect("Read TLS cert");
    cur.set_position(0);
    let mut keys = pemfile::pkcs8_private_keys(&mut cur).expect("Read TLS key");
    let key = keys.pop().unwrap();

    tls.set_single_cert(certs, key).expect("Set TLS keys");

    let (handle, addr) = server.listen_tls(0, tls).block()?;

    hreq::AsyncRuntime::spawn(async move {
        handle.keep_alive().await;
    });

    let uri = format!("https://localhost:{}/path", addr.port());

    let res = http::Request::get(uri)
        .tls_disable_server_cert_verify(true)
        .call()
        .block()?;

    assert_eq!(res.status(), 200);

    Ok(())
}
