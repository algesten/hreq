//! pluggable runtimes

use crate::either::Either;
use crate::Error;
use crate::Stream;
use crate::{AsyncRead, AsyncReadSeek, AsyncSeek, AsyncWrite};
use futures_util::future::poll_fn;
use once_cell::sync::Lazy;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

#[cfg(feature = "tokio")]
use tokio::runtime::Runtime as TokioRuntime;

#[cfg(not(feature = "tokio"))]
pub(crate) struct TokioRuntime;

#[allow(clippy::needless_doctest_main)]
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
/// [`Handle`]: https://docs.rs/tokio/latest/tokio/runtime/struct.Handle.html
/// [`Runtime`]: https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum AsyncRuntime {
    /// Use `async-std` crate.
    ///
    /// # Example using `AsyncStd`:
    ///
    /// ```
    /// use hreq::AsyncRuntime;
    /// AsyncRuntime::AsyncStd.make_default();
    /// ```
    ///
    #[cfg(feature = "async-std")]
    AsyncStd,
    /// Use a tokio `rt-core` single threaded runtime. This is the default.
    #[cfg(feature = "tokio")]
    TokioSingle,
    /// Pick up on a tokio shared runtime.
    ///
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
    #[cfg(feature = "tokio")]
    TokioShared,
    /// Use a tokio runtime owned by hreq.
    ///
    /// # Example using an owned tokio.
    ///
    /// ```
    /// use hreq::AsyncRuntime;
    /// // normally: use tokio::runtime::Builder;
    /// use tokio::runtime::Builder;
    ///
    /// let runtime = Builder::new()
    ///   .enable_io()
    ///   .enable_time()
    ///   .build()
    ///   .expect("Failed to build tokio runtime");
    ///
    /// AsyncRuntime::TokioOwned(runtime).make_default();
    /// ```
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

#[cfg(feature = "server")]
#[allow(dead_code)]
pub(crate) enum Listener {
    #[cfg(feature = "async-std")]
    AsyncStd(::async_std::net::TcpListener),
    #[cfg(not(feature = "async-std"))]
    AsyncStd(FakeListener),
    #[cfg(feature = "tokio")]
    Tokio(tokio::net::TcpListener),
    #[cfg(not(feature = "tokio"))]
    Tokio(FakeListener),
}

#[cfg(feature = "server")]
impl Listener {
    pub async fn accept(&mut self) -> Result<(impl Stream, SocketAddr), Error> {
        use Listener::*;
        Ok(match self {
            Tokio(v) => {
                let (t, a) = v.accept().await?;
                #[cfg(feature = "tokio")]
                {
                    (Either::A(crate::tokio::from_tokio(t)), a)
                }
                #[cfg(not(feature = "tokio"))]
                {
                    (Either::A(t), a)
                }
            }
            AsyncStd(v) => {
                let (t, a) = v.accept().await?;
                (Either::B(t), a)
            }
        })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match self {
            Listener::AsyncStd(l) => l.local_addr(),
            Listener::Tokio(l) => l.local_addr(),
        }
    }
}

static CURRENT_RUNTIME: Lazy<Mutex<Inner>> = Lazy::new(|| {
    let rt = if cfg!(feature = "async-std") {
        Inner::AsyncStd
    } else if cfg!(feature = "tokio") {
        #[cfg(feature = "tokio")]
        if tokio::runtime::Handle::try_current().ok().is_some() {
            trace!("Shared tokio runtime detected");
            async_tokio::use_shared();
        } else {
            async_tokio::use_default();
        }

        Inner::TokioSingle
    } else {
        panic!("No default async runtime. Use feature 'tokio' or 'async-std'");
    };

    trace!("Default runtime: {:?}", rt);

    Mutex::new(rt)
});

fn current() -> Inner {
    *CURRENT_RUNTIME.lock().unwrap()
}

impl AsyncRuntime {
    fn into_inner(self) -> Inner {
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

    /// Make this runtime the default.
    pub fn make_default(self) {
        let mut current = CURRENT_RUNTIME.lock().unwrap();

        trace!(
            "Set runtime: {:?}, num_cpu: {}, num_physical_cpu: {}",
            self,
            num_cpus::get(),
            num_cpus::get_physical()
        );

        let inner = self.into_inner();
        *current = inner;
    }

    pub(crate) async fn connect_tcp(addr: &str) -> Result<impl Stream, Error> {
        use Inner::*;
        Ok(match current() {
            TokioSingle | TokioShared | TokioOwned => {
                Either::A(async_tokio::connect_tcp(addr).await?)
            }
            AsyncStd => Either::B(async_std::connect_tcp(addr).await?),
        })
    }

    pub(crate) async fn timeout(duration: Duration) {
        use Inner::*;
        match current() {
            AsyncStd => async_std::timeout(duration).await,
            TokioSingle | TokioShared | TokioOwned => async_tokio::timeout(duration).await,
        }
    }

    #[doc(hidden)]
    pub fn spawn<T: Future + Send + 'static>(task: T) {
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

    #[cfg(feature = "server")]
    pub(crate) async fn listen(addr: SocketAddr) -> Result<Listener, Error> {
        use Inner::*;
        match current() {
            AsyncStd => async_std::listen(addr).await,
            TokioSingle | TokioShared | TokioOwned => async_tokio::listen(addr).await,
        }
    }

    pub(crate) fn file_to_reader(file: std::fs::File) -> impl AsyncReadSeek {
        use Inner::*;
        match current() {
            AsyncStd => Either::A(async_std::file_to_reader(file)),
            TokioSingle | TokioShared | TokioOwned => Either::B(async_tokio::file_to_reader(file)),
        }
    }
}

#[cfg(not(feature = "async-std"))]
mod async_std {
    use super::*;
    pub(crate) async fn connect_tcp(_: &str) -> Result<impl Stream, Error> {
        Ok(FakeStream) // fulfil type checker
    }
    pub(crate) async fn timeout(_: Duration) {
        unreachable!();
    }
    pub(crate) fn spawn<T>(_: T)
    where
        T: Future + Send + 'static,
    {
        unreachable!();
    }
    pub(crate) fn block_on<F: Future>(_: F) -> F::Output {
        unreachable!();
    }

    #[cfg(feature = "server")]
    pub(crate) async fn listen(_: SocketAddr) -> Result<Listener, Error> {
        unreachable!();
    }

    pub(crate) fn file_to_reader(_: std::fs::File) -> impl AsyncReadSeek {
        FakeStream
    }
}

#[cfg(feature = "async-std")]
#[allow(unused)]
pub(crate) mod async_std {
    use super::*;

    pub(crate) async fn connect_tcp(addr: &str) -> Result<impl Stream, Error> {
        Ok(::async_std::net::TcpStream::connect(addr).await?)
    }

    pub(crate) async fn timeout(duration: Duration) {
        ::async_std::future::timeout(duration, never()).await.ok();
    }

    pub(crate) fn spawn<T>(task: T)
    where
        T: Future + Send + 'static,
    {
        ::async_std::task::spawn(async move {
            task.await;
        });
    }

    pub(crate) fn block_on<F: Future>(task: F) -> F::Output {
        ::async_std::task::block_on(task)
    }

    #[cfg(feature = "server")]
    pub(crate) async fn listen(addr: SocketAddr) -> Result<Listener, Error> {
        use ::async_std::net::TcpListener;
        let listener = TcpListener::bind(addr).await?;
        Ok(Listener::AsyncStd(listener))
    }

    pub(crate) fn file_to_reader(file: std::fs::File) -> impl AsyncReadSeek {
        let file: ::async_std::fs::File = file.into();
        file
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
    pub(crate) async fn timeout(_: Duration) {
        unreachable!();
    }
    pub(crate) fn spawn<T>(_: T)
    where
        T: Future + Send + 'static,
    {
        unreachable!();
    }
    pub(crate) fn block_on<F: Future>(_: F) -> F::Output {
        unreachable!();
    }

    #[cfg(feature = "server")]
    pub(crate) async fn listen(_: SocketAddr) -> Result<Listener, Error> {
        unreachable!();
    }

    pub(crate) fn file_to_reader(_: std::fs::File) -> impl AsyncReadSeek {
        FakeStream
    }
}

#[cfg(feature = "tokio")]
pub(crate) mod async_tokio {
    use super::*;
    use crate::tokio::from_tokio;
    use std::sync::Mutex;
    use tokio::net::TcpStream;
    use tokio::runtime::Builder;
    use tokio::runtime::Handle;

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
    pub(crate) async fn timeout(duration: Duration) {
        tokio::time::delay_for(duration).await;
    }
    pub(crate) fn spawn<T>(task: T)
    where
        T: Future + Send + 'static,
    {
        let mut handle = HANDLE.lock().unwrap();
        handle.as_mut().unwrap().spawn(async move {
            task.await;
        });
    }
    pub(crate) fn block_on<F: Future>(task: F) -> F::Output {
        let mut rt = RUNTIME.lock().unwrap();
        if let Some(rt) = rt.as_mut() {
            rt.block_on(task)
        } else {
            panic!("Can't use .block() with a TokioShared runtime.");
        }
    }

    #[cfg(feature = "server")]
    pub(crate) async fn listen(addr: SocketAddr) -> Result<Listener, Error> {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind(addr).await?;
        Ok(Listener::Tokio(listener))
    }

    pub(crate) fn file_to_reader(file: std::fs::File) -> impl AsyncReadSeek {
        let file = tokio::fs::File::from_std(file);
        from_tokio(file)
    }
}

// TODO does this cause memory leaks?
pub async fn never() {
    poll_fn::<(), _>(|_| Poll::Pending).await;
    unreachable!()
}

#[allow(unused)]
pub(crate) struct FakeListener(SocketAddr);

#[allow(unused)]
impl FakeListener {
    async fn accept(&mut self) -> Result<(FakeStream, SocketAddr), io::Error> {
        Ok((FakeStream, self.0))
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        unreachable!("local_addr() on FakeListener");
    }
}

// filler in for "impl Stream" type
struct FakeStream;

impl AsyncRead for FakeStream {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context,
        _: &mut [u8],
    ) -> Poll<futures_io::Result<usize>> {
        unreachable!()
    }
}
impl AsyncWrite for FakeStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context,
        _: &[u8],
    ) -> Poll<futures_io::Result<usize>> {
        unreachable!()
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context) -> Poll<futures_io::Result<()>> {
        unreachable!()
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context) -> Poll<futures_io::Result<()>> {
        unreachable!()
    }
}

impl AsyncSeek for FakeStream {
    fn poll_seek(self: Pin<&mut Self>, _: &mut Context, _: io::SeekFrom) -> Poll<io::Result<u64>> {
        unreachable!()
    }
}
