use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub exits: Vec<String>,
    #[serde(default)]
    pub props: Vec<String>,
    #[serde(default)]
    pub flags: serde_json::Value,
}
