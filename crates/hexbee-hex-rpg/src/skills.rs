//! SRD 5e abilities, skills, and simple build presets for the tavern hex.

use serde::{Deserialize, Serialize};

/// Six ability scores (SRD).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ability {
    Str,
    Dex,
    Con,
    Int,
    Wis,
    Cha,
}

impl Ability {
    pub const ALL: [Ability; 6] = [
        Ability::Str,
        Ability::Dex,
        Ability::Con,
        Ability::Int,
        Ability::Wis,
        Ability::Cha,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Str => "str",
            Self::Dex => "dex",
            Self::Con => "con",
            Self::Int => "int",
            Self::Wis => "wis",
            Self::Cha => "cha",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Str => "STR",
            Self::Dex => "DEX",
            Self::Con => "CON",
            Self::Int => "INT",
            Self::Wis => "WIS",
            Self::Cha => "CHA",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "str" | "strength" => Some(Self::Str),
            "dex" | "dexterity" => Some(Self::Dex),
            "con" | "constitution" => Some(Self::Con),
            "int" | "intelligence" => Some(Self::Int),
            "wis" | "wisdom" => Some(Self::Wis),
            "cha" | "charisma" => Some(Self::Cha),
            _ => None,
        }
    }
}

/// Ability score modifier: floor((score - 10) / 2).
pub fn ability_mod(score: i32) -> i32 {
    (score - 10).div_euclid(2)
}

/// Eighteen SRD skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skill {
    Athletics,
    Acrobatics,
    SleightOfHand,
    Stealth,
    Arcana,
    History,
    Investigation,
    Nature,
    Religion,
    AnimalHandling,
    Insight,
    Medicine,
    Perception,
    Survival,
    Deception,
    Intimidation,
    Performance,
    Persuasion,
}

impl Skill {
    pub const ALL: [Skill; 18] = [
        Skill::Athletics,
        Skill::Acrobatics,
        Skill::SleightOfHand,
        Skill::Stealth,
        Skill::Arcana,
        Skill::History,
        Skill::Investigation,
        Skill::Nature,
        Skill::Religion,
        Skill::AnimalHandling,
        Skill::Insight,
        Skill::Medicine,
        Skill::Perception,
        Skill::Survival,
        Skill::Deception,
        Skill::Intimidation,
        Skill::Performance,
        Skill::Persuasion,
    ];

    pub fn ability(self) -> Ability {
        match self {
            Skill::Athletics => Ability::Str,
            Skill::Acrobatics | Skill::SleightOfHand | Skill::Stealth => Ability::Dex,
            Skill::Arcana
            | Skill::History
            | Skill::Investigation
            | Skill::Nature
            | Skill::Religion => Ability::Int,
            Skill::AnimalHandling
            | Skill::Insight
            | Skill::Medicine
            | Skill::Perception
            | Skill::Survival => Ability::Wis,
            Skill::Deception
            | Skill::Intimidation
            | Skill::Performance
            | Skill::Persuasion => Ability::Cha,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Skill::Athletics => "athletics",
            Skill::Acrobatics => "acrobatics",
            Skill::SleightOfHand => "sleight_of_hand",
            Skill::Stealth => "stealth",
            Skill::Arcana => "arcana",
            Skill::History => "history",
            Skill::Investigation => "investigation",
            Skill::Nature => "nature",
            Skill::Religion => "religion",
            Skill::AnimalHandling => "animal_handling",
            Skill::Insight => "insight",
            Skill::Medicine => "medicine",
            Skill::Perception => "perception",
            Skill::Survival => "survival",
            Skill::Deception => "deception",
            Skill::Intimidation => "intimidation",
            Skill::Performance => "performance",
            Skill::Persuasion => "persuasion",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Skill::Athletics => "Athletics",
            Skill::Acrobatics => "Acrobatics",
            Skill::SleightOfHand => "Sleight of Hand",
            Skill::Stealth => "Stealth",
            Skill::Arcana => "Arcana",
            Skill::History => "History",
            Skill::Investigation => "Investigation",
            Skill::Nature => "Nature",
            Skill::Religion => "Religion",
            Skill::AnimalHandling => "Animal Handling",
            Skill::Insight => "Insight",
            Skill::Medicine => "Medicine",
            Skill::Perception => "Perception",
            Skill::Survival => "Survival",
            Skill::Deception => "Deception",
            Skill::Intimidation => "Intimidation",
            Skill::Performance => "Performance",
            Skill::Persuasion => "Persuasion",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let n = s.trim().to_lowercase().replace('-', "_").replace(' ', "_");
        Skill::ALL
            .iter()
            .copied()
            .find(|sk| sk.as_str() == n || sk.label().eq_ignore_ascii_case(s.trim()))
    }
}

/// Standard array for builds.
pub const STANDARD_ARRAY: [i32; 6] = [15, 14, 13, 12, 10, 8];

/// Level-1 proficiency bonus.
pub const PROFICIENCY_BONUS: i32 = 2;

/// How many skill proficiencies the traveler picks.
pub const SKILL_PICKS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbilityScores {
    pub str: i32,
    pub dex: i32,
    pub con: i32,
    pub int: i32,
    pub wis: i32,
    pub cha: i32,
}

impl Default for AbilityScores {
    fn default() -> Self {
        // Balanced traveler: standard array assigned Face-like by default.
        Self {
            str: 10,
            dex: 12,
            con: 13,
            int: 8,
            wis: 14,
            cha: 15,
        }
    }
}

impl AbilityScores {
    pub fn get(&self, ab: Ability) -> i32 {
        match ab {
            Ability::Str => self.str,
            Ability::Dex => self.dex,
            Ability::Con => self.con,
            Ability::Int => self.int,
            Ability::Wis => self.wis,
            Ability::Cha => self.cha,
        }
    }

    pub fn set(&mut self, ab: Ability, score: i32) {
        match ab {
            Ability::Str => self.str = score,
            Ability::Dex => self.dex = score,
            Ability::Con => self.con = score,
            Ability::Int => self.int = score,
            Ability::Wis => self.wis = score,
            Ability::Cha => self.cha = score,
        }
    }

    pub fn modifier(&self, ab: Ability) -> i32 {
        ability_mod(self.get(ab))
    }

    /// Assign standard array in order STR DEX CON INT WIS CHA.
    pub fn from_ordered(scores: [i32; 6]) -> Self {
        Self {
            str: scores[0],
            dex: scores[1],
            con: scores[2],
            int: scores[3],
            wis: scores[4],
            cha: scores[5],
        }
    }
}

/// Named build presets (array assignment + suggested skills).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildPreset {
    Face,
    Scout,
    Scholar,
    Bruiser,
}

impl BuildPreset {
    pub const ALL: [BuildPreset; 4] = [
        BuildPreset::Face,
        BuildPreset::Scout,
        BuildPreset::Scholar,
        BuildPreset::Bruiser,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Face => "face",
            Self::Scout => "scout",
            Self::Scholar => "scholar",
            Self::Bruiser => "bruiser",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Face => "Face",
            Self::Scout => "Scout",
            Self::Scholar => "Scholar",
            Self::Bruiser => "Bruiser",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Self::Face => "talker — high CHA, social skills",
            Self::Scout => "sneak & spot — high DEX/WIS",
            Self::Scholar => "lore & wit — high INT/WIS",
            Self::Bruiser => "muscle — high STR/CON",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "face" | "1" => Some(Self::Face),
            "scout" | "2" => Some(Self::Scout),
            "scholar" | "3" => Some(Self::Scholar),
            "bruiser" | "4" => Some(Self::Bruiser),
            _ => None,
        }
    }

    /// Scores ordered STR DEX CON INT WIS CHA using the standard array.
    pub fn scores(self) -> AbilityScores {
        match self {
            // CHA 15, WIS 14, CON 13, DEX 12, STR 10, INT 8
            Self::Face => AbilityScores::from_ordered([10, 12, 13, 8, 14, 15]),
            // DEX 15, WIS 14, CON 13, STR 12, INT 10, CHA 8
            Self::Scout => AbilityScores::from_ordered([12, 15, 13, 10, 14, 8]),
            // INT 15, WIS 14, CON 13, DEX 12, CHA 10, STR 8
            Self::Scholar => AbilityScores::from_ordered([8, 12, 13, 15, 14, 10]),
            // STR 15, CON 14, DEX 13, WIS 12, CHA 10, INT 8
            Self::Bruiser => AbilityScores::from_ordered([15, 13, 14, 8, 12, 10]),
        }
    }

    pub fn skills(self) -> [Skill; SKILL_PICKS] {
        match self {
            Self::Face => [
                Skill::Persuasion,
                Skill::Deception,
                Skill::Insight,
                Skill::Performance,
            ],
            Self::Scout => [
                Skill::Stealth,
                Skill::Perception,
                Skill::Survival,
                Skill::Acrobatics,
            ],
            Self::Scholar => [
                Skill::Investigation,
                Skill::Arcana,
                Skill::History,
                Skill::Insight,
            ],
            Self::Bruiser => [
                Skill::Athletics,
                Skill::Intimidation,
                Skill::Survival,
                Skill::Perception,
            ],
        }
    }
}

/// Full traveler sheet slice used at runtime and by chargen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterBuild {
    pub abilities: AbilityScores,
    pub skills: Vec<Skill>,
    #[serde(default = "default_prof")]
    pub proficiency_bonus: i32,
    #[serde(default)]
    pub preset: Option<String>,
}

fn default_prof() -> i32 {
    PROFICIENCY_BONUS
}

impl Default for CharacterBuild {
    fn default() -> Self {
        let preset = BuildPreset::Face;
        Self {
            abilities: preset.scores(),
            skills: preset.skills().to_vec(),
            proficiency_bonus: PROFICIENCY_BONUS,
            preset: Some(preset.as_str().into()),
        }
    }
}

impl CharacterBuild {
    pub fn from_preset(preset: BuildPreset) -> Self {
        Self {
            abilities: preset.scores(),
            skills: preset.skills().to_vec(),
            proficiency_bonus: PROFICIENCY_BONUS,
            preset: Some(preset.as_str().into()),
        }
    }

    pub fn is_proficient(&self, skill: Skill) -> bool {
        self.skills.contains(&skill)
    }

    /// d20 modifier: ability mod + proficiency if skilled.
    pub fn skill_bonus(&self, skill: Skill) -> i32 {
        let ab = skill.ability();
        let m = self.abilities.modifier(ab);
        if self.is_proficient(skill) {
            m + self.proficiency_bonus
        } else {
            m
        }
    }

    pub fn sheet_summary(&self) -> String {
        let abs = &self.abilities;
        let skills: Vec<_> = self.skills.iter().map(|s| s.label()).collect();
        format!(
            "STR {} ({:+})  DEX {} ({:+})  CON {} ({:+})  INT {} ({:+})  WIS {} ({:+})  CHA {} ({:+})\n\
Proficient: {}  |  prof +{}",
            abs.str,
            ability_mod(abs.str),
            abs.dex,
            ability_mod(abs.dex),
            abs.con,
            ability_mod(abs.con),
            abs.int,
            ability_mod(abs.int),
            abs.wis,
            ability_mod(abs.wis),
            abs.cha,
            ability_mod(abs.cha),
            skills.join(", "),
            self.proficiency_bonus,
        )
    }

    pub fn to_stats_json(&self) -> serde_json::Value {
        serde_json::json!({
            "str": self.abilities.str,
            "dex": self.abilities.dex,
            "con": self.abilities.con,
            "int": self.abilities.int,
            "wis": self.abilities.wis,
            "cha": self.abilities.cha,
            "proficiency_bonus": self.proficiency_bonus,
            "skills": self.skills.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            "preset": self.preset,
            "chargen_done": true,
        })
    }

    pub fn from_stats_json(stats: &serde_json::Value) -> Option<Self> {
        let str = stats.get("str")?.as_i64()? as i32;
        let dex = stats.get("dex")?.as_i64()? as i32;
        let con = stats.get("con")?.as_i64()? as i32;
        let int = stats.get("int")?.as_i64()? as i32;
        let wis = stats.get("wis")?.as_i64()? as i32;
        let cha = stats.get("cha")?.as_i64()? as i32;
        let proficiency_bonus = stats
            .get("proficiency_bonus")
            .and_then(|v| v.as_i64())
            .unwrap_or(PROFICIENCY_BONUS as i64) as i32;
        let skills = stats
            .get("skills")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().and_then(Skill::parse))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let preset = stats
            .get("preset")
            .and_then(|v| v.as_str())
            .map(String::from);
        Some(Self {
            abilities: AbilityScores {
                str,
                dex,
                con,
                int,
                wis,
                cha,
            },
            skills,
            proficiency_bonus,
            preset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ability_mod_table() {
        assert_eq!(ability_mod(8), -1);
        assert_eq!(ability_mod(10), 0);
        assert_eq!(ability_mod(15), 2);
        assert_eq!(ability_mod(1), -5);
    }

    #[test]
    fn persuasion_is_cha() {
        assert_eq!(Skill::Persuasion.ability(), Ability::Cha);
    }

    #[test]
    fn face_skill_bonus_includes_prof() {
        let b = CharacterBuild::from_preset(BuildPreset::Face);
        // CHA 15 → +2, prof +2 → +4 Persuasion
        assert_eq!(b.skill_bonus(Skill::Persuasion), 4);
        // Athletics STR 10 → +0, not proficient
        assert_eq!(b.skill_bonus(Skill::Athletics), 0);
    }

    #[test]
    fn standard_array_sum() {
        let sum: i32 = STANDARD_ARRAY.iter().sum();
        assert_eq!(sum, 15 + 14 + 13 + 12 + 10 + 8);
    }
}
