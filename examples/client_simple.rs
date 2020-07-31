use hreq::prelude::*;

fn main() {
    let response = http::Request::get("https://www.google.com/")
        .call()
        .block()
        .expect("Failed to call");

    println!("Content-Type: {:?}", response.header("content-type"));

    let body = response
        .into_body()
        .read_to_string()
        .block()
        .expect("Failed to read body");

    println!("{}", body);
}
