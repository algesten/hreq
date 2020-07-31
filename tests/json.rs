use hreq::prelude::*;
use hreq::Error;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct MyJsonStruct {
    number: u8,
}

#[test]
fn json_send() -> Result<(), Error> {
    let mut server = Server::new();

    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(
                req.header("content-type"),
                Some("application/json; charset=utf-8")
            );

            let s = req.into_body().read_to_string().await.unwrap();
            assert_eq!(s, "{\"number\":42}");

            "ok"
        });

    let obj = MyJsonStruct { number: 42 };
    let req = http::Request::post("/path").with_json(&obj)?;

    let res = server.handle(req).block()?;
    assert_eq!(res.status(), 200);

    Ok(())
}

#[test]
fn json_recv() -> Result<(), Error> {
    let mut server = Server::new();

    server.at("/path").all(|_: http::Request<Body>| async move {
        http::Response::builder()
            .header("content-type", "application/json")
            .body("{\"number\":42}")
            .unwrap()
    });

    let req = http::Request::get("/path").body(())?;
    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);
    let obj: MyJsonStruct = res.into_body().read_to_json().block()?;

    assert_eq!(obj.number, 42);

    Ok(())
}
