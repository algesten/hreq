use hreq::prelude::*;

#[tokio::main]
async fn main() {
    let mut server = Server::new();

    server.at("/").get(|_| async { "Hello, World!" });

    let (handle, addr) = server.listen(3000).await.expect("Failed to listen");

    println!("Listening to: {}", addr);
    println!("Open in a browser: http://127.0.0.1:{}/", addr.port());

    // never ends
    handle.keep_alive().await
}
