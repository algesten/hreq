use std::fmt;
use std::future::Future;
use std::sync::Arc;

use hreq_h1::mpsc::{Receiver, Sender};

/// Handle to a running server.
///
/// The server functions as long as this handle is not dropped.
pub struct ServerHandle {
    tx_shutdown: Sender<()>,
    rx_confirm: Receiver<()>,
}

impl ServerHandle {
    pub(crate) async fn new() -> (Self, EndFut) {
        let (tx_shutdown, rx_shutdown) = Receiver::new(1);
        let (tx_confirm, rx_confirm) = Receiver::new(1);

        (
            ServerHandle {
                tx_shutdown,
                rx_confirm,
            },
            EndFut {
                rx_shutdown,
                tx_confirm: Arc::new(tx_confirm),
            },
        )
    }

    /// Signal to the server to close down. Stop listening to the port and exit.
    pub async fn shutdown(self) {
        // When we drop the tx_shutdown sender, all connected
        // receivers are woken up and realise it's gone.
        let ServerHandle {
            tx_shutdown,
            rx_confirm,
        } = self;

        drop(tx_shutdown);

        trace!("Await server shutdown confirmation");
        rx_confirm.recv().await;
    }

    /// Await this to keep the server alive forever. Will never return.
    pub async fn keep_alive(self) -> ! {
        NoFuture.await;
        unreachable!()
    }
}

#[derive(Clone)]
pub(crate) struct EndFut {
    rx_shutdown: Receiver<()>,
    tx_confirm: Arc<Sender<()>>,
}

impl EndFut {
    pub async fn race<F>(&self, f: F) -> Option<F::Output>
    where
        F: Future,
    {
        // first to complete...

        let wait_for_value = Box::pin(async {
            let v = f.await;
            Some(v)
        });

        let wait_for_end = Box::pin(async {
            self.rx_shutdown.recv().await;
            None
        });

        let ret = Select(Some(Inner(wait_for_value, wait_for_end))).await;

        trace!("Race is ended: {}", ret.is_none());

        ret
    }
}

impl Drop for EndFut {
    fn drop(&mut self) {
        let count = Arc::strong_count(&self.tx_confirm);
        trace!("EndFut instances left: {}", count - 1);
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

use std::pin::Pin;
use std::task::{Context, Poll};

struct Select<A, B>(Option<Inner<A, B>>);

struct Inner<A, B>(A, B);

impl<A, B, T> Future for Select<A, B>
where
    A: Future<Output = T> + Unpin,
    B: Future<Output = T> + Unpin,
{
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();

        if let Some(inner) = &mut this.0 {
            let ret = match Pin::new(&mut inner.0).poll(cx) {
                Poll::Ready(v) => Poll::Ready(v),
                Poll::Pending => Pin::new(&mut inner.1).poll(cx),
            };

            if ret.is_ready() {
                this.0 = None;
            }

            ret
        } else {
            warn!("Poll::Pending on finished Select");

            Poll::Pending
        }
    }
}
