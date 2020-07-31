use crate::async_impl::never;
use crate::AsyncRuntime;
use crate::Error;
use futures_util::future::FutureExt;
use futures_util::select;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::time::{Duration, Instant};

const ZERO: Duration = Duration::from_millis(0);

#[derive(Debug, Copy, Clone)]
pub(crate) struct Deadline {
    req_start: Option<Instant>,
    timeout: Option<Duration>,
}

impl Deadline {
    pub fn new(req_start: Option<Instant>, timeout: Option<Duration>) -> Self {
        Deadline { req_start, timeout }
    }

    pub async fn race<T, F, Err>(&self, f: F) -> Result<T, Error>
    where
        F: Future<Output = Result<T, Err>>,
        Err: Into<Error>,
    {
        // first to complete...
        // TODO: it might be possible to get rid of the fuse() here. futures_util
        // has new select versions that don't work like that.
        select! {
            a = f.fuse() => match a {
                Ok(a) => Ok(a),
                Err(e) => Err(e.into())
            },
            b = self.delay().fuse() => Err(b)
        }
    }

    pub fn delay_fut(&self) -> Pin<Box<dyn Future<Output = io::Error> + Send + Sync>> {
        let delay = self.remaining();
        let fut = async move {
            if let Some(delay) = delay {
                if delay > ZERO {
                    AsyncRuntime::timeout(delay).await;
                }
            } else {
                // never completes
                never().await;
            }
            io::Error::new(io::ErrorKind::TimedOut, "timeout")
        };
        Box::pin(fut)
    }

    async fn delay(&self) -> Error {
        self.delay_fut().await.into()
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
