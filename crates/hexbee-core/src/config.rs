use serde::{Deserialize, Serialize};

/// Runtime configuration shared across crates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Ollama base URL, e.g. `http://127.0.0.1:11434`.
    pub ollama_base_url: String,
    /// Model tag for the local reasoning agent.
    pub ollama_model: String,
    /// Optional RNG seed for deterministic play / tests.
    pub rng_seed: Option<u64>,
    /// Log file path (informational; init uses env / default).
    pub log_file: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ollama_base_url: "http://127.0.0.1:11434".to_string(),
            ollama_model: std::env::var("HEXBEE_MODEL")
                .unwrap_or_else(|_| "qwen2.5:0.5b".to_string()),
            rng_seed: std::env::var("HEXBEE_SEED")
                .ok()
                .and_then(|s| s.parse().ok()),
            log_file: std::env::var("HEXBEE_LOG_FILE")
                .unwrap_or_else(|_| "logs/hexbee.log".to_string()),
        }
    }
}
