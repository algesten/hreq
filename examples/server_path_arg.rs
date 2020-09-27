use hreq::prelude::*;

#[tokio::main]
async fn main() {
    let mut server = Server::new();

    server.at("/hello/:name").get(hello_there);

    let (shut, addr) = server.listen(0).await.expect("Failed to listen");

    println!("Listening to: {}", addr);

    let url = format!("http://127.0.0.1:{}/hello/Martin", addr.port());

    println!("Calling: {}", url);

    let response = http::Request::get(url)
        .call()
        .await
        .expect("Failed to call");

    println!("Response status: {}", response.status());

    let body = response
        .into_body()
        .read_to_string()
        .await
        .expect("Failed to read body");

    println!("Body:\n{}", body);

    shut.shutdown().await
}

async fn hello_there(req: http::Request<Body>) -> String {
    let name = req.path_param("name").unwrap();

    format!("Hello there {}!\n", name)
}
