use super::run_server;
use crate::prelude::*;
use crate::test_h1_h2;
use crate::AsyncRead;
use crate::Body;
use crate::Error;
use futures_util::io::BufReader;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

struct NeverRead;

impl AsyncRead for NeverRead {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Pending
    }
}

test_h1_h2! {
    fn request_body() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/path")
                .timeout(Duration::from_millis(200))
                .body(Body::from_async_read(NeverRead, None))?;
            let res = run_server(req, "Ok", |tide_req| {
                async move {
                    tide_req
                }
            });
            assert!(res.is_err());
            let err = res.unwrap_err();
            assert!(err.is_io());
            assert_eq!(err.into_io().unwrap().kind(), io::ErrorKind::TimedOut);
            Ok(())
        }
    }

    fn response_body() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/path")
                .timeout(Duration::from_millis(200))
                .body(().into())?;
            let resp = tide::Response::with_reader(200, BufReader::new(NeverRead));
            let res = run_server(req, resp, |tide_req| {
                async move {
                    tide_req
                }
            });
            assert!(res.is_err());
            let err = res.unwrap_err();
            assert!(err.is_io());
            assert_eq!(err.into_io().unwrap().kind(), io::ErrorKind::TimedOut);
            Ok(())
        }
    }
}
