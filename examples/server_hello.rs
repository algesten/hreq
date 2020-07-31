use hreq::prelude::*;

fn main() {
    // use std::sync::Once;
    // static START: Once = Once::new();
    // START.call_once(|| {
    //     let level = log::LevelFilter::Trace;
    //     pretty_env_logger::formatted_builder()
    //         .filter_level(log::LevelFilter::Trace)
    //         .filter_module("hreq", level)
    //         .target(env_logger::Target::Stdout)
    //         .init();
    // });
    // let rt = tokio_lib::runtime::Builder::new()
    //     .threaded_scheduler()
    //     .core_threads(8)
    //     .enable_all()
    //     .build().unwrap();
    // hreq::AsyncRuntime::TokioOwned(rt).make_default();
    // hreq::AsyncRuntime::AsyncStd.make_default();

    let mut server = Server::new();

    server.at("/").get(|_| async { "Hello, World!" });

    let (handl, addr) = server.listen(3000).block().expect("Failed to listen");

    println!("Listening to: {}", addr);

    handl.keep_alive().block()
}
