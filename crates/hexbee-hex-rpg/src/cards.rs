use serde::{Deserialize, Serialize};

use crate::state::NpcId;
use crate::weapons::Weapon;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterCard {
    pub id: String,
    pub name: String,
    pub temperament: String,
    pub description: String,
    pub voice: String,
    pub weapon: String,
    pub likes: Vec<String>,
    pub taboos: Vec<String>,
}

impl CharacterCard {
    pub fn summary(&self) -> String {
        format!(
            "{} — {}. {}. Voice: {}. Weapon: {}.",
            self.name, self.temperament, self.description, self.voice, self.weapon
        )
    }

    /// Short tag for tiny-model prompts (avoids card regurgitation).
    pub fn voice_tag(&self) -> String {
        let temper = self
            .temperament
            .split(',')
            .next()
            .unwrap_or(self.temperament.as_str())
            .trim();
        format!("{}: {}, {}", self.name, temper, self.voice)
    }

    /// Name plus one-line look — for scene presence prompts.
    pub fn description_tag(&self) -> String {
        let desc = first_sentence(self.description.trim(), 100);
        if desc.is_empty() {
            self.voice_tag()
        } else {
            format!("{} — {desc}", self.name)
        }
    }

    /// Full look line for the CLI Present roster (no mid-word cuts).
    pub fn present_line(&self) -> String {
        let icon = self
            .npc_id()
            .map(|id| id.icon())
            .unwrap_or_else(|| crate::organs::npc_display_icon(&self.id));
        let desc = self.description.trim();
        if desc.is_empty() {
            format!("· {icon} {}", self.name)
        } else {
            format!("· {icon} {} — {desc}", self.name)
        }
    }

    pub fn npc_id(&self) -> Option<NpcId> {
        NpcId::parse(&self.id).or_else(|| NpcId::parse(&self.name))
    }

    /// Build a talk-only ambient card (no quest id collision).
    pub fn ambient(
        id: impl Into<String>,
        name: impl Into<String>,
        temperament: impl Into<String>,
        description: impl Into<String>,
        voice: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            temperament: temperament.into(),
            description: description.into(),
            voice: voice.into(),
            weapon: "none".into(),
            likes: vec![],
            taboos: vec!["quest secrets".into()],
        }
    }

    pub fn weapon_enum(&self) -> Weapon {
        match self.weapon.to_lowercase().as_str() {
            "knife" => Weapon::Knife,
            "mace" => Weapon::Mace,
            "battle axe" | "battleaxe" | "axe" => Weapon::BattleAxe,
            "royal sword" | "sword" => Weapon::RoyalSword,
            _ => Weapon::None,
        }
    }
}

const DEWR: &str = r#"
id = "dewr"
name = "Dewr"
temperament = "snarky, unhelpful, picks fights but backs down when challenged"
description = "A goblin loitering at the tavern door, palm out, knife at his belt. He wants coin before anyone passes."
voice = "short, mocking, crude"
weapon = "knife"
likes = ["trouble", "insults", "easy coin"]
taboos = ["admitting fear unless challenged", "admitting his toll is a lie unless caught"]
"#;

const SALLY: &str = r#"
id = "sally"
name = "Sally"
temperament = "flirty, warm to the player, works tips"
description = "Attractive tavern server working the tables. Unarmed. Warm to charm and coin."
voice = "playful, teasing"
weapon = "none"
likes = ["tips", "compliments", "charm"]
taboos = ["violence"]
"#;

const GROLL: &str = r#"
id = "groll"
name = "Groll"
temperament = "rude, guarded"
description = "A cloaked dragon-newt patron in the corner. Hard to see. Speaks little."
voice = "gravelly, dismissive"
weapon = "mace"
likes = ["being left alone", "strong drinks"]
taboos = ["revealing his face", "volunteering secrets unprompted"]
"#;

const DREMIS: &str = r#"
id = "dremis"
name = "Dremis"
temperament = "dark, volatile, quick to anger and quick to forgive"
description = "A black-and-red winged knight wreathed in smoke, fur cape, glowing red eyes. Presence like a weight on the chest."
voice = "intense, clipped, dangerous"
weapon = "battle axe"
likes = ["respect", "courage"]
taboos = ["insults", "mockery"]
"#;

pub fn load_builtin_cards() -> Vec<CharacterCard> {
    [DEWR, SALLY, GROLL, DREMIS]
        .into_iter()
        .map(|raw| toml::from_str(raw).expect("builtin card must parse"))
        .collect()
}

pub fn card_for(cards: &[CharacterCard], id: NpcId) -> Option<&CharacterCard> {
    cards.iter().find(|c| c.npc_id() == Some(id))
}

/// Prefer a full sentence; otherwise cut on a word boundary (never mid-word).
fn first_sentence(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    if let Some(end) = text.find(['.', '!', '?']) {
        let sentence: String = text[..=end].trim().to_string();
        if sentence.chars().count() <= max_chars {
            return sentence;
        }
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    if let Some(idx) = truncated.rfind(|c: char| c.is_whitespace()) {
        truncated[..idx].trim_end().to_string()
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_load() {
        let cards = load_builtin_cards();
        assert_eq!(cards.len(), 4);
        assert!(card_for(&cards, NpcId::Dewr).is_some());
        assert!(card_for(&cards, NpcId::Dremis).is_some());
    }

    #[test]
    fn present_line_keeps_full_dewr_description() {
        let cards = load_builtin_cards();
        let dewr = card_for(&cards, NpcId::Dewr).unwrap();
        let line = dewr.present_line();
        assert!(line.contains("anyone passes"), "got {line}");
        assert!(!line.ends_with("before a"));
    }

    #[test]
    fn description_tag_does_not_cut_mid_word() {
        let card = CharacterCard::ambient(
            "x",
            "X",
            "quiet",
            "A very long description that goes on and on about the muddy street lantern light and a goblin who wants coin before anyone passes the crooked door tonight under the rain.",
            "plain",
        );
        let tag = card.description_tag();
        let after_em = tag.split("— ").nth(1).unwrap_or(&tag);
        assert!(
            !after_em.ends_with(" a") && !after_em.ends_with(" the"),
            "mid-word/awkward cut: {tag}"
        );
    }
}
