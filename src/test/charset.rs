use super::run_server;
use crate::prelude::*;
use crate::test_h1_h2;
use crate::Error;
use async_std_lib::fs::File;
use async_std_lib::io::BufReader;

test_h1_h2! {

    fn from_charset_iso8859() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/from_charset_iso8859")
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

    fn from_charset_iso8859_disable() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/from_charset_iso8859")
                // turns off charset decoding
                .charset_decode(false)
                .body(().into())?;
            let file = File::open("./data/iso8859.txt").block().unwrap();
            let buf_reader = BufReader::new(file);
            let resp = tide::Response::with_reader(200, buf_reader)
                .set_header("content-type", "text/plain; charset=iso8859-1");
            let (_server_req, client_res, client_bytes) = run_server(req, resp, |tide_req| {
                async move { tide_req }
            })?;
            // ÄEÖ.<lf>
            assert_eq!(&client_bytes[42..], &[196, 69, 214, 46, 10]);
            assert_eq!(client_res.header("content-type"), Some("text/plain; charset=iso8859-1"));
            Ok(())
        }
    }

    fn from_charset_shift_jis() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .uri("/from_charset_shift_jis")
                // decode to EUC_JP
                .charset_decode_target("EUC-JP")
                .body(().into())?;
            let file = File::open("./data/shiftjis.txt").block().unwrap();
            let buf_reader = BufReader::new(file);
            let resp = tide::Response::with_reader(200, buf_reader)
                .set_header("content-type", "text/plain; charset=Shift_JIS");
            let (_server_req, client_res, client_bytes) = run_server(req, resp, |tide_req| {
                async move { tide_req }
            })?;
            assert_eq!(client_res.header("content-type"), Some("text/plain; charset=Shift_JIS"));
            assert_eq!(client_bytes, &[164_u8, 170, 164, 207, 164, 232, 164, 166, 192, 164, 179, 166, 10]);
            Ok(())
        }
    }

    fn to_charset_iso8859() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .method("POST")
                .uri("/to_charset_iso8859")
                .header("content-type", "text/plain; charset=iso8859-1")
                .body("och åiåaäeö".into())?;
            let (_server_req, _client_res, _client_bytes) = run_server(req, "Ok", |mut tide_req| {
                async move {
                    let x = tide_req.body_bytes().await.unwrap();
                    assert_eq!(x, vec![111, 99, 104, 32, 229, 105, 229, 97, 228, 101, 246]);
                    tide_req
                }
            })?;
            Ok(())
        }
    }

    fn to_charset_shift_jis() -> Result<(), Error> {
        |bld: http::request::Builder| {
            // おはよう世界 in EUC-JP.
            let euc_jp = [164_u8, 170, 164, 207, 164, 232, 164, 166, 192, 164, 179, 166];
            let req = bld
                .method("POST")
                .uri("/to_charset_shift_jis")
                .charset_encode_source("EUC-JP")
                .header("content-type", "text/plain; charset=Shift_JIS")
                .body((&euc_jp[..]).into())?;
            let (_server_req, _client_res, _client_bytes) = run_server(req, "Ok", |mut tide_req| {
                async move {
                    let x = tide_req.body_bytes().await.unwrap();
                    assert_eq!(x, vec![130, 168, 130, 205, 130, 230, 130, 164, 144, 162, 138, 69]);
                    tide_req
                }
            })?;
            Ok(())
        }
    }

    fn to_charset_shift_jis_disable() -> Result<(), Error> {
        |bld: http::request::Builder| {
            // おはよう世界 in EUC-JP.
            let euc_jp = [164_u8, 170, 164, 207, 164, 232, 164, 166, 192, 164, 179, 166];
            let req = bld
                .method("POST")
                .uri("/to_charset_shift_jis")
                .charset_encode_source("EUC-JP")
                .charset_encode(false)
                // should have no effect now
                .header("content-type", "text/plain; charset=Shift_JIS")
                .body((&euc_jp[..]).into())?;
            let (_server_req, _client_res, _client_bytes) = run_server(req, "Ok", move |mut tide_req| {
                async move {
                    let x = tide_req.body_bytes().await.unwrap();
                    assert_eq!(x, euc_jp);
                    tide_req
                }
            })?;
            Ok(())
        }
    }

    fn to_charset_shift_jis_string() -> Result<(), Error> {
        |bld: http::request::Builder| {
            let req = bld
                .method("POST")
                .uri("/to_charset_shift_jis")
                // this should be ignored since the body is given as a UTF-8 string
                .charset_encode_source("EUC-JP")
                .header("content-type", "text/plain; charset=Shift_JIS")
                .body("おはよう世界".into())?;
            let (_server_req, _client_res, _client_bytes) = run_server(req, "Ok", |mut tide_req| {
                async move {
                    let x = tide_req.body_bytes().await.unwrap();
                    assert_eq!(x, vec![130, 168, 130, 205, 130, 230, 130, 164, 144, 162, 138, 69]);
                    tide_req
                }
            })?;
            Ok(())
        }
    }


}
