//! NPC Actor organ — speak in character from a sheet (wired after legality gate).
#![allow(dead_code)]

use hexbee_provider::LlmClient;

pub use crate::organs::dialogue::{generate_npc_line, DialogueRequest, DialogueResponse};

pub struct ActorOrgan;

impl ActorOrgan {
    pub async fn speak(
        llm: Option<&dyn LlmClient>,
        req: &DialogueRequest,
    ) -> DialogueResponse {
        match llm {
            Some(client) => generate_npc_line(client, req).await,
            None => DialogueResponse {
                line: String::new(),
            },
        }
    }
}
