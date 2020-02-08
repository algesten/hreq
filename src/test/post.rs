use super::run_server;
use super::DataGenerator;
use crate::prelude::*;
use crate::test_h1_h2;
use crate::AsyncRead;
use crate::Body;
use crate::Error;
use futures_util::future::poll_fn;
use std::pin::Pin;

test_h1_h2! {
    fn sane_headers_with_size() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .method("POST")
                .uri("/path")
                .body("requesting".into())?;
            let (server_req, _client_res, _client_bytes) = run_server(req, "Ok", |tide_req| {
                async move {
                    tide_req
                }
            })?;
            assert_eq!(server_req.header("transfer-encoding"), None);
            assert_eq!(server_req.header("content-length"), Some("10"));
            assert_eq!(server_req.header("content-encoding"), None);
            Ok(())
        }
    }

    fn sane_headers_with_size0() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .method("POST")
                .uri("/path")
                .body("".into())?;
            let (server_req, _client_res, _client_bytes) = run_server(req, "Ok", |tide_req| {
                async move {
                    tide_req
                }
            })?;
            assert_eq!(server_req.header("transfer-encoding"), None);
            assert_eq!(server_req.header("content-length"), Some("0"));
            assert_eq!(server_req.header("content-encoding"), None);
            Ok(())
        }
    }

    fn sane_headers_no_size() -> Result<(), Error> {
        |bld: http::request::Builder| {
            const AMOUNT: usize = 1024;
            let data = DataGenerator::new(AMOUNT);
            let req = bld
                .method("POST")
                .uri("/body1kb")
                .body(Body::from_sync_read(data, None))?;
            let (server_req, _client_res, _client_bytes) = run_server(req, "Ok", |tide_req| {
                async move {
                    tide_req
                }
            })?;
            if server_req.version() != http::Version::HTTP_2 {
                assert_eq!(server_req.header("transfer-encoding"), Some("chunked"));
            }
            assert_eq!(server_req.header("content-length"), None);
            assert_eq!(server_req.header("content-encoding"), None);
            Ok(())
        }
    }

    fn sane_headers_with_content_enc() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .method("POST")
                .uri("/path")
                .header("content-encoding", "gzip")
                .body("requesting".into())?;
            let (server_req, _client_res, _client_bytes) = run_server(req, "Ok", |tide_req| {
                async move {
                    tide_req
                }
            })?;
            if server_req.version() != http::Version::HTTP_2 {
                assert_eq!(server_req.header("transfer-encoding"), Some("chunked"));
            }
            assert_eq!(server_req.header("content-length"), None);
            assert_eq!(server_req.header("content-encoding"), Some("gzip"));
            Ok(())
        }
    }

    fn req_body1kb_with_size() -> Result<(), Error> {
        |bld: http::request::Builder| {
            const AMOUNT: usize = 1024;
            let data = DataGenerator::new(AMOUNT);
            let req = bld
                .method("POST")
                .uri("/body1kb")
                .body(Body::from_sync_read(data, Some(AMOUNT as u64)))?;
            let (server_req, client_res, _client_bytes) = run_server(req, "Ok", |mut tide_req| {
                async {
                    let mut buf = [0_u8; 16_384];
                    loop {
                        let amount = poll_fn(|cx| Pin::new(&mut tide_req).poll_read(cx, &mut buf[..])).await.unwrap();
                        // TODO verify the output in buf
                        if amount == 0 {
                            break;
                        }
                    }
                    tide_req
                }
            })?;
            assert_eq!(client_res.status(), 200);
            if server_req.version() != http::Version::HTTP_2 {
                assert_eq!(server_req.header("transfer-encoding"), None);
            }
            Ok(())
        }
    }

    fn req_body10mb_no_size() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let data = DataGenerator::new(10 * 1024 * 1024);
            let req = bld
                .method("POST")
                .uri("/body10mb")
                .body(Body::from_sync_read(data, None))?;
            let (server_req, client_res, _client_bytes) = run_server(req, "Ok", |mut tide_req| {
                async {
                    let mut buf = [0_u8; 16_384];
                    loop {
                        let amount = poll_fn(|cx| Pin::new(&mut tide_req).poll_read(cx, &mut buf[..])).await.unwrap();
                        // TODO verify the output in buf
                        if amount == 0 {
                            break;
                        }
                    }
                    tide_req
                }
            })?;
            assert_eq!(client_res.status(), 200);
            if server_req.version() != http::Version::HTTP_2 {
                assert_eq!(server_req.header("transfer-encoding"), Some("chunked"));
            }
            Ok(())
        }
    }

    fn req_body10mb_with_size() -> Result<(), Error> {
        |bld: http::request::Builder| {
            const AMOUNT: usize = 10 * 1024 * 1024;
            let data = DataGenerator::new(AMOUNT);
            let req = bld
                .method("POST")
                .uri("/body10mb")
                .body(Body::from_sync_read(data, Some(AMOUNT as u64)))?;
            let (server_req, client_res, _client_bytes) = run_server(req, "Ok", |mut tide_req| {
                async {
                    let mut buf = [0_u8; 16_384];
                    loop {
                        let amount = poll_fn(|cx| Pin::new(&mut tide_req).poll_read(cx, &mut buf[..])).await.unwrap();
                        // TODO verify the output in buf
                        if amount == 0 {
                            break;
                        }
                    }
                    tide_req
                }
            })?;
            assert_eq!(client_res.status(), 200);
            if server_req.version() != http::Version::HTTP_2 {
                assert_eq!(server_req.header("transfer-encoding"), None);
            }
            Ok(())
        }
    }
}
