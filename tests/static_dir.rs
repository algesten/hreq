use hreq::prelude::*;
mod common;

#[test]
fn static_dir_ctype() -> Result<(), hreq::Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/my/special/:file")
        .get(hreq::server::Static::dir("tests/data"));

    let (handle, addr) = server.listen(0).block()?;

    hreq::AsyncRuntime::spawn(async move {
        handle.keep_alive().await;
    });

    {
        let uri = format!("http://localhost:{}/my/special/tls_cert.pem", addr.port());
        let res = http::Request::get(uri).call().block()?;

        assert_eq!(res.status(), 200);
        assert_eq!(
            res.header("content-type"),
            Some("application/x-x509-ca-cert")
        );

        let s = res.into_body().read_to_string().block()?;
        assert_eq!(&s[0..10], "-----BEGIN");
    }

    {
        let uri = format!("http://localhost:{}/my/special/iso8859.txt", addr.port());
        let res = http::Request::get(uri).call().block()?;

        assert_eq!(res.status(), 200);
        assert_eq!(
            res.header("content-type"),
            Some("text/plain; charset=windows-1252")
        );

        let s = res.into_body().read_to_string().block()?;
        assert_eq!(&s[0..10], "and in the");
    }

    {
        let uri = format!("http://localhost:{}/my/special/shiftjis.txt", addr.port());
        let res = http::Request::get(uri).call().block()?;

        assert_eq!(res.status(), 200);
        assert_eq!(
            res.header("content-type"),
            Some("text/plain; charset=Shift_JIS")
        );

        let s = res.into_body().read_to_string().block()?;
        assert_eq!(&s[0..12], "おはよう");
    }

    Ok(())
}

#[test]
fn static_dir_subdir() -> Result<(), hreq::Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/my/special/*path")
        .get(hreq::server::Static::dir("tests/data"));

    let (handle, addr) = server.listen(0).block()?;

    hreq::AsyncRuntime::spawn(async move {
        handle.keep_alive().await;
    });

    let uri = format!("http://localhost:{}/my/special/subdir/ok.txt", addr.port());
    let res = http::Request::get(uri).call().block()?;

    assert_eq!(res.status(), 200);
    assert_eq!(
        res.header("content-type"),
        Some("text/plain; charset=UTF-8")
    );

    let s = res.into_body().read_to_string().block()?;
    assert_eq!(&s[0..12], "It's alright");

    Ok(())
}

#[test]
fn static_dir_404() -> Result<(), hreq::Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/my/special/*path")
        .get(hreq::server::Static::dir("tests/data"));

    let (handle, addr) = server.listen(0).block()?;

    hreq::AsyncRuntime::spawn(async move {
        handle.keep_alive().await;
    });

    {
        let uri = format!("http://localhost:{}/my/special/not_there.txt", addr.port());
        let res = http::Request::get(uri).call().block()?;

        assert_eq!(res.status(), 404);
    }

    {
        let uri = format!("http://localhost:{}/my/special/subdir", addr.port());
        let res = http::Request::get(uri).call().block()?;

        assert_eq!(res.status(), 404);
    }

    {
        let uri = format!("http://localhost:{}/my/special/../get.rs", addr.port());
        let res = http::Request::get(uri).call().block()?;

        assert_eq!(res.status(), 404);
    }

    Ok(())
}

#[test]
fn static_dir_index() -> Result<(), hreq::Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/my/special/*path")
        .get(hreq::server::Static::dir("tests/data"));

    let (handle, addr) = server.listen(0).block()?;

    hreq::AsyncRuntime::spawn(async move {
        handle.keep_alive().await;
    });

    let uri = format!("http://localhost:{}/my/special/", addr.port());
    let res = http::Request::get(uri).call().block()?;

    assert_eq!(res.status(), 200);
    assert_eq!(res.header("content-type"), Some("text/html; charset=UTF-8"));

    // let uri = format!("http://localhost:{}/my/special", addr.port());
    // let res = http::Request::get(uri).call().block()?;

    // assert_eq!(res.status(), 200);

    Ok(())
}

#[test]
fn static_dir_no_index() -> Result<(), hreq::Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/my/special/*path")
        .get(hreq::server::Static::dir("tests/data").index_file(None));

    let (handle, addr) = server.listen(0).block()?;

    hreq::AsyncRuntime::spawn(async move {
        handle.keep_alive().await;
    });

    let uri = format!("http://localhost:{}/my/special/", addr.port());
    let res = http::Request::get(uri).call().block()?;

    assert_eq!(res.status(), 404);

    Ok(())
}

#[test]
fn static_dir_other_index() -> Result<(), hreq::Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/my/special/*path")
        .get(hreq::server::Static::dir("tests/data").index_file(Some("shiftjis.txt")));

    let (handle, addr) = server.listen(0).block()?;

    hreq::AsyncRuntime::spawn(async move {
        handle.keep_alive().await;
    });

    let uri = format!("http://localhost:{}/my/special/", addr.port());
    let res = http::Request::get(uri).call().block()?;

    assert_eq!(res.status(), 200);
    assert_eq!(
        res.header("content-type"),
        Some("text/plain; charset=Shift_JIS")
    );

    Ok(())
}

#[test]
fn static_dir_last_modified() -> Result<(), hreq::Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/my/special/*path")
        .get(hreq::server::Static::dir("tests/data"));

    let (handle, addr) = server.listen(0).block()?;

    hreq::AsyncRuntime::spawn(async move {
        handle.keep_alive().await;
    });

    let uri = format!("http://localhost:{}/my/special/index.html", addr.port());

    let res = http::Request::get(&uri).call().block()?;

    use std::fs::File;
    let file = File::open("tests/data/index.html")?;
    let meta = file.metadata()?;
    let modified = meta.modified()?;

    assert_eq!(res.status(), 200);
    let last_mod_head = res.header("last-modified").expect("last-modified header");
    let last_mod = httpdate::parse_http_date(last_mod_head).expect("parse last-modified");

    let dur = modified
        .duration_since(last_mod)
        .expect("duration_since")
        .as_secs_f32();

    assert!(dur < 1.0);

    {
        let res = http::Request::get(&uri)
            .header("if-modified-since", last_mod_head)
            .call()
            .block()?;

        assert_eq!(res.status(), 304);
        assert_eq!(res.header("content-length"), None);
        assert_eq!(res.header("content-type"), None);
    }

    {
        let res = http::Request::get(&uri)
            .header("if-modified-since", "Fri, 15 May 2015 15:34:21 GMT")
            .call()
            .block()?;

        assert_eq!(res.status(), 200);
        assert_eq!(res.header("content-length"), Some("87"));
        assert_eq!(res.header("content-type"), Some("text/html; charset=UTF-8"));
    }

    Ok(())
}

#[test]
fn static_dir_head() -> Result<(), hreq::Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/my/special/*path")
        .all(hreq::server::Static::dir("tests/data"));

    let (handle, addr) = server.listen(0).block()?;

    hreq::AsyncRuntime::spawn(async move {
        handle.keep_alive().await;
    });

    let uri = format!("http://localhost:{}/my/special/iso8859.txt", addr.port());
    let res = http::Request::head(uri).call().block()?;

    assert_eq!(res.status(), 200);
    assert_eq!(
        res.header("content-type"),
        Some("text/plain; charset=windows-1252")
    );

    assert_eq!(res.header("accept-ranges"), Some("bytes"));

    let len: u64 = res.header_as("content-length").unwrap();
    assert!(len > 0);

    let s = res.into_body().read_to_string().block()?;
    assert!(s.is_empty());

    Ok(())
}

#[test]
fn static_dir_range() -> Result<(), hreq::Error> {
    common::setup_logger();

    let mut server = Server::new();
    server
        .at("/my/special/*path")
        .get(hreq::server::Static::dir("tests/data"));

    let (handle, addr) = server.listen(0).block()?;

    hreq::AsyncRuntime::spawn(async move {
        handle.keep_alive().await;
    });

    {
        let uri = format!("http://localhost:{}/my/special/iso8859.txt", addr.port());
        let res = http::Request::get(uri)
            .header("range", "bytes=7-9")
            .call()
            .block()?;

        assert_eq!(res.status(), 206);
        assert_eq!(res.header("content-range"), Some("bytes 7-9/47"));
        assert_eq!(res.header("content-length"), Some("3"));

        let s = res.into_body().read_to_string().block()?;
        assert_eq!(s, "the");
    }

    {
        let uri = format!("http://localhost:{}/my/special/iso8859.txt", addr.port());
        let res = http::Request::get(uri)
            .header("range", "bytes=46-46") // in range, last byte
            .call()
            .block()?;

        assert_eq!(res.status(), 206);
        assert_eq!(res.header("content-range"), Some("bytes 46-46/47"));
        assert_eq!(res.header("content-length"), Some("1"));

        let s = res.into_body().read_to_string().block()?;
        assert_eq!(s, "\n");
    }

    {
        let uri = format!("http://localhost:{}/my/special/iso8859.txt", addr.port());
        let res = http::Request::get(uri)
            .header("range", "bytes=47-47") // out of range
            .call()
            .block()?;

        assert_eq!(res.status(), 416);

        let s = res.into_body().read_to_string().block()?;
        assert_eq!(s, "");
    }

    {
        let uri = format!("http://localhost:{}/my/special/iso8859.txt", addr.port());
        let res = http::Request::get(uri)
            .header("range", "bytes=46-47") // out of range
            .call()
            .block()?;

        assert_eq!(res.status(), 416);

        let s = res.into_body().read_to_string().block()?;
        assert_eq!(s, "");
    }

    {
        let uri = format!("http://localhost:{}/my/special/iso8859.txt", addr.port());
        let res = http::Request::get(uri)
            .header("range", "bytes=45-44") // incorrect range
            .call()
            .block()?;

        assert_eq!(res.status(), 416);

        let s = res.into_body().read_to_string().block()?;
        assert_eq!(s, "");
    }

    Ok(())
}
