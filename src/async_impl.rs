use crate::Error;
use crate::Stream;
use futures_util::future::poll_fn;
use std::future::Future;
use std::sync::Mutex;
use std::task::Poll;
use std::time::Duration;

use once_cell::sync::Lazy;

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

#[derive(Clone, Copy)]
pub enum AsyncRuntime {
    #[cfg(feature = "async-std")]
    AsyncStd,
    #[cfg(feature = "tokio")]
    TokioDefault,
    #[cfg(feature = "tokio")]
    TokioShared,
}

impl AsyncRuntime {
    pub fn set_default(rt: Self) {
        let mut ptr = CURRENT_RUNTIME.lock().unwrap();
        *ptr = rt;
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
                AsyncRuntime::TokioDefault => Ok(Either::B(async_tokio::connect_tcp(addr).await?)),
                AsyncRuntime::TokioShared => Ok(Either::B(async_tokio::connect_tcp(addr).await?)),
            }
        }
        #[cfg(all(feature = "async-std", not(feature = "tokio")))]
        match self {
            AsyncRuntime::AsyncStd => Ok(async_std::connect_tcp(addr).await?),
        }
        #[cfg(all(feature = "tokio", not(feature = "async-std")))]
        match self {
            AsyncRuntime::TokioDefault => Ok(async_tokio::connect_tcp(addr).await?),
            AsyncRuntime::TokioShared => Ok(async_tokio::connect_tcp(addr).await?),
        }
    }

    pub(crate) async fn timeout(self, duration: Duration) {
        match self {
            #[cfg(feature = "async-std")]
            AsyncRuntime::AsyncStd => async_std::timeout(duration).await,
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioDefault => async_tokio::timeout(duration).await,
            #[cfg(feature = "tokio")]
            AsyncRuntime::TokioShared => async_tokio::timeout(duration).await,
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
        }
    }
}

#[cfg(feature = "async-std")]
pub mod async_std {
    use super::*;
    use async_std_lib::net::TcpStream;
    use async_std_lib::task;

    impl Stream for TcpStream {}

    pub async fn connect_tcp(addr: &str) -> Result<impl Stream, Error> {
        Ok(TcpStream::connect(addr).await?)
    }

    pub fn spawn<T>(task: T)
    where
        T: Future + Send + 'static,
    {
        async_std_lib::task::spawn(async move {
            task.await;
        });
    }

    pub async fn timeout(duration: Duration) {
        async_std_lib::future::timeout(duration, never()).await.ok();
    }

    pub fn block_on<F: Future>(future: F) -> F::Output {
        task::block_on(future)
    }
}

#[cfg(feature = "tokio")]
pub mod async_tokio {
    use super::*;
    use crate::tokio::from_tokio;
    use once_cell::sync::OnceCell;
    use std::sync::Mutex;
    use tokio_lib::net::TcpStream;
    pub use tokio_lib::runtime::Handle;
    use tokio_lib::runtime::{Builder, Runtime};

    static RUNTIME: OnceCell<Mutex<Runtime>> = OnceCell::new();
    static HANDLE: OnceCell<Handle> = OnceCell::new();

    pub async fn connect_tcp(addr: &str) -> Result<impl Stream, Error> {
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
