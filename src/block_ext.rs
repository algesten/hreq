use crate::async_impl::AsyncRuntime;
use std::future::Future;

pub trait BlockExt {
    fn block(self) -> Self::Output
    where
        Self: Future;
}

impl<F: Future> BlockExt for F {
    fn block(self) -> F::Output {
        AsyncRuntime::current().block_on(self)
    }
}
