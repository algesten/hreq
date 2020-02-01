use crate::prelude::*;
use crate::AsyncRuntime;
use crate::Body;
use crate::Error;
use async_std_lib::sync::channel;
use futures_util::future::FutureExt;
use futures_util::select;
use tide;

mod simplelog;

pub fn test_setup() {
    simplelog::set_logger();
    // We're using async-std for the tests because that's what tide uses.
    AsyncRuntime::set_default(AsyncRuntime::AsyncStd);
}

#[allow(clippy::type_complexity)]
pub fn run_server<Res>(
    req: http::Request<Body>,
    res: Res,
) -> Result<(tide::Request<()>, http::Response<()>, Vec<u8>), Error>
where
    Res: tide::IntoResponse + Clone + Sync + 'static,
{
    test_setup();
    AsyncRuntime::current().block_on(async {
        // channel where we "leak" the server request from tide
        let (txsreq, rxsreq) = channel(1);
        // channel where we shut down tide server using select! macro
        let (txend, rxend) = channel::<()>(1);

        let mut app = tide::new();
        app.at("/").all(move |req| {
            let txsreq = txsreq.clone();
            let resp = res.clone();
            async move {
                txsreq.send(req).await;
                resp
            }
        });

        // Run the server app in a select! that ends when we send the end signal.
        AsyncRuntime::current().spawn(async move {
            select! {
                a = app.listen("127.0.0.1:8080").fuse() => a.map_err(|e| Error::Io(e)),
                b = rxend.recv().fuse() => Ok(()),
            }
        });

        // Send request and wait for the client response.
        let client_res = req.send().await.expect("Send request");

        // Read out entire response bytes to a vec.
        let (parts, mut body) = client_res.into_parts();
        let bytes = body.read_to_vec().await?;
        let client_res = http::Response::from_parts(parts, ());

        // Wait for the server request to "leak out" of the server app.
        let server_req = rxsreq.recv().await.expect("Wait for server request");

        // Shut down the server.
        txend.send(()).await;

        Ok((server_req, client_res, bytes))
    })
}

#[test]
fn it_works() -> Result<(), Error> {
    let req = http::Request::builder()
        .uri("http://127.0.0.1:8080/")
        .query("x", "y")
        .header("x-foo", "bar")
        .body(().into())?;
    let (server_req, client_res, body) = run_server(req, "Hello there!")?;
    assert_eq!(server_req.uri(), "/?x=y");
    assert_eq!(client_res.status(), 200);
    assert_eq!(String::from_utf8_lossy(&body), "Hello there!");
    Ok(())
}
