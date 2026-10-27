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

/// Whether a language model endpoint is reachable and usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmStatus {
    Online { model: String },
    Offline { detail: String },
}

impl LlmStatus {
    pub fn is_online(&self) -> bool {
        matches!(self, Self::Online { .. })
    }

    /// Title suffix for the framed board: empty when online, ` <ai offline>` otherwise.
    pub fn title_suffix(&self) -> &'static str {
        if self.is_online() {
            ""
        } else {
            " <ai offline>"
        }
    }
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

    /// Lightweight reachability check — no chat generation.
    /// Default: offline (providers must override).
    fn probe<'a>(&'a self) -> BoxFuture<'a, LlmStatus> {
        Box::pin(async move {
            LlmStatus::Offline {
                detail: "provider does not support probe".into(),
            }
        })
    }
}
