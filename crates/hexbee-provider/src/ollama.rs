use serde::Deserialize;
use tracing::{debug, info, warn};

use hexbee_core::metrics;

use crate::traits::{BoxFuture, LlmClient, LlmError, LlmResult, LlmStatus};

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

    fn model_listed(configured: &str, names: &[String]) -> bool {
        let want = configured.trim().to_lowercase();
        if want.is_empty() {
            return false;
        }
        names.iter().any(|n| {
            let have = n.trim().to_lowercase();
            have == want
                || have.starts_with(&format!("{want}:"))
                || want.starts_with(&format!("{have}:"))
        })
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

#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Option<Vec<TagModel>>,
}

#[derive(Debug, Deserialize)]
struct TagModel {
    name: Option<String>,
    model: Option<String>,
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

    fn probe<'a>(&'a self) -> BoxFuture<'a, LlmStatus> {
        Box::pin(async move {
            let url = format!("{}/api/tags", self.base_url);
            debug!(%url, model = %self.model, "ollama probe");
            let resp = match self
                .http
                .get(&url)
                .timeout(std::time::Duration::from_secs(2))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!(error = %e, "ollama probe unreachable");
                    return LlmStatus::Offline {
                        detail: format!("unreachable: {e}"),
                    };
                }
            };
            if !resp.status().is_success() {
                let status = resp.status();
                warn!(%status, "ollama probe non-success");
                return LlmStatus::Offline {
                    detail: format!("status {status}"),
                };
            }
            let text = match resp.text().await {
                Ok(t) => t,
                Err(e) => {
                    return LlmStatus::Offline {
                        detail: format!("body read failed: {e}"),
                    };
                }
            };
            let parsed: TagsResponse = match serde_json::from_str(&text) {
                Ok(p) => p,
                Err(e) => {
                    return LlmStatus::Offline {
                        detail: format!("tags parse failed: {e}"),
                    };
                }
            };
            let names: Vec<String> = parsed
                .models
                .unwrap_or_default()
                .into_iter()
                .filter_map(|m| m.name.or(m.model))
                .collect();
            if names.is_empty() {
                return LlmStatus::Offline {
                    detail: "no models pulled".into(),
                };
            }
            if !Self::model_listed(&self.model, &names) {
                warn!(model = %self.model, available = ?names, "configured model not listed");
                return LlmStatus::Offline {
                    detail: format!("model '{}' not found", self.model),
                };
            }
            info!(model = %self.model, "ollama probe online");
            LlmStatus::Online {
                model: self.model.clone(),
            }
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_listed_matches_tag_variants() {
        let names = vec!["qwen2.5:0.5b".into(), "llama3:latest".into()];
        assert!(OllamaClient::model_listed("qwen2.5:0.5b", &names));
        assert!(OllamaClient::model_listed("qwen2.5", &names));
        assert!(!OllamaClient::model_listed("mistral", &names));
    }
}
