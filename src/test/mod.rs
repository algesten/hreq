use crate::prelude::*;
use crate::AsyncRead;
use crate::AsyncRuntime;
use crate::Body;
use crate::Error;
use async_std_lib::sync::channel;
use futures_util::future::FutureExt;
use futures_util::select;
use rand::Rng;
use std::future::Future;
use std::io;
use std::net;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use tide;

mod basic;
mod charset;
mod get;
mod gzip;
mod post;
mod redir;
mod simplelog;
mod timeout;

pub fn test_setup() {
    simplelog::set_logger();
    // We're using async-std for the tests because that's what tide uses.
    AsyncRuntime::set_default(AsyncRuntime::AsyncStd, None);
}

#[allow(clippy::type_complexity)]
pub fn run_server<'a, Res, Acc, Fut>(
    req: http::Request<Body>,
    res: Res,
    accept_req: Acc,
) -> Result<(http::Request<()>, http::Response<()>, Vec<u8>), Error>
where
    Res: tide::IntoResponse + 'static,
    Acc: (FnOnce(tide::Request<()>) -> Fut) + Send + 'static,
    Fut: Future<Output = tide::Request<()>> + Send + 'a,
{
    test_setup();
    AsyncRuntime::current().block_on(async {
        // channel where we "leak" the server request from tide
        let (txsreq, rxsreq) = channel(1);
        // channel where send a message when we know the server is up
        let (txstart, rxstart) = channel(1);
        // channel where we shut down tide server using select! macro
        let (txend, rxend) = channel::<()>(1);

        let accept_req_once = Mutex::new(Some(accept_req));
        let res_mut = Mutex::new(Some(res));

        let mut app = tide::new();
        app.at("/*path").all(move |req: tide::Request<()>| {
            let txsreq = txsreq.clone();
            let accept_req = accept_req_once.lock().unwrap().take().unwrap();
            let res = res_mut.lock().unwrap().take();
            async move {
                let req = accept_req(req).await;
                txsreq.send(req).await;
                if let Some(res) = res {
                    res
                } else {
                    panic!("Already used up response");
                }
            }
        });

        let (hostport, test_uri) = random_test_uri(req.uri());

        // Rewrite the incoming request to use the port.
        let (mut parts, body) = req.into_parts();
        parts.uri = test_uri;
        let req = http::Request::from_parts(parts, body);

        // Run the server app in a select! that ends when we send the end signal.
        {
            let hostport = hostport.clone();
            AsyncRuntime::current().spawn(async move {
                let req = select! {
                    a = app.listen(&hostport).fuse() => a.map_err(|e| Error::Io(e)),
                    b = rxend.recv().fuse() => Ok(()),
                };
                req.expect("Error in app.listen()");
            });
        }

        // loop until a tcp connection can connect to the server
        AsyncRuntime::current().spawn(async move {
            let ret = loop {
                match AsyncRuntime::current().connect_tcp(&hostport).await {
                    Ok(_) => break Ok(()),
                    Err(e) => match e.into_io() {
                        Some(ioe) => match ioe.kind() {
                            io::ErrorKind::ConnectionRefused => continue,
                            _ => break Err(ioe),
                        },
                        None => panic!("Unexpected error in connect_tcp"),
                    },
                }
            };
            txstart.send(ret).await;
        });

        // wait until we know the server accepts requests
        rxstart.recv().await.expect("rxstart.recv()")?;

        // Send request and wait for the client response.
        let client_res = req.send().await?;

        // Wait for the server request to "leak out" of the server app.
        let tide_server_req = rxsreq.recv().await.expect("Wait for server request");

        // Normalize client response
        let (parts, mut body) = client_res.into_parts();
        // Read out entire response bytes to a vec.
        let client_bytes = body.read_to_vec().await?;
        let client_res = http::Response::from_parts(parts, ());

        // Shut down the server.
        txend.send(()).await;

        let server_req = normalize_tide_request(tide_server_req);

        Ok((server_req, client_res, client_bytes))
    })
}

/// Generate a random hos:port and uri pair
fn random_test_uri(uri: &http::Uri) -> (String, http::Uri) {
    // TODO There's no guarantee this port will be free by the time we do app.listen()
    // this could lead to random test failures. If tide provided some way of binding :0
    // and returning the port bound would be the best.
    let port = random_local_port();
    let hostport = format!("127.0.0.1:{}", port);
    let pq = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .expect("Test bad pq");

    // This is the request uri for the test
    let test_uri_s = format!("http://{}{}", hostport, pq);
    let test_uri = test_uri_s
        .as_str()
        .parse::<http::Uri>()
        .expect("Test bad req uri");

    (hostport, test_uri)
}

/// there's no guarantee this port wil be available when we want to (re-)use it
fn random_local_port() -> u16 {
    let mut n = 0;
    loop {
        n += 1;
        if n > 100 {
            panic!("Failed to allocate port after 100 retries");
        }
        let socket = net::SocketAddrV4::new(net::Ipv4Addr::LOCALHOST, 0);
        let port = net::TcpListener::bind(socket)
            .and_then(|listener| listener.local_addr())
            .and_then(|addr| Ok(addr.port()))
            .ok();
        if let Some(port) = port {
            break port;
        }
    }
}

/// Normalize tide request to a http::Request<()>
fn normalize_tide_request(tide_req: tide::Request<()>) -> http::Request<()> {
    let pq = tide_req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .expect("Test bad tide pq");
    let mut server_req = http::Request::builder()
        .version(match tide_req.version() {
            tide::http::Version::HTTP_09 => http::Version::HTTP_09,
            tide::http::Version::HTTP_10 => http::Version::HTTP_10,
            tide::http::Version::HTTP_11 => http::Version::HTTP_11,
            tide::http::Version::HTTP_2 => http::Version::HTTP_2,
        })
        .method(tide_req.method().as_str())
        .uri(pq);
    for (k, v) in tide_req.headers().clone().into_iter() {
        if let (Some(k), Some(v)) = (k, v.to_str().ok()) {
            server_req = server_req.header(k.as_str(), v);
        }
    }
    server_req.body(()).expect("Normalize tide req")
}

#[macro_export]
macro_rules! test_h1_h2 {
    (fn $name:ident () -> $ret:ty { $($body:tt)* } $($rest:tt)*) => {
        paste::item! {
            #[test]
            fn [<$name _h1>]() -> $ret {
                let bld = http::Request::builder();
                let close = $($body)*;
                (close)(bld)
            }
            #[test]
            fn [<$name _h2>]() -> $ret {
                let bld = http::Request::builder().force_http2(true);
                let close = $($body)*;
                (close)(bld)
            }
        }
        test_h1_h2!($($rest)*);
    };
    () => ()
}

#[derive(Debug)]
pub struct DataGenerator {
    total: usize,
    produced: usize,
}

impl DataGenerator {
    fn new(total: usize) -> Self {
        DataGenerator { total, produced: 0 }
    }
}

use std::io::Read;

impl io::Read for DataGenerator {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut rng = rand::thread_rng();
        let max = buf.len().min(self.total - self.produced);
        rng.fill(&mut buf[0..max]);
        self.produced += max;
        Ok(max)
    }
}

impl AsyncRead for DataGenerator {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let amount = this.read(buf)?;
        Ok(amount).into()
    }
}
