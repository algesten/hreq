use super::run_server;
use crate::prelude::*;
use crate::test_h1_h2;
use crate::AsyncRead;
use crate::Body;
use crate::Error;
use futures_util::future::poll_fn;
use std::fs::File;
use std::io;
use std::pin::Pin;

#[derive(Debug)]
struct DataGenerator {
    rand: File,
    total: usize,
    produced: usize,
}

impl DataGenerator {
    fn new(total: usize) -> Self {
        DataGenerator {
            rand: File::open("/dev/random").unwrap(),
            total,
            produced: 0,
        }
    }
}

impl io::Read for DataGenerator {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let max = buf.len().min(self.total - self.produced);
        let amount = self.rand.read(&mut buf[..max])?;
        self.produced += amount;
        Ok(amount)
    }
}

test_h1_h2! {
    fn body1kb_with_size() -> Result<(), Error> {
        |bld: http::request::Builder| {
            const AMOUNT: usize = 1024;
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

    fn body10mb_no_size() -> Result<(), Error> {
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

    fn body10mb_with_size() -> Result<(), Error> {
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
