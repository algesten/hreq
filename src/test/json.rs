use super::run_server;
use crate::prelude::*;
use crate::test_h1_h2;
use crate::Body;
use crate::Error;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct MyJsonStruct {
    number: u8,
}

test_h1_h2! {
    fn json_send() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let obj = MyJsonStruct {
                number: 42,
            };
            let req = bld
                .uri("/json_send")
                .with_json(&obj)?;
            let (server_req, _client_res, _client_bytes) = run_server(req, "Ok", |mut tide_req| {
                async move {
                    let json = tide_req.body_string().await.unwrap();
                    assert_eq!(json, r#"{"number":42}"#);
                    tide_req
                }
            })?;
            assert_eq!(server_req.header("content-type"), Some("application/json; charset=utf-8"));
            Ok(())
        }
    }

}

#[test]
fn json_recv() {
    let json = r#"{
        "number":42
    }"#;
    let mut body = Body::from_str(json);
    body.set_codec_pass();
    let obj: MyJsonStruct = body.read_to_json().block().unwrap();
    assert_eq!(format!("{:?}", obj), "MyJsonStruct { number: 42 }");
}
