use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::types::{Lexicon, LexiconEntry, LexiconKind, VerbClass};

#[derive(Debug, Clone, Deserialize)]
pub struct LexiconDoc {
    #[serde(default)]
    pub normalize: NormalizeTables,
    #[serde(default)]
    pub entries: Vec<LexiconEntryRaw>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NormalizeTables {
    #[serde(default)]
    pub phrase_aliases: HashMap<String, String>,
    #[serde(default)]
    pub token_spelling: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LexiconEntryRaw {
    pub alias: String,
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub verb_class: Option<String>,
    #[serde(default)]
    pub holder: Option<String>,
}

impl LexiconDoc {
    pub fn bundled() -> Self {
        let raw = include_str!("../../campaign/lexicon.json");
        serde_json::from_str(raw).expect("bundled lexicon.json")
    }

    /// Load hex lexicon from a campaign directory (not yet wired on the hot path).
    #[allow(dead_code)]
    pub fn load_path(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| e.to_string())
    }

    pub fn into_lexicon(self) -> Lexicon {
        let mut lex = Lexicon::new_from_parts(
            self.normalize.phrase_aliases,
            self.normalize.token_spelling,
            kernel_defaults(),
            Vec::new(),
        );
        for entry in self.entries {
            if let Some(e) = entry.into_entry() {
                lex.push_hex_entry(e);
            }
        }
        lex
    }
}

impl LexiconEntryRaw {
    fn into_entry(self) -> Option<LexiconEntry> {
        let kind = LexiconKind::parse(&self.kind)?;
        let verb_class = self.verb_class.as_deref().and_then(|s| match s {
            "social" => Some(VerbClass::Social),
            "flavor" => Some(VerbClass::Flavor),
            "act" => Some(VerbClass::Act),
            _ => None,
        });
        Some(LexiconEntry {
            alias: self.alias.trim().to_lowercase(),
            id: self.id,
            kind,
            verb_class,
            holder: self.holder,
            runtime: false,
        })
    }
}

fn kernel_defaults() -> Vec<LexiconEntry> {
    vec![
        entry("help", "command.help", LexiconKind::Command, None, None),
        entry("inventory", "command.inventory", LexiconKind::Command, None, None),
        entry("inv", "command.inventory", LexiconKind::Command, None, None),
        entry("look", "command.look", LexiconKind::Command, None, None),
        entry("look around", "command.look", LexiconKind::Command, None, None),
        entry("leave", "command.leave", LexiconKind::Command, None, None),
        entry("quit tavern", "command.leave", LexiconKind::Command, None, None),
        entry("dict", "command.dict", LexiconKind::Command, None, None),
    ]
}

pub(crate) fn kernel_canonical_ids() -> &'static [&'static str] {
    &[
        "command.help",
        "command.inventory",
        "command.look",
        "command.leave",
        "command.dict",
        "command.pay_scam",
        "command.charm_sally",
    ]
}

pub(crate) fn hex_canonical_ids() -> &'static [&'static str] {
    &[
        "actor.dewr",
        "actor.sally",
        "actor.groll",
        "actor.dremis",
        "actor.goblin",
        "actor.patron",
        "verb.challenge_dewr",
        "verb.call_out_scam",
        "verb.charm_sally",
        "verb.persuade_groll",
        "verb.impress_dremis",
        "verb.talk",
        "secret.groll_storage_password",
        "secret.sally_charm",
        "secret.dewr_intimidate",
        "secret.dewr_scam",
        "secret.dremis_amulet",
        "scene.outside",
        "scene.main_hall",
        "scene.storage_room",
        "prop.ugly_hat",
        "flag.sworn_silent",
        "stat.hp",
        "stat.gold",
        "stat.inventory",
    ]
}

fn entry(
    alias: &str,
    id: &str,
    kind: LexiconKind,
    verb_class: Option<VerbClass>,
    holder: Option<&str>,
) -> LexiconEntry {
    LexiconEntry {
        alias: alias.into(),
        id: id.into(),
        kind,
        verb_class,
        holder: holder.map(str::to_string),
        runtime: false,
    }
}
