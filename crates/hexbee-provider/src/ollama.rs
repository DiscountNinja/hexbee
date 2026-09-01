use serde::Deserialize;
use tracing::{debug, info, warn};

use hexbee_core::metrics;

use crate::traits::{BoxFuture, LlmClient, LlmError, LlmResult};

#[derive(Debug, Clone)]
pub struct OllamaClient {
    base_url: String,
    model: String,
    http: reqwest::Client,
}

impl OllamaClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            http: reqwest::Client::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: Option<ChatMessage>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

impl LlmClient for OllamaClient {
    fn complete<'a>(
        &'a self,
        system: &'a str,
        user: &'a str,
    ) -> BoxFuture<'a, LlmResult<String>> {
        self.chat(system, user, false)
    }

    fn complete_json<'a>(
        &'a self,
        system: &'a str,
        user: &'a str,
    ) -> BoxFuture<'a, LlmResult<String>> {
        self.chat(system, user, true)
    }
}

impl OllamaClient {
    fn chat<'a>(
        &'a self,
        system: &'a str,
        user: &'a str,
        json_format: bool,
    ) -> BoxFuture<'a, LlmResult<String>> {
        Box::pin(async move {
            metrics::inc_llm_call();
            let url = format!("{}/api/chat", self.base_url);
            debug!(
                model = %self.model,
                system_len = system.len(),
                user_len = user.len(),
                json_format,
                "ollama request"
            );
            let mut body = serde_json::json!({
                "model": self.model,
                "stream": false,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
                "options": {
                    "num_predict": if json_format { 120 } else { 160 },
                    "temperature": 0.4,
                }
            });
            if json_format {
                body["format"] = serde_json::json!("json");
            }
            let resp = match self.http.post(&url).json(&body).send().await {
                Ok(r) => r,
                Err(e) => {
                    metrics::inc_llm_error();
                    warn!(error = %e, "ollama http send failed");
                    return Err(LlmError::Http(e.to_string()));
                }
            };
            let status = resp.status();
            let text = match resp.text().await {
                Ok(t) => t,
                Err(e) => {
                    metrics::inc_llm_error();
                    warn!(error = %e, "ollama body read failed");
                    return Err(LlmError::Http(e.to_string()));
                }
            };
            if !status.is_success() {
                metrics::inc_llm_error();
                warn!(%status, body = %text, "ollama non-success status");
                return Err(LlmError::Http(format!("status {status}: {text}")));
            }
            let parsed: ChatResponse = match serde_json::from_str(&text) {
                Ok(p) => p,
                Err(e) => {
                    metrics::inc_llm_error();
                    warn!(error = %e, body = %text, "ollama parse failed");
                    return Err(LlmError::Response(format!("{e}; body={text}")));
                }
            };
            if let Some(err) = parsed.error {
                metrics::inc_llm_error();
                warn!(%err, "ollama API error field");
                return Err(LlmError::Response(err));
            }
            match parsed.message.map(|m| m.content) {
                Some(content) => {
                    metrics::inc_llm_success();
                    info!(chars = content.len(), json_format, "ollama success");
                    Ok(content)
                }
                None => {
                    metrics::inc_llm_error();
                    warn!("ollama missing message");
                    Err(LlmError::Response("missing message".into()))
                }
            }
        })
    }
}
