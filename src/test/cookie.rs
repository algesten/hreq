use super::run_agent;
use crate::prelude::*;
use crate::test_h1_h2;
use crate::Agent;
use crate::Error;

test_h1_h2! {
    fn cookie_simple() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let uri: http::Uri = "https://some.host.com/cookie_simple".parse().unwrap();
            let req = bld
                .uri(&uri)
                .body(().into())?;
            let mut agent = Agent::new();
            let resp = tide::Response::new(200).body_string("Ok".to_string())
                .set_header("set-cookie", "Foo=Bar");
            let (_server_req, _client_res, _client_bytes) = run_agent(&mut agent, req, resp, |tide_req| {
                async move {
                    tide_req
                }
            })?;
            let cookies = agent.get_cookies(&uri);
            println!("{:?}", cookies);
            Ok(())
        }
    }
}
