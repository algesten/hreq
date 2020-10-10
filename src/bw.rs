//! Calculates h2 window size given a current bandwidth estimation.
//!
//! Ideas from here: https://github.com/hyperium/hyper/blob/aafeeb7638c42a2f2e79a42a06625d4d2cfc76e3/src/proto/h2/ping.rs
//!
//! # BDP Algorithm
//!
//! 1. When receiving a DATA frame, if a BDP ping isn't outstanding:
//!   1a. Record current time.
//!   1b. Send a BDP ping.
//! 2. Increment the number of received bytes.
//! 3. When the BDP ping ack is received:
//!   3a. Record duration from sent time.
//!   3b. Merge RTT with a running average.
//!   3c. Calculate bdp as bytes/rtt.
//!   3d. If bdp is over 2/3 max, set new max to bdp and update windows.

use futures_util::ready;
use hreq_h2::{Ping, PingPong};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(crate) struct BandwidthMonitor {
    inner: Arc<Mutex<Inner>>,
}

type WindowSize = u32;

impl BandwidthMonitor {
    pub fn new(pinger: PingPong) -> Self {
        BandwidthMonitor {
            inner: Arc::new(Mutex::new(Inner {
                pinger,
                ping_sent: None,
                bytes: 0,
                bdp: Bdp::new(),
            })),
        }
    }

    pub fn append_read_bytes(&self, bytes: usize) {
        let mut lock = self.inner.lock().unwrap();
        lock.bytes += bytes;
    }

    /// Poll for ping/pong to drive banwidth monitoring resulting in h2 window updates.
    ///
    /// This poll() is special, it's just piggy-backed on polling the underlying connection.
    /// Hence it doesn't register any waker unless it's Pending on an outstanding pong.
    pub fn poll_window_update(&mut self, cx: &mut Context<'_>) -> Poll<WindowSize> {
        let mut lock = self.inner.lock().unwrap();

        if !lock.ping_sent.is_some() {
            // send ping if we haven't done so.

            match lock.pinger.send_ping(Ping::opaque()) {
                Ok(_) => {
                    lock.ping_sent = Some(Instant::now());
                }
                Err(e) => {
                    debug!("Error sending ping: {}", e);
                }
            }

            // no need to progress, since we must have a pong first.
            return Poll::Pending;
        }

        let (bytes, rtt) = match ready!(lock.pinger.poll_pong(cx)) {
            Ok(_pong) => {
                let rtt = lock.ping_sent.expect("Pong implies ping_sent").elapsed();

                lock.ping_sent = None;

                let bytes = lock.bytes;

                // reset back for next ping
                lock.bytes = 0;

                trace!("Received BDP pong; bytes = {}, rtt = {:?}", bytes, rtt);

                (bytes, rtt)
            }

            Err(e) => {
                debug!("Pong error: {}", e);

                return Poll::Pending;
            }
        };

        let window_update = lock.bdp.update(bytes, rtt);

        if let Some(window_update) = window_update {
            Poll::Ready(window_update)
        } else {
            Poll::Pending
        }
    }
}

struct Inner {
    /// Sender of pings, from connection.
    pinger: PingPong,

    /// Time we sent last ping. None if not sent.
    ping_sent: Option<Instant>,

    /// Accumulated bytes received since `ping_sent`.
    bytes: usize,

    /// BDP impl
    bdp: Bdp,
}

/// Any higher than this likely will be hitting the TCP flow control.
const BDP_LIMIT: usize = 1024 * 1024 * 16;

struct Bdp {
    /// Current bandwidth-delay product in bytes
    bdp: u32,

    /// The largest bandwidth observed so far.
    largest_bandwidth: f64,

    /// Current ping roundtrip time.
    rtt: f64,
}

impl Bdp {
    fn new() -> Self {
        Bdp {
            bdp: 0,
            largest_bandwidth: 0.0,
            rtt: 0.0,
        }
    }

    fn update(&mut self, bytes: usize, rtt: Duration) -> Option<WindowSize> {
        // Stop counting if we're at limit.
        if self.bdp as usize == BDP_LIMIT {
            return None;
        }

        // moving average for rtt
        let rtt = rtt.as_secs_f64();
        if self.rtt == 0.0 {
            // first ever rtt
            self.rtt = rtt;
        } else {
            // 1/8 moving average
            self.rtt += (rtt - self.rtt) * 0.125;
        }

        // Current bandwidth
        let bw = (bytes as f64) / (self.rtt * 1.5);
        trace!("Current bandwidth = {:.1}B/s", bw);

        if bw <= self.largest_bandwidth {
            // No update, since bandwidth didn't increase
            return None;
        } else {
            self.largest_bandwidth = bw;
        }

        if bytes >= self.bdp as usize * 2 / 3 {
            // if the current `bytes` sample is at least 2/3 the previous
            // bdp, increase to double the current sample.
            self.bdp = (bytes * 2).min(BDP_LIMIT) as WindowSize;

            trace!("BDP increased to {}", self.bdp);

            Some(self.bdp)
        } else {
            None
        }
    }
}
