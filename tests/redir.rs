use hreq::prelude::*;
use hreq::Error;

#[test]
fn code_302() -> Result<(), Error> {
    let mut server = Server::new();

    server.at("/path").all(|_: http::Request<Body>| async move {
        http::Response::builder()
            .status(302)
            .header("Location", "https://www.google.com/")
            .body(())
            .unwrap()
    });

    let (shut, addr) = server.listen(0).block()?;

    let uri = format!("http://127.0.0.1:{}/path", addr.port());

    let req = http::Request::get(&uri).body(())?;
    let res = req.send().block()?;

    assert_eq!(res.status_code(), 200);
    assert_eq!(
        res.header("content-type"),
        Some("text/html; charset=ISO-8859-1")
    );

    let bytes = res.into_body().read_to_vec().block()?;
    assert!(bytes.len() > 100);

    shut.shutdown().block();
    Ok(())
}
