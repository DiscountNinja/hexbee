//! Legacy competency scoring (0–5 + d6 era). Contested Checks now use d20+attr.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use hexbee_provider::{complete_with_retry, DEFAULT_JSON_ATTEMPTS};
use hexbee_provider::LlmClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetencyScore {
    pub competency: u8,
    pub reason: String,
}

/// Heuristic fallback when the LLM is unavailable.
pub fn heuristic_competency(player_text: &str, action_label: &str) -> CompetencyScore {
    let text = player_text.trim();
    let words = text.split_whitespace().count();
    let lower = text.to_lowercase();

    let mut score: u8 = 0;
    if words >= 3 {
        score = 1;
    }
    if words >= 8 {
        score = 2;
    }
    if words >= 14 {
        score = 3;
    }

    let witty_markers = [
        "because",
        "remember",
        "you said",
        "gold",
        "password",
        "cloak",
        "tips",
        "warlord",
        "amulet",
        "goblin",
        "flirt",
        "charm",
        "respect",
        "honor",
        "sword",
        "knife",
        "storage",
        "aura",
    ];
    let hits = witty_markers
        .iter()
        .filter(|m| lower.contains(*m))
        .count();
    score = score.saturating_add(hits.min(2) as u8);

    if lower.contains('?') || lower.contains('!') {
        score = score.saturating_add(1);
    }

    // Bare keyword commands stay low.
    let bare = [
        "charm",
        "seduce",
        "impress",
        "persuade",
        "challenge",
        "talk",
        "fight",
    ];
    if words <= 2 && bare.iter().any(|b| lower.contains(b)) {
        score = 0;
    }

    let competency = score.min(5);
    debug!(%action_label, competency, words, hits, "heuristic competency");
    CompetencyScore {
        competency,
        reason: format!(
            "heuristic for '{action_label}': {words} words, leverage_hits={hits}"
        ),
    }
}

fn clamp_score(raw: u8) -> u8 {
    raw.min(5)
}

fn parse_competency_json(text: &str) -> Option<CompetencyScore> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let slice = &text[start..=end];
    #[derive(Deserialize)]
    struct Raw {
        competency: u8,
        #[serde(default)]
        reason: String,
    }
    let raw: Raw = serde_json::from_str(slice).ok()?;
    Some(CompetencyScore {
        competency: clamp_score(raw.competency),
        reason: if raw.reason.is_empty() {
            "llm score".into()
        } else {
            raw.reason
        },
    })
}

/// Score competency via LLM JSON, falling back to heuristic.
pub async fn score_competency(
    llm: &dyn LlmClient,
    player_text: &str,
    action_label: &str,
    npc_card_summary: &str,
) -> CompetencyScore {
    let system = r#"You score how witty and well-aimed a player's attempt is.
Return ONLY JSON: {"competency":0-5,"reason":"one short line"}
Rubric: 0-1 generic/off-target; 2-3 clear intent fitting the NPC; 4-5 witty, specific, uses known leverage.
Never decide success or failure. Never invent game state."#;
    let user = format!(
        "Action: {action_label}\nNPC: {npc_card_summary}\nPlayer said: {player_text}"
    );
    if let Some(score) =
        complete_with_retry(llm, system, &user, DEFAULT_JSON_ATTEMPTS, parse_competency_json).await
    {
        info!(competency = score.competency, reason = %score.reason, "competency from llm");
        score
    } else {
        info!("competency using heuristic after retries");
        heuristic_competency(player_text, action_label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hexbee_provider::FakeLlm;

    #[test]
    fn bare_command_low() {
        let s = heuristic_competency("charm sally", "charm Sally");
        assert_eq!(s.competency, 0);
    }

    #[test]
    fn witty_line_higher() {
        let s = heuristic_competency(
            "Sally, keep the gold tip because your smile already bought my respect!",
            "charm Sally",
        );
        assert!(s.competency >= 3);
    }

    #[tokio::test]
    async fn fake_llm_json() {
        let llm = FakeLlm::new([r#"{"competency":4,"reason":"clever leverage"}"#]);
        let s = score_competency(&llm, "long witty line about the cloak", "persuade", "Groll").await;
        assert_eq!(s.competency, 4);
    }
}
