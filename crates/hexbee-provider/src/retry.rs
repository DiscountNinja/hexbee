//! Retry helpers for tiny local models that occasionally emit junk.

use tracing::{info, warn};

use hexbee_core::metrics;

use crate::traits::LlmClient;

/// Default attempts for JSON completions (1 try + 2 retries).
pub const DEFAULT_JSON_ATTEMPTS: usize = 3;

/// Run `complete_json`, validating each reply. Retries with a short nudge on failure.
///
/// `accept` returns `Some(T)` when the raw model text is usable.
/// `retry_nudge` — optional extra sentence appended on retries (attempt 2+).
pub async fn complete_json_with_retry<T, F>(
    llm: &dyn LlmClient,
    system: &str,
    user: &str,
    max_attempts: usize,
    accept: F,
) -> Option<T>
where
    F: FnMut(&str) -> Option<T>,
{
    complete_json_with_retry_nudge(llm, system, user, max_attempts, None, accept).await
}

pub async fn complete_json_with_retry_nudge<T, F>(
    llm: &dyn LlmClient,
    system: &str,
    user: &str,
    max_attempts: usize,
    retry_nudge: Option<&str>,
    mut accept: F,
) -> Option<T>
where
    F: FnMut(&str) -> Option<T>,
{
    let attempts = max_attempts.max(1);
    let mut last_err: Option<String> = None;
    for attempt in 1..=attempts {
        let user_attempt = if attempt == 1 {
            user.to_string()
        } else {
            let base = format!(
                "{user}\n\nRetry {attempt}/{attempts}: previous reply was unusable. \
Output valid JSON only with real content — no placeholders, no prompt labels, no copying instructions."
            );
            if let Some(extra) = retry_nudge {
                format!("{base} {extra}")
            } else {
                base
            }
        };
        match llm.complete_json(system, &user_attempt).await {
            Ok(text) => {
                if let Some(ok) = accept(&text) {
                    if attempt > 1 {
                        info!(attempt, "llm json accepted after retry");
                    }
                    return Some(ok);
                }
                last_err = Some(format!("rejected attempt {attempt}"));
                warn!(attempt, sample = %truncate(&text, 80), "llm json rejected; retrying");
            }
            Err(e) => {
                last_err = Some(e.to_string());
                warn!(attempt, error = %e, "llm json call failed; retrying");
            }
        }
    }
    metrics::inc_llm_fallback();
    warn!(
        attempts,
        last = last_err.as_deref().unwrap_or("unknown"),
        "llm json exhausted retries"
    );
    None
}

/// Same as [`complete_json_with_retry`] but uses plain `complete` (for models without format:json).
pub async fn complete_with_retry<T, F>(
    llm: &dyn LlmClient,
    system: &str,
    user: &str,
    max_attempts: usize,
    mut accept: F,
) -> Option<T>
where
    F: FnMut(&str) -> Option<T>,
{
    let attempts = max_attempts.max(1);
    let mut last_err: Option<String> = None;
    for attempt in 1..=attempts {
        let user_attempt = if attempt == 1 {
            user.to_string()
        } else {
            format!(
                "{user}\n\nRetry {attempt}/{attempts}: previous reply was unusable. \
Output valid JSON only with real content — no placeholders, no prompt labels."
            )
        };
        match llm.complete(system, &user_attempt).await {
            Ok(text) => {
                if let Some(ok) = accept(&text) {
                    if attempt > 1 {
                        info!(attempt, "llm text accepted after retry");
                    }
                    return Some(ok);
                }
                last_err = Some(format!("rejected attempt {attempt}"));
                warn!(attempt, sample = %truncate(&text, 80), "llm text rejected; retrying");
            }
            Err(e) => {
                last_err = Some(e.to_string());
                warn!(attempt, error = %e, "llm call failed; retrying");
            }
        }
    }
    metrics::inc_llm_fallback();
    warn!(
        attempts,
        last = last_err.as_deref().unwrap_or("unknown"),
        "llm text exhausted retries"
    );
    None
}

fn truncate(s: &str, max: usize) -> String {
    let t = s.trim().replace('\n', " ");
    if t.chars().count() <= max {
        t
    } else {
        format!("{}…", t.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FakeLlm;

    #[tokio::test]
    async fn retries_until_accept() {
        let llm = FakeLlm::new([
            r#"{"line":"*does something* \"says this\""}"#,
            r#"{"line":"Scram, softskin."}"#,
        ]);
        let out = complete_json_with_retry(&llm, "sys", "user", 3, |text| {
            if text.contains("does something") {
                None
            } else if text.contains("Scram") {
                Some("Scram, softskin.".to_string())
            } else {
                None
            }
        })
        .await;
        assert_eq!(out.as_deref(), Some("Scram, softskin."));
    }

    #[tokio::test]
    async fn falls_back_when_all_bad() {
        let llm = FakeLlm::new([
            r#"{"line":"*does something* \"says this\""}"#,
            r#"{"line":"Present:"}"#,
            r#"{"line":"..."}"#,
        ]);
        let out = complete_json_with_retry(&llm, "sys", "user", 3, |_text| None::<String>).await;
        assert!(out.is_none());
    }
}
