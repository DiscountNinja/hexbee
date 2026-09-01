//! Judge organ — capability / legitimacy score + tags, no prose (legacy competency path).
#![allow(dead_code)]

use hexbee_provider::LlmClient;

pub use crate::organs::competency::{heuristic_competency, score_competency, CompetencyScore};

pub struct JudgeOrgan;

impl JudgeOrgan {
    pub async fn score(
        llm: Option<&dyn LlmClient>,
        player_text: &str,
        context: &str,
        npc_summary: &str,
    ) -> CompetencyScore {
        match llm {
            Some(client) => {
                score_competency(client, player_text, context, npc_summary).await
            }
            None => heuristic_competency(player_text, context),
        }
    }

    pub fn heuristic(player_text: &str, context: &str) -> CompetencyScore {
        heuristic_competency(player_text, context)
    }
}
