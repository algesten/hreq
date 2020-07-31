use hreq::prelude::*;

fn main() {
    // use std::sync::Once;
    // static START: Once = Once::new();
    // START.call_once(|| {
    //     let level = log::LevelFilter::Info;
    //     pretty_env_logger::formatted_builder()
    //         .filter_level(log::LevelFilter::Warn)
    //         .filter_module("hreq", level)
    //         .target(env_logger::Target::Stdout)
    //         .init();
    // });

    let mut server = Server::new();

    server.at("/hello/:name").get(hello_there);

    let (shut, addr) = server.listen(0).block().expect("Failed to listen");

    println!("Listening to: {}", addr);

    let url = format!("http://127.0.0.1:{}/hello/Martin", addr.port());

    let response = http::Request::get(url)
        .call()
        .block()
        .expect("Failed to call");

    let body = response
        .into_body()
        .read_to_string()
        .block()
        .expect("Failed to read body");

    println!("Response body:\n{}", body);

    shut.shutdown().block();
}

async fn hello_there(req: http::Request<Body>) -> String {
    let name = req.path_param("name").unwrap();

    format!("Hello there {}!\n", name)
}

// struct NoFuture;

// impl std::future::Future for NoFuture {
//     type Output = ();
//     fn poll(
//         self: std::pin::Pin<&mut Self>,
//         _cx: &mut std::task::Context<'_>,
//     ) -> std::task::Poll<Self::Output> {
//         std::task::Poll::Pending
//     }
// }
