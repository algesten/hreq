use super::run_server;
use crate::prelude::*;
use crate::test_h1_h2;
use crate::Error;
use async_std_lib::fs::File;
use async_std_lib::io::BufReader;

test_h1_h2! {

    fn charset_iso8859() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/charset_iso8859")
                .body(().into())?;

            let file = File::open("./data/iso8859.txt").block().unwrap();
            let buf_reader = BufReader::new(file);
            let resp = tide::Response::with_reader(200, buf_reader)
                .set_header("content-type", "text/plain; charset=iso8859-1");
            let (_server_req, client_res, client_bytes) = run_server(req, resp, |tide_req| {
                async move { tide_req }
            })?;
            let body = String::from_utf8(client_bytes).unwrap();
            assert_eq!(body, "and in the river is an island. åiåaäeöÅIÅAÄEÖ.\n");
            assert_eq!(client_res.header("content-type"), Some("text/plain; charset=iso8859-1"));
            Ok(())
        }
    }

}
