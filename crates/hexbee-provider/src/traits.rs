use std::future::Future;
use std::pin::Pin;

use thiserror::Error;

pub type LlmResult<T> = Result<T, LlmError>;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("llm unavailable: {0}")]
    Unavailable(String),
    #[error("llm response error: {0}")]
    Response(String),
    #[error("http error: {0}")]
    Http(String),
}

/// Minimal chat completion interface.
pub trait LlmClient: Send + Sync {
    fn complete<'a>(
        &'a self,
        system: &'a str,
        user: &'a str,
    ) -> BoxFuture<'a, LlmResult<String>>;

    /// Prefer JSON-shaped replies (Ollama `format: json`). Default: same as [`complete`].
    fn complete_json<'a>(
        &'a self,
        system: &'a str,
        user: &'a str,
    ) -> BoxFuture<'a, LlmResult<String>> {
        self.complete(system, user)
    }
}
