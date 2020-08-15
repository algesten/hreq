use hreq::prelude::*;
use hreq::Error;
use std::fs::File;
use std::io::BufReader;

mod common;

#[test]
fn from_charset_iso8859_ok() -> Result<(), Error> {
    common::setup_logger();

    let server = iso8859_server();

    let (shut, addr) = server.listen(0).block()?;

    let uri = format!("http://127.0.0.1:{}/path", addr.port());

    let mut agent = hreq::Agent::new();
    agent.retries(0);

    let req = http::Request::get(uri).body(())?;

    let res = agent.send(req).block()?;

    assert_eq!(
        res.header("content-type"),
        Some("text/plain; charset=iso8859-1")
    );

    let body = String::from_utf8(res.into_body().read_to_vec().block()?).unwrap();
    assert_eq!(body, "and in the river is an island. åiåaäeöÅIÅAÄEÖ.\n");

    shut.shutdown().block();
    Ok(())
}

#[test]
fn from_charset_iso8859_disable() -> Result<(), Error> {
    common::setup_logger();

    let server = iso8859_server();

    let (shut, addr) = server.listen(0).block()?;

    let uri = format!("http://127.0.0.1:{}/path", addr.port());

    let res = http::Request::get(uri)
        // turns off charset decoding
        .charset_decode(false)
        .call()
        .block()?;

    let vec = res.into_body().read_to_vec().block()?;

    // ÄEÖ.<lf>
    assert_eq!(&vec[42..], &[196, 69, 214, 46, 10]);

    shut.shutdown().block();
    Ok(())
}

#[test]
fn from_charset_shift_jis() -> Result<(), Error> {
    common::setup_logger();

    let server = shiftjis_server();

    let (shut, addr) = server.listen(0).block()?;

    let uri = format!("http://127.0.0.1:{}/path", addr.port());

    let res = http::Request::get(uri)
        .charset_decode_target("EUC-JP")
        .call()
        .block()?;

    assert_eq!(
        res.header("content-type"),
        Some("text/plain; charset=Shift_JIS")
    );

    let vec = res.into_body().read_to_vec().block()?;

    assert_eq!(
        vec,
        &[164_u8, 170, 164, 207, 164, 232, 164, 166, 192, 164, 179, 166, 10]
    );

    shut.shutdown().block();
    Ok(())
}

#[test]
fn to_charset_iso8859() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            let req = req.charset_decode_target("iso8859-1");
            let x = req.into_body().read_to_vec().await?;

            assert_eq!(x, vec![111, 99, 104, 32, 229, 105, 229, 97, 228, 101, 246]);

            Result::<_, Error>::Ok(())
        });

    let req = http::Request::post("/path")
        .header("content-type", "text/plain; charset=iso8859-1")
        .body("och åiåaäeö")?;

    server.handle(req).block()?;

    Ok(())
}

// おはよう世界 in EUC-JP.
const EUC_JP: &[u8] = &[
    164_u8, 170, 164, 207, 164, 232, 164, 166, 192, 164, 179, 166,
];

#[test]
fn charset_euc_jp_to_shift_jis() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            let req = req.charset_decode(false);

            let x = req.into_body().read_to_vec().await?;
            assert_eq!(
                x,
                vec![130, 168, 130, 205, 130, 230, 130, 164, 144, 162, 138, 69]
            );
            Result::<_, Error>::Ok(())
        });

    let (shut, addr) = server.listen(0).block()?;

    let uri = format!("http://127.0.0.1:{}/path", addr.port());

    let res = http::Request::post(uri)
        .charset_encode_source("EUC-JP")
        .header("content-type", "text/plain; charset=Shift_JIS")
        .send(Body::from_bytes(EUC_JP))
        .block()?;

    assert_eq!(res.status(), 200);

    shut.shutdown().block();
    Ok(())
}

#[test]
fn to_charset_shift_jis_disable() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            let req = req.charset_decode(false);

            let x = req.into_body().read_to_vec().await?;
            assert_eq!(x, EUC_JP);
            Result::<_, Error>::Ok(())
        });

    let req = http::Request::post("/path")
        .charset_encode_source("EUC-JP")
        .charset_encode(false)
        // should have no effect now
        .header("content-type", "text/plain; charset=Shift_JIS")
        .body(Body::from_bytes(EUC_JP))?;

    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);

    Ok(())
}

#[test]
fn to_charset_shift_jis_string() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/path")
        .all(|req: http::Request<Body>| async move {
            let req = req.charset_decode(false);

            let x = req.into_body().read_to_vec().await?;
            assert_eq!(
                x,
                vec![130, 168, 130, 205, 130, 230, 130, 164, 144, 162, 138, 69]
            );
            Result::<_, Error>::Ok(())
        });

    let req = http::Request::post("/path")
        // this should be ignored since the body is given as a UTF-8 string
        .charset_encode_source("EUC-JP")
        .header("content-type", "text/plain; charset=Shift_JIS")
        .body("おはよう世界")?;

    let res = server.handle(req).block()?;

    assert_eq!(res.status(), 200);

    Ok(())
}

fn iso8859_server() -> Server<()> {
    common::setup_logger();

    let mut server = Server::new();
    server.at("/path").all(|_: http::Request<Body>| async move {
        let file = File::open("./data/iso8859.txt").unwrap();
        let buf_reader = BufReader::new(file);
        http::Response::builder()
            .charset_encode_source("iso8859-1")
            .header("content-type", "text/plain; charset=iso8859-1")
            .body(Body::from_sync_read(buf_reader, None))
            .unwrap()
    });

    server
}

fn shiftjis_server() -> Server<()> {
    common::setup_logger();

    let mut server = Server::new();
    server.at("/path").all(|_: http::Request<Body>| async move {
        let file = File::open("./data/shiftjis.txt").unwrap();
        let buf_reader = BufReader::new(file);
        http::Response::builder()
            .charset_encode_source("Shift_JIS")
            .header("content-type", "text/plain; charset=Shift_JIS")
            .body(Body::from_sync_read(buf_reader, None))
            .unwrap()
    });

    server
}
