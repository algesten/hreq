use futures_util::future::FutureExt;
use futures_util::select;
use std::fmt;
use std::future::Future;

use hreq_h1::mpsc::{Receiver, Sender};

/// Handle to a running server.
///
/// The server functions as long as this handle is not dropped.
pub struct ServerHandle {
    tx: Sender<()>,
}

impl ServerHandle {
    pub(crate) async fn new() -> (Self, EndFut) {
        let (tx, rx) = Receiver::new(1);

        (ServerHandle { tx }, EndFut { rx })
    }

    /// Signal to the server to close down. Stop listening to the port and exit.
    pub async fn shutdown(self) {
        self.tx.send(());
    }

    /// Await this to keep the server alive forever. Will never return.
    pub async fn keep_alive(self) -> ! {
        NoFuture.await;
        unreachable!()
    }
}

#[derive(Clone)]
pub(crate) struct EndFut {
    rx: Receiver<()>,
}

impl EndFut {
    pub async fn race<F>(&self, f: F) -> Option<F::Output>
    where
        F: Future,
    {
        // first to complete...
        // TODO: it might be possible to get rid of the fuse() here. futures_util
        // has new select versions that don't work like that.
        select! {
            a = f.fuse() => Some(a),
            b = self.rx.recv().fuse() => None
        }
    }
}

struct NoFuture;

impl std::future::Future for NoFuture {
    type Output = ();
    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context,
    ) -> std::task::Poll<Self::Output> {
        std::task::Poll::Pending
    }
}

impl fmt::Debug for ServerHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ServerHandle")
    }
}
