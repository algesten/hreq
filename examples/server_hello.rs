use hreq::prelude::*;

fn main() {
    let mut server = Server::new();

    server.at("/").get(|_| async { "Hello, World!" });

    let (handl, addr) = server.listen(3000).block().expect("Failed to listen");

    println!("Listening to: {}", addr);
    println!("Open in a browser: http://127.0.0.1:{}/", addr.port());

    handl.keep_alive().block()
}
