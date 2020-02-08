use super::run_server;
use super::DataGenerator;
use crate::prelude::*;
use crate::test_h1_h2;
use crate::Error;
use futures_util::io::BufReader;

test_h1_h2! {
    fn res_body1kb_no_size() -> Result<(), Error> {
        |bld: http::request::Builder| {
            const AMOUNT: usize = 1024;
            let req = bld
                .uri("/get1kb")
                .body(().into())?;
            let data = DataGenerator::new(AMOUNT);
            let resp = tide::Response::with_reader(200, BufReader::new(data));
            let (server_req, client_res, client_bytes) = run_server(req, resp, |tide_req| {
                async { tide_req }
            })?;
            assert_eq!(client_res.status(), 200);
            assert_eq!(client_bytes.len(), AMOUNT);
            if server_req.version() != http::Version::HTTP_2 {
                assert_eq!(client_res.header("transfer-encoding"), Some("chunked"));
            }
            Ok(())
        }
    }

    fn res_body10mb_with_size() -> Result<(), Error> {
        |bld: http::request::Builder| {
            const AMOUNT: usize = 10 * 1024 * 1024;
            let req = bld
                .uri("/get10mb")
                .body(().into())?;
            let data = DataGenerator::new(AMOUNT);
            let resp = tide::Response::with_reader(200, BufReader::new(data))
                .set_header("content-length", AMOUNT.to_string());
            let (server_req, client_res, client_bytes) = run_server(req, resp, |tide_req| {
                async { tide_req }
            })?;
            assert_eq!(client_res.status(), 200);
            assert_eq!(client_bytes.len(), AMOUNT);
            if server_req.version() != http::Version::HTTP_2 {
                assert_eq!(client_res.header("transfer-encoding"), None);
            }
            Ok(())
        }
    }
}
