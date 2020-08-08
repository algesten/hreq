use hreq::prelude::*;
use hreq::Error;

mod common;

#[test]
fn query_params() -> Result<(), Error> {
    common::setup_logger();

    let bld = http::Request::builder();
    let req = bld.uri("/path").query("x", "y").body(())?;

    let mut server = Server::new();
    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.uri(), "/path?x=y");
            "ok"
        });

    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    Ok(())
}

#[test]
fn query_params_doubled() -> Result<(), Error> {
    common::setup_logger();

    let bld = http::Request::builder();
    let req = bld.uri("/path").query("x", "y").query("x", "y").body(())?;

    let mut server = Server::new();
    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.uri(), "/path?x=y&x=y");
            "ok"
        });

    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    Ok(())
}

#[test]
fn request_header() -> Result<(), Error> {
    common::setup_logger();

    let bld = http::Request::builder();
    let req = bld.uri("/path").header("x-foo", "bar").body(())?;

    let mut server = Server::new();
    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("x-foo"), Some("bar"));
            "ok"
        });

    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    Ok(())
}

// #[test]
// fn non_existing_host_name() {
//     common::setup_logger();

//     let res = Request::get("https://tremendously-incorrect-host-name.com")
//         .call()
//         .block();

//     assert!(res.is_err());
//     let err = res.unwrap_err();

//     assert!(err.is_io());
// }

// #[test]
// fn missing_scheme() {
//     common::setup_logger();

//     // defaults to http
//     let res = Request::get("google.com").call().block();

//     assert!(res.is_ok());
// }

// xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

// #[test]
// fn post_307() -> Result<(), Error> {
//     use common::DataGenerator;
//     common::setup_logger();

//     let mut server = Server::new();

//     server.at("/path").all(|_: http::Request<Body>| async move {
//         println!("path1");
//         http::Response::builder()
//             .status(307)
//             .header("location", "/path2")
//             .body(())
//     });

//     server
//         .at("/path2")
//         .all(|req: http::Request<Body>| async move {
//             println!("path2");
//             let vec = req.into_body().read_to_vec().await?;
//             assert_eq!(vec.len(), 123);
//             Result::<_, Error>::Ok("ok")
//         });

//     let (shut, addr) = server.listen(0).block()?;

//     let uri = format!("http://127.0.0.1:{}/path", addr.port());

//     let data = DataGenerator::new(10 * 1024 * 1024);
//     let req = http::Request::post(uri)
//         // .force_http2(true)
//         .redirect_body_buffer(10 * 1024 * 1024)
//         .body(crate::Body::from_sync_read(data, None))?;

//     let res = req.send().block()?;

//     assert_eq!(res.into_body().read_to_string().block()?, "ok");

//     shut.shutdown().block();

//     Ok(())
// }
