use futures_util::io::AsyncRead;
use hreq::prelude::*;
use hreq::Error;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

mod common;

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

#[test]
fn request_body_timeout() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    server
        .at("/path")
        .all(|_: http::Request<Body>| async move { "Ok" });

    let (shut, addr) = server.listen(0).block()?;

    let uri = format!("http://127.0.0.1:{}/path", addr.port());

    let res = http::Request::post(&uri)
        .timeout(Duration::from_millis(200))
        .send(Body::from_async_read(NeverRead, None))
        .block();

    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.is_io());
    assert_eq!(err.into_io().unwrap().kind(), io::ErrorKind::TimedOut);

    shut.shutdown().block();
    Ok(())
}

#[test]
fn response_body_timeout() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    server
        .at("/path")
        .all(|_: http::Request<Body>| async move { Body::from_async_read(NeverRead, None) });

    let (shut, addr) = server.listen(0).block()?;

    let uri = format!("http://127.0.0.1:{}/path", addr.port());

    let res = http::Request::get(&uri)
        .timeout(Duration::from_millis(1000))
        .call()
        .block();

    let res = res?;

    // we do get a response, but reading the body times out.
    assert_eq!(res.status(), 200);

    let r = res.into_body().read_to_vec().block();

    assert!(r.is_err());
    let err = r.unwrap_err();
    assert!(err.is_io());
    assert_eq!(err.into_io().unwrap().kind(), io::ErrorKind::TimedOut);

    shut.shutdown().block();
    Ok(())
}
