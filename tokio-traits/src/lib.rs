//! Re-export tokio async traits to be able to write code that ports
//! between future `AsyncRead`/`AsyncWrite` to tokio without bringing
//! in all of tokio.
//!
//! This crate is not maintained by the tokio team.
pub use tokio::io::{AsyncRead as TokioAsyncRead, AsyncWrite as TokioAsyncWrite};
