use hreq::prelude::*;

#[tokio::main]
async fn main() {
    let response = http::Request::get("https://c64games.de")
        .call()
        .await
        .expect("Failed to call");

    println!("Content-Type: {:?}", response.header("content-type"));

    let body = response
        .into_body()
        .read_to_string()
        .await
        .expect("Failed to read body");

    println!("{}", body);
}
