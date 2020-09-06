#![allow(unused)]

use rand::Rng;
use std::io;
use std::io::Read;
use std::pin::Pin;
use std::sync::Once;
use std::task::{Context, Poll};

#[allow(unused_imports)]
pub(crate) use futures_io::{AsyncBufRead, AsyncRead, AsyncWrite};

#[derive(Debug)]
pub struct DataGenerator {
    total: usize,
    produced: usize,
}

impl DataGenerator {
    pub fn new(total: usize) -> Self {
        DataGenerator { total, produced: 0 }
    }
}

impl io::Read for DataGenerator {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut rng = rand::thread_rng();
        let max = buf.len().min(self.total - self.produced);
        rng.fill(&mut buf[0..max]);
        self.produced += max;
        Ok(max)
    }
}

impl AsyncRead for DataGenerator {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let amount = this.read(buf)?;
        Ok(amount).into()
    }
}

pub fn setup_logger() {
    static START: Once = Once::new();
    START.call_once(|| {
        let test_log = std::env::var("TEST_LOG")
            .map(|x| x != "0" && x.to_lowercase() != "false")
            .unwrap_or(false);
        let level = if test_log {
            log::LevelFilter::Trace
        } else {
            log::LevelFilter::Info
        };
        pretty_env_logger::formatted_builder()
            .filter_level(log::LevelFilter::Warn)
            .filter_module("hreq", level)
            .target(env_logger::Target::Stdout)
            .init();
    });
}
