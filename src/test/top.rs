use crate::prelude::*;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;

#[test]
fn get_top_sites() {
    super::test_setup();
    let file = File::open("./topsites.txt").unwrap();
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        let x = reader.read_line(&mut line).ok();
        if x.is_none() || line == "" {
            break;
        }
        let url = format!("https://{}", line.trim());
        println!("{:?}", url);
        match http::Request::get(&url)
            .timeout_millis(5_000)
            .send(())
            .block()
        {
            Ok(mut resp) => match resp.body_mut().read_to_vec().block() {
                Ok(_) => {}
                Err(e) => println!("FAILED BODY: {}", e),
            },
            Err(e) => {
                println!("FAILED: {}", e);
            }
        }
    }
}

// this one redirects 2 times and reuses the connection the second time.
#[test]
fn get_apple() {
    super::test_setup();
    let resp = http::Request::get("https://apple.com")
        .send(())
        .block()
        .unwrap();
    assert_eq!(200, resp.status_code());
}

#[test]
fn get_blogspot() {
    super::test_setup();
    let mut resp = http::Request::get("https://blogspot.com")
        .send(())
        .block()
        .unwrap();
    assert_eq!(400, resp.status_code());
    assert!(!resp.body_mut().read_to_vec().block().unwrap().is_empty());
}

#[test]
fn get_player_vimeo() {
    super::test_setup();
    let mut resp = http::Request::get("https://player.vimeo.com")
        .send(())
        .block()
        .unwrap();
    assert_eq!(200, resp.status_code());
    assert!(!resp.body_mut().read_to_vec().block().unwrap().is_empty());
}

// this one times out
#[test]
fn get_macromedia() {
    super::test_setup();
    let mut resp = http::Request::get("https://macromedia.com")
        .timeout_millis(10_000)
        .send(())
        .block()
        .unwrap();
    assert_eq!(200, resp.status_code());
    assert!(!resp.body_mut().read_to_vec().block().unwrap().is_empty());
}

// caused charset decoder to finish many times
#[test]
fn get_youtube() {
    super::test_setup();
    let mut resp = http::Request::get("https://youtube.com")
        .timeout_millis(10_000)
        .send(())
        .block()
        .unwrap();
    assert_eq!(200, resp.status_code());
    assert!(!resp.body_mut().read_to_vec().block().unwrap().is_empty());
}
