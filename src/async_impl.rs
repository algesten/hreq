//! pluggable runtimes

use crate::either::Either;
use crate::Error;
use crate::Stream;
use futures_util::future::poll_fn;
use once_cell::sync::Lazy;
use std::future::Future;
use std::sync::Mutex;
use std::task::Poll;
use std::time::Duration;

#[cfg(feature = "tokio")]
use tokio_lib::runtime::Runtime as TokioRuntime;

#[cfg(not(feature = "tokio"))]
pub(crate) struct TokioRuntime;

/// Switches between different async runtimes.
///
/// This is a global singleton.
///
/// hreq supports async-std and tokio. Tokio support is enabled by default and comes in some
/// different flavors.
///
///   * `AsyncStd`. Requires the feature `async-std`. Supports
///     `.block()`.
///   * `TokioSingle`. The default option. A minimal tokio `rt-core`
///     which executes calls in one single thread. It does nothing
///     until the current thread blocks on a future using `.block()`.
///   * `TokioShared`. Picks up on a globally shared runtime by using a
///     [`Handle`]. This runtime cannot use the `.block()` extension
///     trait since that requires having a direct connection to the
///     tokio [`Runtime`].
///   * `TokioOwned`. Uses a preconfigured tokio [`Runtime`] that is
///     "handed over" to hreq.
///
///
/// # Example using `AsyncStd`:
///
/// ```
/// use hreq::AsyncRuntime;
/// #[cfg(feature = "async-std")]
/// AsyncRuntime::AsyncStd.make_default();
/// ```
///
/// # Example using a shared tokio.
///
/// ```no_run
/// use hreq::AsyncRuntime;
///
/// // assuming the current thread has some tokio runtime, such
/// // as using the `#[tokio::main]` macro on `fn main() { .. }`
///
/// AsyncRuntime::TokioShared.make_default();
/// ```
///
/// # Example using an owned tokio.
///
/// ```
/// use hreq::AsyncRuntime;
/// // normally: use tokio::runtime::Builder;
/// use tokio_lib::runtime::Builder;
///
/// let runtime = Builder::new()
///   .enable_io()
///   .enable_time()
///   .build()
///   .expect("Failed to build tokio runtime");
///
/// AsyncRuntime::TokioOwned(runtime).make_default();
/// ```
///
///
/// [`Handle`]: https://docs.rs/tokio/latest/tokio/runtime/struct.Handle.html
/// [`Runtime`]: https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html
#[derive(Debug)]
pub enum AsyncRuntime {
    #[cfg(feature = "async-std")]
    AsyncStd,
    #[cfg(feature = "tokio")]
    TokioSingle,
    #[cfg(feature = "tokio")]
    TokioShared,
    #[cfg(feature = "tokio")]
    TokioOwned(TokioRuntime),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(unused)]
enum Inner {
    AsyncStd,
    TokioSingle,
    TokioShared,
    TokioOwned,
}

static CURRENT_RUNTIME: Lazy<Mutex<Inner>> = Lazy::new(|| {
    Mutex::new(if cfg!(feature = "async-std") {
        Inner::AsyncStd
    } else if cfg!(feature = "tokio") {
        async_tokio::use_default();
        Inner::TokioSingle
    } else {
        panic!("No default async runtime. Use feature 'tokio' or 'async-std'");
    })
});

fn current() -> Inner {
    *CURRENT_RUNTIME.lock().unwrap()
}

impl AsyncRuntime {
    fn to_inner(self) -> Inner {
        match self {
            #[cfg(feature = "async-std")]
            AsyncRuntime::AsyncStd => Inner::AsyncStd,
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioSingle => {
                async_tokio::use_default();
                Inner::TokioSingle
            }
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioShared => {
                async_tokio::use_shared();
                Inner::TokioShared
            }
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioOwned(rt) => {
                async_tokio::use_owned(rt);
                Inner::TokioOwned
            }
        }
    }

    pub fn make_default(self) {
        let mut current = CURRENT_RUNTIME.lock().unwrap();
        let inner = self.to_inner();
        *current = inner;
    }

    pub(crate) async fn connect_tcp(addr: &str) -> Result<impl Stream, Error> {
        use Inner::*;
        Ok(match current() {
            AsyncStd => Either::A(async_std::connect_tcp(addr).await?),
            TokioSingle | TokioShared | TokioOwned => {
                Either::B(async_tokio::connect_tcp(addr).await?)
            }
        })
    }

    pub(crate) async fn timeout(duration: Duration) {
        use Inner::*;
        match current() {
            AsyncStd => async_std::timeout(duration).await,
            TokioSingle | TokioShared | TokioOwned => async_tokio::timeout(duration).await,
        }
    }

    pub(crate) fn spawn<T: Future + Send + 'static>(task: T) {
        use Inner::*;
        match current() {
            AsyncStd => async_std::spawn(task),
            TokioSingle | TokioShared | TokioOwned => async_tokio::spawn(task),
        }
    }

    pub(crate) fn block_on<F: Future>(task: F) -> F::Output {
        use Inner::*;
        match current() {
            AsyncStd => async_std::block_on(task),
            TokioSingle | TokioShared | TokioOwned => async_tokio::block_on(task),
        }
    }
}

// pub async fn connect_tcp(addr: &str) -> Result<impl Stream, Error>;
// pub async fn timeout(duration: Duration);
// pub fn spawn<T>(task: T) where T: Future + Send + 'static;
// pub fn block_on<F: Future>(task: F) -> F::Output;

#[cfg(not(feature = "async-std"))]
pub(crate) mod async_std {
    use super::*;
    pub(crate) async fn connect_tcp(_: &str) -> Result<impl Stream, Error> {
        Ok(FakeStream) // fulfil type checker
    }
    pub async fn timeout(_: Duration) {
        unreachable!();
    }
    pub fn spawn<T>(_: T)
    where
        T: Future + Send + 'static,
    {
        unreachable!();
    }
    pub fn block_on<F: Future>(_: F) -> F::Output {
        unreachable!();
    }
}

#[cfg(feature = "async-std")]
#[allow(unused)]
pub(crate) mod async_std {
    use super::*;
    use async_std_lib::net::TcpStream;
    use async_std_lib::task;

    impl Stream for TcpStream {}

    pub(crate) async fn connect_tcp(addr: &str) -> Result<impl Stream, Error> {
        Ok(TcpStream::connect(addr).await?)
    }

    pub async fn timeout(duration: Duration) {
        async_std_lib::future::timeout(duration, never()).await.ok();
    }

    pub fn spawn<T>(task: T)
    where
        T: Future + Send + 'static,
    {
        async_std_lib::task::spawn(async move {
            task.await;
        });
    }

    pub fn block_on<F: Future>(task: F) -> F::Output {
        task::block_on(task)
    }
}

#[cfg(not(feature = "tokio"))]
#[allow(unused)]
pub(crate) mod async_tokio {
    use super::*;
    pub(crate) fn use_default() {
        unreachable!();
    }
    pub(crate) fn use_shared() {
        unreachable!();
    }
    pub(crate) fn use_owned(rt: TokioRuntime) {
        unreachable!();
    }
    pub(crate) async fn connect_tcp(_: &str) -> Result<impl Stream, Error> {
        Ok(FakeStream) // fulfil type checker
    }
    pub async fn timeout(_: Duration) {
        unreachable!();
    }
    pub fn spawn<T>(_: T)
    where
        T: Future + Send + 'static,
    {
        unreachable!();
    }
    pub fn block_on<F: Future>(_: F) -> F::Output {
        unreachable!();
    }
}

#[cfg(feature = "tokio")]
pub(crate) mod async_tokio {
    use super::*;
    use crate::tokio::from_tokio;
    use std::sync::Mutex;
    use tokio_lib::net::TcpStream;
    use tokio_lib::runtime::Builder;
    use tokio_lib::runtime::Handle;

    static RUNTIME: Lazy<Mutex<Option<TokioRuntime>>> = Lazy::new(|| Mutex::new(None));
    static HANDLE: Lazy<Mutex<Option<Handle>>> = Lazy::new(|| Mutex::new(None));

    fn set_singletons(handle: Handle, rt: Option<TokioRuntime>) {
        let mut rt_handle = HANDLE.lock().unwrap();
        *rt_handle = Some(handle);
        let mut rt_singleton = RUNTIME.lock().unwrap();
        *rt_singleton = rt;
    }

    fn unset_singletons() {
        let unset = || {
            let rt = RUNTIME.lock().unwrap().take();
            {
                let _ = HANDLE.lock().unwrap().take(); // go out of scope
            }
            if let Some(rt) = rt {
                rt.shutdown_timeout(Duration::from_millis(10));
            }
        };

        // this fails if we are currently running in a tokio context.
        let is_in_context = Handle::try_current().is_ok();

        if is_in_context {
            std::thread::spawn(unset).join().unwrap();
        } else {
            unset();
        }
    }

    pub(crate) fn use_default() {
        unset_singletons();
        let (handle, rt) = create_default_runtime();
        set_singletons(handle, Some(rt));
    }
    pub(crate) fn use_shared() {
        unset_singletons();
        let handle = Handle::current();
        set_singletons(handle, None);
    }
    pub(crate) fn use_owned(rt: TokioRuntime) {
        unset_singletons();
        let handle = rt.handle().clone();
        set_singletons(handle, Some(rt));
    }

    fn create_default_runtime() -> (Handle, TokioRuntime) {
        let runtime = Builder::new()
            .basic_scheduler()
            .enable_io()
            .enable_time()
            .build()
            .expect("Failed to build tokio runtime");
        let handle = runtime.handle().clone();
        (handle, runtime)
    }

    pub(crate) async fn connect_tcp(addr: &str) -> Result<impl Stream, Error> {
        Ok(from_tokio(TcpStream::connect(addr).await?))
    }
    pub async fn timeout(duration: Duration) {
        tokio_lib::time::delay_for(duration).await;
    }
    pub fn spawn<T>(task: T)
    where
        T: Future + Send + 'static,
    {
        let mut handle = HANDLE.lock().unwrap();
        handle.as_mut().unwrap().spawn(async move {
            task.await;
        });
    }
    pub fn block_on<F: Future>(task: F) -> F::Output {
        let mut rt = RUNTIME.lock().unwrap();
        if let Some(rt) = rt.as_mut() {
            rt.block_on(task)
        } else {
            panic!("Can't use .block() with a TokioShared runtime.");
        }
    }
}

// TODO does this cause memory leaks?
pub async fn never() {
    poll_fn::<(), _>(|_| Poll::Pending).await;
    unreachable!()
}

// filler in for "impl Stream" type
struct FakeStream;

use crate::AsyncRead;
use crate::AsyncWrite;
use std::pin::Pin;
use std::task::Context;

impl Stream for FakeStream {}

impl AsyncRead for FakeStream {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut [u8],
    ) -> Poll<futures_io::Result<usize>> {
        unreachable!()
    }
}
impl AsyncWrite for FakeStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &[u8],
    ) -> Poll<futures_io::Result<usize>> {
        unreachable!()
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<futures_io::Result<()>> {
        unreachable!()
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<futures_io::Result<()>> {
        unreachable!()
    }
}
