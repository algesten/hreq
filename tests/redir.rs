use hreq::prelude::*;
use hreq::Error;

mod common;

#[test]
fn code_302() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    server
        .at("/path1")
        .all(|_: http::Request<Body>| async move {
            http::Response::builder()
                .status(302)
                .header("Location", "/path2")
                .body(())
                .unwrap()
        });

    server
        .at("/path2")
        .all(|_: http::Request<Body>| async move { http::Response::builder().body("OK").unwrap() });

    let (shut, addr) = server.listen(0).block()?;

    let uri = format!("http://127.0.0.1:{}/path1", addr.port());

    let req = http::Request::get(&uri).body(())?;
    let res = req.send().block()?;

    assert_eq!(res.status_code(), 200);
    let body = res.into_body().read_to_string().block()?;
    assert_eq!(body, "OK");

    shut.shutdown().block();
    Ok(())
}
