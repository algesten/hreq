use hreq::prelude::*;
use hreq::Agent;
use hreq::Error;

mod common;

#[test]
fn cookie_simple() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    let mut agent = Agent::new();

    server
        .at("/path1")
        .all(|_: http::Request<Body>| async move {
            http::Response::builder()
                .header("set-cookie", "Foo=Bar%20Baz; HttpOnly")
                .body("Ok1")
                .unwrap()
        });

    server
        .at("/path2")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("cookie"), Some("Foo=Bar%20Baz"));
            "Ok2"
        });

    let (shut, addr) = server.listen(0).block()?;

    let uri1: http::Uri = "https://some.host.com/path1".parse().unwrap();
    let uri2: http::Uri = "https://some.host.com/path2".parse().unwrap();

    let req1 = http::Request::get(&uri1)
        .with_override(&addr.ip().to_string(), addr.port(), false)
        .body(())?;

    let res1 = agent.send(req1).block()?;
    assert_eq!(res1.status(), 200);

    // check cookies set in /path1 are indeed in agent
    let cookies = agent.get_cookies(&uri1);
    assert!(cookies.len() == 1);
    let cookie = cookies[0];
    assert_eq!(cookie.name(), "Foo");
    assert_eq!(cookie.value(), "Bar Baz");

    let req2 = http::Request::builder()
        .uri(&uri2)
        .with_override(&addr.ip().to_string(), addr.port(), false)
        .body(())?;

    let res2 = agent.send(req2).block()?;
    assert_eq!(res2.status(), 200);

    shut.shutdown().block();
    Ok(())
}

#[test]
fn cookie_for_another_domain() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    let mut agent = Agent::new();

    server
        .at("/path1")
        .all(|_: http::Request<Body>| async move {
            http::Response::builder()
                .header("set-cookie", "Foo=Bar%20Baz; HttpOnly")
                .body("Ok1")
                .unwrap()
        });

    server
        .at("/path2")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("cookie"), None);
            "Ok2"
        });

    let (shut, addr) = server.listen(0).block()?;

    let uri1: http::Uri = "https://some.host.com/path1".parse().unwrap();
    // domain mismatch from uri1, no cookies should be sent by agent
    let uri2: http::Uri = "https://another_domain.com/path2".parse().unwrap();

    let req1 = http::Request::get(&uri1)
        .with_override(&addr.ip().to_string(), addr.port(), false)
        .body(())?;

    let res1 = agent.send(req1).block()?;
    assert_eq!(res1.status(), 200);

    let req2 = http::Request::builder()
        .uri(&uri2)
        .with_override(&addr.ip().to_string(), addr.port(), false)
        .body(())?;

    let res2 = agent.send(req2).block()?;
    assert_eq!(res2.status(), 200);

    shut.shutdown().block();
    Ok(())
}

#[test]
fn cookie_with_shared_domain() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    let mut agent = Agent::new();

    server
        .at("/path1")
        .all(|_: http::Request<Body>| async move {
            http::Response::builder()
                .header("set-cookie", "Foo=Bar%20Baz; HttpOnly; Domain=host.com")
                .body("Ok1")
                .unwrap()
        });

    server
        .at("/path2")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("cookie"), Some("Foo=Bar%20Baz"));
            "Ok2"
        });

    let (shut, addr) = server.listen(0).block()?;

    let uri1: http::Uri = "https://some.host.com/path1".parse().unwrap();
    // shared "host.com" and cookie domain is set
    let uri2: http::Uri = "https://another.host.com/path2".parse().unwrap();

    let req1 = http::Request::get(&uri1)
        .with_override(&addr.ip().to_string(), addr.port(), false)
        .body(())?;

    let res1 = agent.send(req1).block()?;
    assert_eq!(res1.status(), 200);

    let req2 = http::Request::builder()
        .uri(&uri2)
        .with_override(&addr.ip().to_string(), addr.port(), false)
        .body(())?;

    let res2 = agent.send(req2).block()?;
    assert_eq!(res2.status(), 200);

    shut.shutdown().block();
    Ok(())
}

#[test]
fn cookie_with_different_path() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    let mut agent = Agent::new();

    server
        .at("/cookie/path1")
        .all(|_: http::Request<Body>| async move {
            http::Response::builder()
                .header("set-cookie", "Foo=Bar%20Baz; HttpOnly; Path=/cookie/")
                .body("Ok1")
                .unwrap()
        });

    server
        .at("/cookie2/path2")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("cookie"), None);
            "Ok2"
        });

    let (shut, addr) = server.listen(0).block()?;

    let uri1: http::Uri = "https://some.host.com/cookie/path1".parse().unwrap();
    // path mismatch from uri1, no cookies should be sent by agent
    let uri2: http::Uri = "https://some.host.com/cookie2/path2".parse().unwrap();

    let req1 = http::Request::get(&uri1)
        .with_override(&addr.ip().to_string(), addr.port(), false)
        .body(())?;

    let res1 = agent.send(req1).block()?;
    assert_eq!(res1.status(), 200);

    let req2 = http::Request::builder()
        .uri(&uri2)
        .with_override(&addr.ip().to_string(), addr.port(), false)
        .body(())?;

    let res2 = agent.send(req2).block()?;
    assert_eq!(res2.status(), 200);

    shut.shutdown().block();
    Ok(())
}

#[test]
fn cookie_with_matching_path() -> Result<(), Error> {
    common::setup_logger();

    let mut server = Server::new();

    let mut agent = Agent::new();

    server
        .at("/cookie/path1")
        .all(|_: http::Request<Body>| async move {
            http::Response::builder()
                .header("set-cookie", "Foo=Bar%20Baz; HttpOnly; Path=/cookie/")
                .body("Ok1")
                .unwrap()
        });

    server
        .at("/cookie/path2")
        .all(|req: http::Request<Body>| async move {
            assert_eq!(req.header("cookie"), Some("Foo=Bar%20Baz"));
            "Ok2"
        });

    let (shut, addr) = server.listen(0).block()?;

    let uri1: http::Uri = "https://some.host.com/cookie/path1".parse().unwrap();
    // shared "/cookie" and cookie path is set
    let uri2: http::Uri = "https://some.host.com/cookie/path2".parse().unwrap();

    let req1 = http::Request::get(&uri1)
        .with_override(&addr.ip().to_string(), addr.port(), false)
        .body(())?;

    let res1 = agent.send(req1).block()?;
    assert_eq!(res1.status(), 200);

    let req2 = http::Request::builder()
        .uri(&uri2)
        .with_override(&addr.ip().to_string(), addr.port(), false)
        .body(())?;

    let res2 = agent.send(req2).block()?;
    assert_eq!(res2.status(), 200);

    shut.shutdown().block();
    Ok(())
}
