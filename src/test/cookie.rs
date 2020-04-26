use super::run_agent;
use crate::prelude::*;
use crate::Agent;
use crate::Error;

#[test]
fn cookie_simple() -> Result<(), Error> {
    let uri: http::Uri = "https://some.host.com/cookie".parse().unwrap();
    let mut agent = Agent::new();

    let req1 = http::Request::builder().uri(&uri).body(().into())?;
    let resp1 = tide::Response::new(200)
        .body_string("Ok".to_string())
        .append_header("set-cookie", "Foo=Bar%20Baz; HttpOnly");
    let (_server_req, _client_res, _client_bytes) =
        run_agent(&mut agent, req1, resp1, |tide_req| async move { tide_req })?;

    let cookies = agent.get_cookies(&uri);
    assert!(cookies.len() == 1);
    let cookie = cookies[0];
    assert_eq!(cookie.name(), "Foo");
    assert_eq!(cookie.value(), "Bar Baz");

    let req2 = http::Request::builder().uri(&uri).body(().into())?;
    let (server_req, _client_res, _client_bytes) =
        run_agent(&mut agent, req2, "Ok", |tide_req| async move { tide_req })?;

    assert_eq!(server_req.header("cookie"), Some("Foo=Bar%20Baz"));

    Ok(())
}

#[test]
fn cookie_for_another_domain() -> Result<(), Error> {
    let mut agent = Agent::new();

    let uri1: http::Uri = "https://some.host.com/cookie".parse().unwrap();
    let req1 = http::Request::builder().uri(&uri1).body(().into())?;
    let resp1 = tide::Response::new(200)
        .body_string("Ok".to_string())
        .append_header("set-cookie", "Foo=Bar%20Baz; HttpOnly");
    let (_server_req, _client_res, _client_bytes) =
        run_agent(&mut agent, req1, resp1, |tide_req| async move { tide_req })?;

    let uri2: http::Uri = "https://another.host.com/cookie".parse().unwrap();
    let req2 = http::Request::builder().uri(&uri2).body(().into())?;
    let (server_req, _client_res, _client_bytes) =
        run_agent(&mut agent, req2, "Ok", |tide_req| async move { tide_req })?;

    // no cookie here!
    assert_eq!(server_req.header("cookie"), None);

    Ok(())
}

#[test]
fn cookie_with_domain() -> Result<(), Error> {
    let mut agent = Agent::new();

    let uri1: http::Uri = "https://some.host.com/cookie".parse().unwrap();
    let req1 = http::Request::builder().uri(&uri1).body(().into())?;
    let resp1 = tide::Response::new(200)
        .body_string("Ok".to_string())
        .append_header("set-cookie", "Foo=Bar%20Baz; Domain=host.com");
    let (_server_req, _client_res, _client_bytes) =
        run_agent(&mut agent, req1, resp1, |tide_req| async move { tide_req })?;

    let uri2: http::Uri = "https://another.host.com/cookie".parse().unwrap();
    let req2 = http::Request::builder().uri(&uri2).body(().into())?;
    let (server_req, _client_res, _client_bytes) =
        run_agent(&mut agent, req2, "Ok", |tide_req| async move { tide_req })?;

    // shared cookie domain
    assert_eq!(server_req.header("cookie"), Some("Foo=Bar%20Baz"));

    Ok(())
}

#[test]
fn cookie_with_different_path() -> Result<(), Error> {
    let mut agent = Agent::new();

    let uri1: http::Uri = "https://some.host.com/cookie/with/path".parse().unwrap();
    let req1 = http::Request::builder().uri(&uri1).body(().into())?;
    let resp1 = tide::Response::new(200)
        .body_string("Ok".to_string())
        .append_header("set-cookie", "Foo=Bar%20Baz; Path=/cookie/");
    let (_server_req, _client_res, _client_bytes) =
        run_agent(&mut agent, req1, resp1, |tide_req| async move { tide_req })?;

    let uri2: http::Uri = "https://some.host.com/cookie2".parse().unwrap();
    let req2 = http::Request::builder().uri(&uri2).body(().into())?;
    let (server_req, _client_res, _client_bytes) =
        run_agent(&mut agent, req2, "Ok", |tide_req| async move { tide_req })?;

    // no cookie here!
    assert_eq!(server_req.header("cookie"), None);

    Ok(())
}


#[test]
fn cookie_with_matching_path() -> Result<(), Error> {
    let mut agent = Agent::new();

    let uri1: http::Uri = "https://some.host.com/cookie/with/path".parse().unwrap();
    let req1 = http::Request::builder().uri(&uri1).body(().into())?;
    let resp1 = tide::Response::new(200)
        .body_string("Ok".to_string())
        .append_header("set-cookie", "Foo=Bar%20Baz; Path=/cookie/");
    let (_server_req, _client_res, _client_bytes) =
        run_agent(&mut agent, req1, resp1, |tide_req| async move { tide_req })?;

    let uri2: http::Uri = "https://some.host.com/cookie/with/other".parse().unwrap();
    let req2 = http::Request::builder().uri(&uri2).body(().into())?;
    let (server_req, _client_res, _client_bytes) =
        run_agent(&mut agent, req2, "Ok", |tide_req| async move { tide_req })?;

    // matching path
    assert_eq!(server_req.header("cookie"), Some("Foo=Bar%20Baz"));

    Ok(())
}

