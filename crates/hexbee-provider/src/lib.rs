//! LLM provider trait and implementations.

mod fake;
mod ollama;
mod retry;
mod traits;

pub use fake::FakeLlm;
pub use ollama::OllamaClient;
pub use retry::{
    complete_json_with_retry, complete_json_with_retry_nudge, complete_with_retry,
    DEFAULT_JSON_ATTEMPTS,
};
pub use traits::{LlmClient, LlmError, LlmResult, LlmStatus};
