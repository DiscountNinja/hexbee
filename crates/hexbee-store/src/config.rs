use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityKind {
    Deny,
    Grant,
    Lock,
    Program,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantEntry {
    pub capability: String,
    pub kind: CapabilityKind,
    #[serde(default)]
    pub writable_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HexConfig {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub grants: Vec<GrantEntry>,
}

impl Default for HexConfig {
    fn default() -> Self {
        Self {
            id: "tavern".into(),
            name: "Tavern RPG".into(),
            version: "0.1.0".into(),
            grants: default_tavern_grants(),
        }
    }
}

pub fn default_tavern_grants() -> Vec<GrantEntry> {
    vec![
        GrantEntry {
            capability: "store.read".into(),
            kind: CapabilityKind::Grant,
            writable_paths: vec![],
        },
        GrantEntry {
            capability: "store.patch".into(),
            kind: CapabilityKind::Grant,
            writable_paths: vec![
                "state.*".into(),
                "runtime".into(),
                "runtime.*".into(),
                "actors.*.inventory".into(),
                "actors.*.flags".into(),
                "actors.*.stats".into(),
                "scenes.*.flags".into(),
            ],
        },
        GrantEntry {
            capability: "transcript.append".into(),
            kind: CapabilityKind::Grant,
            writable_paths: vec![],
        },
        GrantEntry {
            capability: "model.complete".into(),
            kind: CapabilityKind::Grant,
            writable_paths: vec![],
        },
        GrantEntry {
            capability: "proc.exec".into(),
            kind: CapabilityKind::Deny,
            writable_paths: vec![],
        },
        GrantEntry {
            capability: "mail.send".into(),
            kind: CapabilityKind::Deny,
            writable_paths: vec![],
        },
    ]
}
