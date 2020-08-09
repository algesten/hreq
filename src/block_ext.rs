//! Extension trait for `Future` to handle `.block()`

use crate::async_impl::AsyncRuntime;
use std::future::Future;

/// Blocks on a `Future` using the hreq configured [`AsyncRuntime`].
///
/// hreq is an async lib but every call can be turned sync by
/// using `.block()` in the same positions you would use `.await`.
/// Depending on the runtime, this might use the blocked thread
/// to drive the entire async operation.
///
/// This feature needs support by the current [`AsyncRuntime`].
/// The only runtime where this *doesn't* work is [`TokioShared`]
/// (see technical notes below).
///
/// # Single thread default
///
/// We want hreq to use a minimal amount of resources. By default
/// hreq uses a tokio runtime using the `rt-core` feature
/// which is a single threaded executor. The user can configure
/// hreq into a runtime with thread pools and work stealing.
///
/// The default runtime configuration is [`TokioSingle`] which
/// supports `.block()`.
///
/// # Usage
///
/// To synchronously make a request put `.block()` where `.await`
/// usually goes.
///
/// ```
/// use hreq::prelude::*;
///
/// let res = Request::get("https://www.google.com")
///     .call().block();
/// ```
///
/// Another way is to group a series of async actions with
/// `.await` and then run the entire thing with one `.block()`.
///
/// ```
/// use hreq::prelude::*;
///
/// let body_str = async {
///     let res = Request::get("https://www.google.com")
///         .call().await?;
///
///     let mut body = res.into_body();
///     body.read_to_string().await
/// }.block().unwrap();
///
/// assert_eq!(&body_str.as_bytes()[0..15], b"<!doctype html>");
/// ```
///
/// # Technical note
///
/// For tokio we need a direct reference to the [`Runtime`] to
/// reach the `block_on` function, something we don't get when
/// talking to a shared runtime via a [`Handle`].
///
/// This is not a problem with async-std where there is only one
/// shared runtime and we can always use the [`block_on`] function.
///
/// [`AsyncRuntime`]: enum.AsyncRuntime.html
/// [`TokioSingle`]: enum.AsyncRuntime.html#variant.TokioSingle
/// [`TokioShared`]: enum.AsyncRuntime.html#variant.TokioShared
/// [`Runtime`]: https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html
/// [`Handle`]: https://docs.rs/tokio/latest/tokio/runtime/struct.Handle.html
/// [`block_on`]: https://docs.rs/async-std/latest/async_std/task/fn.block_on.html
pub trait BlockExt {
    /// Block on a future to complete.
    fn block(self) -> Self::Output
    where
        Self: Future;
}

impl<F: Future> BlockExt for F {
    fn block(self) -> F::Output {
        AsyncRuntime::block_on(self)
    }
}
