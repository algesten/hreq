use hreq::prelude::*;

#[tokio::main]
async fn main() {
    let mut server = Server::new();

    server
        .at("/*any")
        .all(|req: http::Request<Body>| async move {
            if let Some(v) = req.into_body().read_to_vec().await.ok() {
                format!("You sent: {} bytes\n", v.len())
            } else {
                "Nothing sent".into()
            }
        });

    let config = hreq::server::TlsConfig::new()
        .key_path("tests/data/tls_cert.pem")
        .cert_path("tests/data/tls_cert.pem");

    let (handle, addr) = server.listen_tls(3000, config).await.unwrap();

    println!("TLS listening to: {}", addr);
    println!("Try this: curl -k https://localhost:{}/ -d\"Sweet\"", addr.port());

    handle.keep_alive().await;
}
