//! Purpose organ — classify Talk | Act | Query | Ambiguous when rules miss.

use hexbee_provider::{complete_json_with_retry, LlmClient, DEFAULT_JSON_ATTEMPTS};
use serde::{Deserialize, Serialize};

use crate::rules::{RuleHit, TurnMode};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PurposeResult {
    pub mode: TurnMode,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub summary: Option<String>,
    /// When Act/social: optional check / action hint (e.g. "challenge_dewr").
    #[serde(default)]
    pub action_hint: Option<String>,
    /// True when speech is pressure / convince / intimidate (Check-shaped).
    #[serde(default)]
    pub social_act: bool,
    /// 0.0–1.0 from Purpose organ when LLM classifies; None when rules/lexicon routed.
    #[serde(default)]
    pub confidence: Option<f32>,
}

impl PurposeResult {
    pub fn ambiguous(raw: &str) -> Self {
        Self {
            mode: TurnMode::Ambiguous,
            targets: vec![],
            summary: Some(raw.into()),
            action_hint: None,
            social_act: false,
            confidence: None,
        }
    }

    pub fn from_rule(hit: &RuleHit) -> Self {
        Self {
            mode: hit.mode,
            targets: hit.targets.clone(),
            summary: hit.summary.clone(),
            action_hint: hit.action_hint.clone(),
            social_act: hit.social_act,
            confidence: Some(1.0),
        }
    }
}

/// LLM Purpose when the rule table misses. Validated / reprompted.
pub async fn purpose_classify(
    llm: &dyn LlmClient,
    input: &str,
    location: &str,
    present: &str,
    memory: &str,
) -> PurposeResult {
    let system = "You classify a TTRPG player line. Return ONLY JSON:\n\
{\"mode\":\"talk|act|query|ambiguous\",\"targets\":[],\"summary\":\"short\",\
\"action_hint\":\"optional catalog-like id e.g. challenge_dewr|charm_sally|go|look|talk\",\
\"social_act\":false,\"confidence\":0-5}\n\
Modes: talk=banter/speech only; act=physical or intentional world change; \
query=ask about inventory/status/who/where; ambiguous=unclear.\n\
social_act=true when speech tries to persuade/intimidate/charm/pressure someone.\n\
Never invent items or absent NPCs. Prefer ambiguous over wrong act.";
    let user = format!(
        "Location: {location}\nPresent: {present}\nMemory: {memory}\nPlayer: {input}"
    );
    let parsed = complete_json_with_retry(llm, system, &user, DEFAULT_JSON_ATTEMPTS, |text| {
        accept_purpose(text)
    })
    .await;
    match parsed {
        Some(v) => v,
        None => {
            tracing::warn!("purpose organ failed");
            PurposeResult::ambiguous(input)
        }
    }
}

fn accept_purpose(text: &str) -> Option<PurposeResult> {
    let v: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    let mode_raw = v.get("mode")?.as_str()?;
    let mode = TurnMode::parse(mode_raw)?;
    // Soft validate via kernel-shaped kinds.
    let _ = hexbee_kernel::Validator::validate_intent_kind(match mode {
        TurnMode::Talk => "speech",
        TurnMode::Act => "action",
        TurnMode::Query => "query",
        TurnMode::Ambiguous => "ambiguous",
    });
    let targets = v
        .get("targets")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_lowercase()))
                .collect()
        })
        .unwrap_or_default();
    let summary = v
        .get("summary")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let action_hint = v
        .get("action_hint")
        .and_then(|s| s.as_str())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let social_act = v
        .get("social_act")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let confidence = v
        .get("confidence")
        .and_then(|c| c.as_f64())
        .map(|c| (c / 5.0).clamp(0.0, 1.0) as f32);
    Some(PurposeResult {
        mode,
        targets,
        summary,
        action_hint,
        social_act,
        confidence,
    })
}

/// Heuristic Purpose when no LLM (tests / offline).
pub fn purpose_heuristic(input: &str) -> PurposeResult {
    if let Some(hit) = crate::rules::match_rules(input) {
        return PurposeResult::from_rule(&hit);
    }
    let lower = crate::slang::normalize(input);
    if looks_query(&lower) {
        return PurposeResult {
            mode: TurnMode::Query,
            targets: vec![],
            summary: Some("query".into()),
            action_hint: None,
            social_act: false,
            confidence: Some(0.85),
        };
    }
    if looks_social_pressure(&lower) {
        let hint = if lower.contains("charm") || lower.contains("flirt") {
            "charm_sally"
        } else if lower.contains("persuade") || lower.contains("password") {
            "persuade_groll"
        } else if lower.contains("scam") || lower.contains("not pay") || lower.contains("refuse") {
            "call_out_scam"
        } else if lower.contains("impress") {
            "impress_dremis"
        } else {
            "challenge_dewr"
        };
        return PurposeResult {
            mode: TurnMode::Talk,
            targets: crate::rules::extract_npc_targets(&lower),
            summary: Some("social pressure".into()),
            action_hint: Some(hint.into()),
            social_act: false,
            confidence: Some(0.8),
        };
    }
    if looks_talk(&lower) {
        return PurposeResult {
            mode: TurnMode::Talk,
            targets: crate::rules::extract_npc_targets(&lower),
            summary: Some("talk".into()),
            action_hint: Some("talk".into()),
            social_act: false,
            confidence: Some(0.75),
        };
    }
    if looks_act(&lower) {
        return PurposeResult {
            mode: TurnMode::Act,
            targets: crate::rules::extract_npc_targets(&lower),
            summary: Some("act".into()),
            action_hint: None,
            social_act: false,
            confidence: Some(0.7),
        };
    }
    PurposeResult::ambiguous(input)
}

fn looks_query(lower: &str) -> bool {
    if lower.starts_with("what do you")
        || lower.starts_with("what are you")
        || lower.starts_with("why do you")
        || lower.starts_with("who are you")
        || lower.starts_with("how are you")
    {
        return false;
    }
    lower.starts_with("where ")
        || lower.starts_with("who ")
        || lower.starts_with("what ")
        || lower.starts_with("are there")
        || lower.starts_with("is there")
        || lower.contains("inventory")
        || lower.contains("do i have")
        || lower == "sheet"
        || lower == "status"
        || lower == "skills"
}

fn looks_talk(lower: &str) -> bool {
    lower.starts_with("say ")
        || lower.starts_with("tell ")
        || lower.starts_with("ask ")
        || lower.contains("talk to")
        || lower.contains("speak to")
        || lower.starts_with('"')
        || lower.starts_with('\u{201c}')
        || matches!(
            lower,
            "hello" | "hi" | "hey" | "howdy" | "greetings" | "talk"
        )
        || lower.starts_with("what do you")
        || lower.starts_with("can you ")
}

fn looks_act(lower: &str) -> bool {
    lower.contains("attack")
        || lower.contains("fight")
        || lower.contains("go ")
        || lower.contains("enter")
        || lower.contains("leave")
        || lower.contains("look")
        || lower.contains("pay")
        || lower.contains("buy")
        || lower.contains("sleep")
        || lower.contains("sneak")
        || lower.starts_with("i ")
}

fn looks_social_pressure(lower: &str) -> bool {
    lower.contains("intimidate")
        || lower.contains("threaten")
        || lower.contains("convince")
        || lower.contains("persuade")
        || lower.contains("talk into")
        || lower.contains("charm")
        || lower.contains("flirt")
        || lower.contains("impress")
        || lower.contains("creep out")
        || ((lower.contains("not pay") || lower.contains("won't pay") || lower.contains("refuse"))
            && (lower.contains("scam") || lower.contains("tax") || lower.contains("toll") || crate::slang::mentions_npc(lower, "goblin")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_intimidate_is_talk() {
        let p = purpose_heuristic("I intimidate the goblin — move aside");
        assert_eq!(p.mode, TurnMode::Talk);
        assert!(!p.social_act);
        assert_eq!(p.action_hint.as_deref(), Some("challenge_dewr"));
    }

    #[test]
    fn heuristic_inventory_is_query() {
        let p = purpose_heuristic("what's in my inventory?");
        assert_eq!(p.mode, TurnMode::Query);
    }

    #[test]
    fn accept_purpose_json() {
        let raw = r#"{"mode":"talk","targets":["sally"],"summary":"hi","social_act":false}"#;
        let p = accept_purpose(raw).expect("parse");
        assert_eq!(p.mode, TurnMode::Talk);
        assert_eq!(p.targets, vec!["sally"]);
    }
}
