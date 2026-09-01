use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    Command,
    Query,
    Speech,
    Action,
    Attack,
    Ambiguous,
}

impl IntentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Query => "query",
            Self::Speech => "speech",
            Self::Action => "action",
            Self::Attack => "attack",
            Self::Ambiguous => "ambiguous",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    pub kind: IntentKind,
    #[serde(default)]
    pub hex_target: Option<String>,
    pub actor_id: String,
    #[serde(default)]
    pub verb: Option<String>,
    #[serde(default)]
    pub object_ids: Vec<String>,
    pub raw: String,
    pub confidence: f32,
    #[serde(default)]
    pub notes: Option<String>,
}

impl Intent {
    pub fn command(raw: impl Into<String>, actor_id: impl Into<String>) -> Self {
        Self {
            kind: IntentKind::Command,
            hex_target: None,
            actor_id: actor_id.into(),
            verb: None,
            object_ids: vec![],
            raw: raw.into(),
            confidence: 1.0,
            notes: None,
        }
    }

    pub fn ambiguous(raw: impl Into<String>, actor_id: impl Into<String>) -> Self {
        Self {
            kind: IntentKind::Ambiguous,
            hex_target: None,
            actor_id: actor_id.into(),
            verb: None,
            object_ids: vec![],
            raw: raw.into(),
            confidence: 0.0,
            notes: Some("could not classify".into()),
        }
    }
}
