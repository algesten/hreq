use hreq::prelude::*;
use io::BufRead;
use std::env;
use std::fs::File;
use std::io;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = env::args().skip(1).next().expect("No file provided");

    let mut file = io::BufReader::new(File::open(file)?);
    let mut buf = String::new();

    const CONCURRENT: u32 = 50;

    let sem = Arc::new(tokio::sync::Semaphore::new(CONCURRENT as usize));

    while file.read_line(&mut buf)? > 0 {
        let site = buf.trim();
        let url = format!("http://{}", site);

        let permit = sem.clone().acquire_owned().await?;

        tokio::task::spawn(async move {
            let _permit = permit;

            let res = Request::builder()
                .uri(&url)
                .timeout(Duration::from_secs(40))
                .call()
                .await;

            match res {
                Ok(_) => {
                    println!("OK {}", url);
                }
                Err(e) => {
                    println!("{} {}", e, url);
                }
            }
        });

        buf.clear();
    }

    let _ = sem.acquire_many(CONCURRENT).await?;

    Ok(())
}
