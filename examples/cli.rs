//! This is a simple CLI interface. Takes the URL as a command line argument.
//! cargo run --example cli -- https://www.google.com/

use hreq::prelude::*;
use std::env;
use std::time::Duration;

fn main() {
    pretty_env_logger::formatted_builder()
        .filter_level(log::LevelFilter::Warn)
        .filter_module("hreq", log::LevelFilter::Trace)
        .filter_module("hreq-h1", log::LevelFilter::Trace)
        .target(env_logger::Target::Stdout)
        .init();

    match smoke_test() {
        Ok(()) => println!("Did not hang! Success"),
        Err(err) => {
            println!("Did not hang! Error: {}", err);
            std::process::exit(1);
        }
    }
}

fn smoke_test() -> Result<(), Box<dyn std::error::Error>> {
    let url = env::args().skip(1).next().expect("No URL provided");
    println!("Fetching {}", url);

    let response = Request::builder()
        .uri(&url)
        .timeout(Duration::from_secs(40))
        .call()
        .block()?;

    println!("HTTP status code: {}", response.status());

    // Print headers
    for header_name in response.headers().keys() {
        for value in response.headers().get_all(header_name).iter() {
            println!("Header: {}: {:?}", header_name, value);
        }
    }

    // This retains the whole body in memory, but tests show that RAM is plentiful, so I didn't bother optimizing.
    // Converting to a string lets us exercise encoding conversion routines.
    let mut body = response.into_body();
    let text = body.read_to_string().block()?;
    // Print the first 8k chars of the body to get an idea of what we've downloaded, ignore the rest.

    let first_8k_chars_of_body: String = text.chars().take(8192).collect();
    println!("{}", first_8k_chars_of_body);
    Ok(())
}
