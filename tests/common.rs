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
        _cx: &mut Context<'_>,
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
        use tracing::Level;
        use tracing_subscriber::fmt::format::FmtSpan;
        use tracing_subscriber::FmtSubscriber;

        let test_log = std::env::var("TEST_LOG")
            .map(|x| x != "0" && x.to_lowercase() != "false")
            .unwrap_or(false);
        let level = if test_log { Level::TRACE } else { Level::ERROR };

        let sub = FmtSubscriber::builder()
            .with_env_filter("hreq=trace,hreq_h1=trace,hreq_h2=trace")
            .with_max_level(level)
            .with_span_events(FmtSpan::CLOSE)
            .finish();

        tracing::subscriber::set_global_default(sub).expect("tracing set_global_default");
    });
}
