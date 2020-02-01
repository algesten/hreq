use crate::async_impl::never;
use crate::AsyncRuntime;
use crate::Error;
use futures_util::future::FutureExt;
use futures_util::select;
use std::future::Future;
use std::io;
use std::time::{Duration, Instant};

const ZERO: Duration = Duration::from_millis(0);

#[derive(Debug, Copy, Clone)]
pub struct Deadline {
    req_start: Option<Instant>,
    timeout: Option<Duration>,
}

impl Deadline {
    pub fn inert() -> Self {
        Self::new(None, None)
    }

    pub fn new(req_start: Option<Instant>, timeout: Option<Duration>) -> Self {
        Deadline { req_start, timeout }
    }

    pub async fn race<T, F, Err>(&self, f: F) -> Result<T, Error>
    where
        F: Future<Output = Result<T, Err>>,
        Err: Into<Error>,
    {
        // first to complete...
        select! {
            a = f.fuse() => match a {
                Ok(a) => Ok(a),
                Err(e) => Err(e.into())
            },
            b = self.delay().fuse() => Err(b)
        }
    }

    pub fn check_time_left(&self) -> Option<io::Error> {
        if let Some(remaining) = self.remaining() {
            if remaining == ZERO {
                return Some(io::Error::new(io::ErrorKind::TimedOut, "timeout"));
            }
        }
        None
    }

    async fn delay(&self) -> Error {
        if let Some(delay) = self.remaining() {
            if delay > ZERO {
                AsyncRuntime::current().timeout(delay).await;
            }
        } else {
            // never completes
            never().await;
        }
        Error::Static("timeout")
    }

    fn remaining(&self) -> Option<Duration> {
        match (self.req_start, self.timeout) {
            (Some(req_start), Some(timeout)) => {
                let remain = timeout.checked_sub(Instant::now() - req_start);
                let remain_or_zero = remain.unwrap_or(ZERO);
                Some(remain_or_zero)
            }

            _ => None,
        }
    }
}
