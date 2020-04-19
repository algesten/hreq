use super::run_server;
use crate::prelude::*;
use crate::test_h1_h2;
use crate::Error;
use async_std_lib::io::Cursor;
use futures_util::io::AsyncReadExt;
use futures_util::io::BufReader;

use async_compression::futures::bufread::{GzipDecoder, GzipEncoder};

test_h1_h2! {
    fn gzip_response() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/path")
                .header("accept-encoding", "gzip")
                .body(().into())?;
            let data = b"Ok";
            let curs = Cursor::new(data);
            // doesn't seem tide provides this functionality.
            let read = BufReader::new(GzipEncoder::new(BufReader::new(curs)));
            let resp = tide::Response::with_reader(200, read)
                .set_header("content-encoding", "gzip");
            let (_server_req, client_res, client_bytes) = run_server(req, resp, |tide_req| {
                async move {
                    tide_req
                }
            })?;
            assert_eq!(client_res.status(), 200);
            assert_eq!(client_res.header("content-encoding"), Some("gzip"));
            let body_s = String::from_utf8_lossy(&client_bytes);
            assert_eq!(body_s, "Ok");
            Ok(())
        }
    }

    fn gzip_response_no_decode() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/path")
                .header("accept-encoding", "gzip")
                .content_decode(false)
                .body(().into())?;
            let data = b"Ok";
            let curs = Cursor::new(data);
            // doesn't seem tide provides this functionality.
            let read = BufReader::new(GzipEncoder::new(BufReader::new(curs)));
            let resp = tide::Response::with_reader(200, read)
                .set_header("content-encoding", "gzip");
            let (_server_req, client_res, client_bytes) = run_server(req, resp, |tide_req| {
                async move {
                    tide_req
                }
            })?;
            assert_eq!(client_res.status(), 200);
            assert_eq!(client_res.header("content-encoding"), Some("gzip"));
            let curs = Cursor::new(client_bytes);
            let mut read = BufReader::new(GzipDecoder::new(BufReader::new(curs)));
            let mut body_s = String::new();
            read.read_to_string(&mut body_s).block().expect("read gzip decode");
            assert_eq!(body_s, "Ok");
            Ok(())
        }
    }

    fn gzip_request() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .method("POST")
                .uri("/path")
                .header("content-encoding", "gzip")
                .body("request that is compressed".into())?;
            let (server_req, client_res, _client_bytes) = run_server(req, "Ok", |mut tide_req| {
                async {
                    let bytes = tide_req.body_bytes().await.expect("read request body");
                    let curs = Cursor::new(bytes);
                    let mut read = BufReader::new(GzipDecoder::new(BufReader::new(curs)));
                    let mut body_s = String::new();
                    read.read_to_string(&mut body_s).await.expect("read gzip decode");
                    assert_eq!(body_s, "request that is compressed");
                    tide_req
                }
            })?;
            assert_eq!(client_res.status(), 200);
            assert_eq!(server_req.header("content-encoding"), Some("gzip"));
            if server_req.version() != http::Version::HTTP_2 {
                assert_eq!(server_req.header("transfer-encoding"), Some("chunked"));
            }
            Ok(())
        }
    }

    fn gzip_request_no_encode() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .method("POST")
                .uri("/path")
                .header("content-encoding", "gzip")
                .content_encode(false)
                .body("not compressed".into())?;
            let (server_req, client_res, _client_bytes) = run_server(req, "Ok", |mut tide_req| {
                async {
                    let bytes = tide_req.body_bytes().await.expect("read request body");
                    assert_eq!(bytes, b"not compressed");
                    tide_req
                }
            })?;
            assert_eq!(client_res.status(), 200);
            assert_eq!(server_req.header("content-encoding"), Some("gzip"));
            if server_req.version() != http::Version::HTTP_2 {
                assert_eq!(server_req.header("transfer-encoding"), None);
            }
            Ok(())
        }
    }
}
