use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorSheet {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub player_controllable: bool,
    pub voice: String,
    #[serde(default)]
    pub traits: Vec<String>,
    #[serde(default)]
    pub stats: serde_json::Value,
    #[serde(default)]
    pub inventory: Vec<String>,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub known_facts: Vec<String>,
    #[serde(default)]
    pub relationships: serde_json::Value,
    /// Legacy card fields for organ prompts.
    #[serde(default)]
    pub temperament: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub weapon: String,
    #[serde(default)]
    pub likes: Vec<String>,
    #[serde(default)]
    pub taboos: Vec<String>,
}
