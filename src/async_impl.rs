//! pluggable runtimes

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

// TODO: This whole file is a bit of a mess. Needs refactoring.

static CURRENT_RUNTIME: Lazy<Mutex<AsyncRuntime>> = Lazy::new(|| {
    if cfg!(feature = "tokio") {
        #[cfg(feature = "tokio")]
        return Mutex::new(AsyncRuntime::TokioDefault);
    } else if cfg!(feature = "async-std") {
        #[cfg(feature = "async-std")]
        return Mutex::new(AsyncRuntime::AsyncStd);
    }
    panic!("No default async runtime. Use cargo feature 'async-std' or 'tokio'");
});

static TOKIO_OWNED_RUNTIME: Lazy<Mutex<Option<TokioRuntime>>> = Lazy::new(|| Mutex::new(None));

/// Switches between different async runtimes.
///
/// This is a global singleton.
///
/// hreq supports async-std and tokio. Tokio support is enabled by default and comes in some
/// different flavors.
///
///   * `AsyncStd`. Requires the feature `async-std`. Supports
///     `.block()`.
///   * `TokioDefault`. The default option. A minimal tokio `rt-core`
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
/// AsyncRuntime::set_default(AsyncRuntime::AsyncStd, None);
/// ```
///
/// # Example using a shared tokio.
///
/// ```
/// use hreq::AsyncRuntime;
///
/// AsyncRuntime::set_default(AsyncRuntime::TokioShared, None);
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
/// AsyncRuntime::set_default(AsyncRuntime::TokioOwned, Some(runtime));
/// ```
///
///
/// [`Handle`]: https://docs.rs/tokio/0.2.11/tokio/runtime/struct.Handle.html
/// [`Runtime`]: https://docs.rs/tokio/0.2.11/tokio/runtime/struct.Runtime.html
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsyncRuntime {
    #[cfg(feature = "async-std")]
    AsyncStd,
    #[cfg(feature = "tokio")]
    TokioDefault,
    #[cfg(feature = "tokio")]
    TokioShared,
    #[cfg(feature = "tokio")]
    TokioOwned,
}

impl AsyncRuntime {
    /// Sets the runtime global singleton.
    ///
    /// The `owned` parameter must be used together with `TokioOwned`, all other runtimes must
    /// have `None` as the second argument.
    ///
    /// Panics on an incorrect combination of first/second argument.
    ///
    /// ```
    /// use hreq::AsyncRuntime;
    ///
    /// AsyncRuntime::set_default(AsyncRuntime::TokioShared, None);
    /// ```
    pub fn set_default(rt: Self, owned: Option<TokioRuntime>) {
        let mut ptr = CURRENT_RUNTIME.lock().unwrap();
        *ptr = rt;
        if rt == AsyncRuntime::TokioOwned {
            if let Some(owned) = owned {
                let mut lock = TOKIO_OWNED_RUNTIME.lock().unwrap();
                *lock = Some(owned);
            } else {
                panic!("AsyncRuntime::set_default {:?} without TokioRuntime", rt);
            }
        } else if owned.is_some() {
            panic!("AsyncRuntime::set_default {:?} with TokioRuntime", rt);
        } else {
            let mut lock = TOKIO_OWNED_RUNTIME.lock().unwrap();
            *lock = None;
        }
    }

    pub(crate) fn current() -> Self {
        *CURRENT_RUNTIME.lock().unwrap()
    }

    pub(crate) async fn connect_tcp(self, addr: &str) -> Result<impl Stream, Error> {
        #[cfg(all(feature = "async-std", feature = "tokio"))]
        {
            use crate::either::Either;
            match self {
                AsyncRuntime::AsyncStd => Ok(Either::A(async_std::connect_tcp(addr).await?)),
                AsyncRuntime::TokioDefault
                | AsyncRuntime::TokioShared
                | AsyncRuntime::TokioOwned => Ok(Either::B(async_tokio::connect_tcp(addr).await?)),
            }
        }
        #[cfg(all(feature = "async-std", not(feature = "tokio")))]
        {
            Ok(async_std::connect_tcp(addr).await?)
        }
        #[cfg(all(feature = "tokio", not(feature = "async-std")))]
        {
            Ok(async_tokio::connect_tcp(addr).await?)
        }
    }

    pub(crate) async fn timeout(self, duration: Duration) {
        match self {
            #[cfg(feature = "async-std")]
            AsyncRuntime::AsyncStd => async_std::timeout(duration).await,
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioDefault | AsyncRuntime::TokioShared | AsyncRuntime::TokioOwned => {
                async_tokio::timeout(duration).await
            }
        }
    }

    pub(crate) fn spawn<T: Future + Send + 'static>(self, task: T) {
        match self {
            #[cfg(feature = "async-std")]
            AsyncRuntime::AsyncStd => async_std::spawn(task),
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioDefault => async_tokio::default_spawn(task),
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioShared => {
                async_tokio::Handle::current().spawn(async move {
                    task.await;
                });
            }
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioOwned => {
                let handle = {
                    TOKIO_OWNED_RUNTIME
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .handle()
                        .clone()
                };
                handle.spawn(async move {
                    task.await;
                });
            }
        }
    }

    pub(crate) fn block_on<F: Future>(self, future: F) -> F::Output {
        match self {
            #[cfg(feature = "async-std")]
            AsyncRuntime::AsyncStd => async_std::block_on(future),
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioDefault => async_tokio::default_block_on(future),
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioShared => {
                panic!("Blocking is not possible with a shared tokio runtime")
            }
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioOwned => {
                let mut lock = TOKIO_OWNED_RUNTIME.lock().unwrap();
                let rt = lock.as_mut().unwrap();
                rt.block_on(future)
            }
        }
    }
}

#[cfg(feature = "async-std")]
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

    pub fn block_on<F: Future>(future: F) -> F::Output {
        task::block_on(future)
    }
}

#[cfg(feature = "tokio")]
pub(crate) mod async_tokio {
    use super::*;
    use crate::tokio::from_tokio;
    use once_cell::sync::OnceCell;
    use std::sync::Mutex;
    use tokio_lib::net::TcpStream;
    pub use tokio_lib::runtime::Handle;
    use tokio_lib::runtime::{Builder, Runtime};

    static RUNTIME: OnceCell<Mutex<Runtime>> = OnceCell::new();
    static HANDLE: OnceCell<Handle> = OnceCell::new();

    pub(crate) async fn connect_tcp(addr: &str) -> Result<impl Stream, Error> {
        Ok(from_tokio(TcpStream::connect(addr).await?))
    }

    pub async fn timeout(duration: Duration) {
        tokio_lib::time::delay_for(duration).await;
    }

    pub fn default_spawn<T>(task: T)
    where
        T: Future + Send + 'static,
    {
        with_handle(|h| {
            h.spawn(async move {
                task.await;
            });
        });
    }

    pub fn default_block_on<F: Future>(future: F) -> F::Output {
        with_runtime(|rt| rt.block_on(future))
    }

    fn create_default_runtime() -> (Handle, Runtime) {
        let runtime = Builder::new()
            .basic_scheduler()
            .enable_io()
            .enable_time()
            .build()
            .expect("Failed to build tokio runtime");
        let handle = runtime.handle().clone();
        (handle, runtime)
    }

    fn with_runtime<F: FnOnce(&mut tokio_lib::runtime::Runtime) -> R, R>(f: F) -> R {
        let mut rt = RUNTIME
            .get_or_init(|| {
                let (h, rt) = create_default_runtime();
                HANDLE.set(h).expect("Failed to set HANDLE");
                Mutex::new(rt)
            })
            .lock()
            .unwrap();
        f(&mut rt)
    }

    fn with_handle<F: FnOnce(tokio_lib::runtime::Handle)>(f: F) {
        let h = {
            HANDLE
                .get_or_init(|| {
                    let (h, rt) = create_default_runtime();
                    RUNTIME.set(Mutex::new(rt)).expect("Failed to set RUNTIME");
                    h
                })
                .clone()
        };
        f(h)
    }
}

// TODO does this cause memory leaks?
pub async fn never() {
    poll_fn::<(), _>(|_| Poll::Pending).await;
    unreachable!()
}
