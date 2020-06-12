use super::run_server;
use crate::prelude::*;
use crate::test_h1_h2;
use crate::Error;

test_h1_h2! {
    fn query_params() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/path")
                .query("x", "y")
                .body(().into())?;
            let (server_req, client_res, _client_bytes) = run_server(req, "Ok", |tide_req| {
                async move {
                    tide_req
                }
            })?;
            assert_eq!(client_res.status(), 200);
            assert_eq!(server_req.uri(), "/path?x=y");
            Ok(())
        }
    }

    fn query_params_doubled() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/path")
                .query("x", "y")
                .query("x", "y")
                .body(().into())?;
            let (server_req, client_res, _client_bytes) = run_server(req, "Ok", |tide_req| {
                async move {
                    tide_req
                }
            })?;
            assert_eq!(client_res.status(), 200);
            assert_eq!(server_req.uri(), "/path?x=y&x=y");
            Ok(())
        }
    }

    fn request_header() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/path")
                .header("x-foo", "bar")
                .body(().into())?;
            let (server_req, client_res, _client_bytes) = run_server(req, "Ok", |tide_req| {
                async move {
                    tide_req
                }
            })?;
            assert_eq!(client_res.status(), 200);
            assert_eq!(server_req.header("x-foo"), Some("bar"));
            Ok(())
        }
    }
}

#[test]
fn non_existing_host_name() {
    super::test_setup();
    let res = Request::get("https://tremendously-incorrect-host-name.com")
        .call()
        .block();
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.is_io());
}

#[test]
fn missing_scheme() {
    super::test_setup();
    let res = Request::get("why-no-scheme.com").call().block();
    assert!(res.is_err());
    let err = res.unwrap_err();
    println!("{:?}", err);
}

// #[test]
// fn post_307() {
//     super::test_setup();
//     let data = super::DataGenerator::new(10 * 1024 * 1024);
//     http::Request::post("http://localhost:3000/1")
//         // .force_http2(true)
//         .redirect_body_buffer(10 * 1024 * 1024)
//         .header("content-type", "application/json")
//         .send(crate::Body::from_sync_read(data, None))
//         // .send(r#"{"foo": 43}"#)
//         .block()
//         .unwrap();
// }
