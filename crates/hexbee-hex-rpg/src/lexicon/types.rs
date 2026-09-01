use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LexiconKind {
    Command,
    Verb,
    Actor,
    Item,
    Scene,
    Prop,
    Secret,
    Flag,
    Stat,
}

impl LexiconKind {
    /// Higher wins when two topics collide (secret over actor).
    pub fn topic_priority(self) -> u8 {
        match self {
            Self::Secret => 8,
            Self::Actor => 7,
            Self::Prop => 6,
            Self::Item => 5,
            Self::Scene => 4,
            Self::Flag => 3,
            Self::Stat => 2,
            Self::Verb | Self::Command => 0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Verb => "verb",
            Self::Actor => "actor",
            Self::Item => "item",
            Self::Scene => "scene",
            Self::Prop => "prop",
            Self::Secret => "secret",
            Self::Flag => "flag",
            Self::Stat => "stat",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "command" => Some(Self::Command),
            "verb" => Some(Self::Verb),
            "actor" => Some(Self::Actor),
            "item" => Some(Self::Item),
            "scene" => Some(Self::Scene),
            "prop" => Some(Self::Prop),
            "secret" => Some(Self::Secret),
            "flag" => Some(Self::Flag),
            "stat" => Some(Self::Stat),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerbClass {
    Social,
    Flavor,
    Act,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LexiconEntry {
    /// Substring matched on normalized player text.
    pub alias: String,
    /// Canonical id, e.g. `actor.goblin`, `verb.challenge_dewr`, `command.look`.
    pub id: String,
    pub kind: LexiconKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verb_class: Option<VerbClass>,
    /// Secret holder (`groll`) — aliases do not grant existence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
    #[serde(default)]
    pub runtime: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexiconHit {
    pub entry: LexiconEntry,
    pub alias_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexiconError {
    UnknownCanonicalId(String),
    UnknownKind(String),
    EmptyAlias,
    InvalidTarget(String),
}

impl std::fmt::Display for LexiconError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownCanonicalId(id) => write!(f, "unknown canonical id `{id}`"),
            Self::UnknownKind(k) => write!(f, "unknown kind `{k}`"),
            Self::EmptyAlias => write!(f, "alias cannot be empty"),
            Self::InvalidTarget(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Lexicon {
    pub phrase_aliases: HashMap<String, String>,
    pub token_spelling: HashMap<String, String>,
    kernel_entries: Vec<LexiconEntry>,
    hex_entries: Vec<LexiconEntry>,
    runtime_entries: Vec<LexiconEntry>,
    known_ids: HashSet<String>,
}

impl Lexicon {
    pub fn bundled() -> Self {
        crate::lexicon::load::LexiconDoc::bundled().into_lexicon()
    }

    pub(crate) fn new_from_parts(
        phrase_aliases: HashMap<String, String>,
        token_spelling: HashMap<String, String>,
        kernel_entries: Vec<LexiconEntry>,
        hex_entries: Vec<LexiconEntry>,
    ) -> Self {
        let mut lex = Self {
            phrase_aliases,
            token_spelling,
            kernel_entries,
            hex_entries,
            runtime_entries: Vec::new(),
            known_ids: HashSet::new(),
        };
        for id in crate::lexicon::load::kernel_canonical_ids() {
            lex.register_known_id(id);
        }
        for id in crate::lexicon::load::hex_canonical_ids() {
            lex.register_known_id(id);
        }
        lex
    }

    pub fn entries(&self) -> impl Iterator<Item = &LexiconEntry> {
        self.kernel_entries
            .iter()
            .chain(self.hex_entries.iter())
            .chain(self.runtime_entries.iter())
    }

    pub fn normalize<'a>(&self, input: &'a str) -> String {
        crate::lexicon::normalize::normalize_text(input, &self.phrase_aliases, &self.token_spelling)
    }

    pub fn match_hits(&self, normalized: &str) -> Vec<LexiconHit> {
        let mut hits = Vec::new();
        for entry in self.entries() {
            if entry.alias.is_empty() {
                continue;
            }
            if normalized.contains(&entry.alias) {
                hits.push(LexiconHit {
                    alias_len: entry.alias.len(),
                    entry: entry.clone(),
                });
            }
        }
        hits.sort_by(|a, b| b.alias_len.cmp(&a.alias_len));
        hits
    }

    /// Runtime operator alias — synonym for an existing id only.
    pub fn add_runtime_alias(
        &mut self,
        alias: &str,
        kind: LexiconKind,
        canonical_id: &str,
    ) -> Result<(), LexiconError> {
        let alias = alias.trim().to_lowercase();
        if alias.is_empty() {
            return Err(LexiconError::EmptyAlias);
        }
        if !self.known_ids.contains(canonical_id) {
            return Err(LexiconError::UnknownCanonicalId(canonical_id.into()));
        }
        if kind == LexiconKind::Secret && !canonical_id.starts_with("secret.") {
            return Err(LexiconError::InvalidTarget(
                "secret aliases must use secret.<id>".into(),
            ));
        }
        self.runtime_entries.retain(|e| e.alias != alias);
        self.runtime_entries.push(LexiconEntry {
            alias,
            id: canonical_id.into(),
            kind,
            verb_class: None,
            holder: None,
            runtime: true,
        });
        Ok(())
    }

    pub fn dump_text(&self) -> String {
        let mut lines = vec![
            "📚 Parser lexicon (normalize → match → rules; Purpose only on miss)".into(),
            String::new(),
            "Kinds: command, verb, actor, item, scene, prop, secret, flag, stat".into(),
            "Runtime aliases: alias add <word> <kind>.<id>  (existing ids only)".into(),
            String::new(),
        ];
        let mut sections: Vec<(LexiconKind, Vec<String>)> = LexiconKind::ALL
            .iter()
            .map(|k| (*k, Vec::new()))
            .collect();
        for entry in self.entries() {
            if let Some(sec) = sections.iter_mut().find(|(k, _)| *k == entry.kind) {
                let tag = if entry.runtime { " [runtime]" } else { "" };
                let holder = entry
                    .holder
                    .as_deref()
                    .map(|h| format!(" holder={h}"))
                    .unwrap_or_default();
                let verb = entry
                    .verb_class
                    .map(|v| format!(" {:?}", v))
                    .unwrap_or_default();
                sec.1.push(format!(
                    "  {} → {} ({}{holder}{verb}){tag}",
                    entry.alias,
                    entry.id,
                    entry.kind.as_str(),
                ));
            }
        }
        for (kind, rows) in sections {
            if rows.is_empty() {
                continue;
            }
            lines.push(format!("{}:", kind.as_str()));
            lines.extend(rows);
            lines.push(String::new());
        }
        if !self.phrase_aliases.is_empty() || !self.token_spelling.is_empty() {
            lines.push("normalize phrase aliases:".into());
            let mut keys: Vec<_> = self.phrase_aliases.keys().collect();
            keys.sort();
            for k in keys {
                lines.push(format!("  {k} → {}", self.phrase_aliases[k]));
            }
            lines.push(String::new());
            lines.push("normalize token spelling:".into());
            let mut keys: Vec<_> = self.token_spelling.keys().collect();
            keys.sort();
            for k in keys {
                lines.push(format!("  {k} → {}", self.token_spelling[k]));
            }
        }
        lines.join("\n")
    }

    pub(crate) fn push_hex_entry(&mut self, entry: LexiconEntry) {
        self.register_known_id(&entry.id);
        if let Some(holder) = &entry.holder {
            self.register_known_id(&format!("actor.{holder}"));
        }
        self.hex_entries.push(entry);
    }

    pub(crate) fn register_known_id(&mut self, id: &str) {
        self.known_ids.insert(id.to_string());
    }
}

impl LexiconKind {
    pub const ALL: [LexiconKind; 9] = [
        Self::Command,
        Self::Verb,
        Self::Actor,
        Self::Item,
        Self::Scene,
        Self::Prop,
        Self::Secret,
        Self::Flag,
        Self::Stat,
    ];
}