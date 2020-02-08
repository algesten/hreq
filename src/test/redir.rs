use super::run_server;
use crate::prelude::*;
use crate::test_h1_h2;
use crate::Error;

test_h1_h2! {
    fn code_302() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/path")
                .body(().into())?;
            let resp = tide::Response::new(302)
                .set_header("Location", "https://www.google.com/");
            let (_server_req, client_res, client_bytes) = run_server(req, resp, |tide_req| {
                async move { tide_req }
            })?;
            assert_eq!(client_res.status_code(), 200);
            assert_eq!(client_res.header("content-type"), Some("text/html; charset=ISO-8859-1"));
            assert!(client_bytes.len() > 100);
            Ok(())
        }
    }
}
