use serde::{Deserialize, Serialize};

use crate::weapons::Weapon;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Location {
    /// Street / porch in front of the tavern door.
    Outside,
    MainHall,
    Kitchen,
    Upstairs,
    StorageRoom,
}

impl Location {
    pub fn display_name(self) -> &'static str {
        match self {
            Location::Outside => "outside the tavern",
            Location::MainHall => "main hall",
            Location::Kitchen => "kitchen",
            Location::Upstairs => "upstairs bedrooms",
            Location::StorageRoom => "storage room",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        let n = name.trim().to_lowercase();
        match n.as_str() {
            "outside" | "street" | "porch" | "door" | "front" => Some(Location::Outside),
            "hall" | "main" | "main hall" | "main_hall" | "tavern" | "inside" | "in" => {
                Some(Location::MainHall)
            }
            "kitchen" | "cook" => Some(Location::Kitchen),
            "upstairs" | "bedroom" | "bedrooms" | "rooms" | "inn" => Some(Location::Upstairs),
            "storage" | "storage room" | "storage_room" | "store" | "cellar" => {
                Some(Location::StorageRoom)
            }
            _ => None,
        }
    }

    /// Scan free text for a destination (e.g. "go to the kitchen").
    pub fn parse_from_text(text: &str) -> Option<Self> {
        let lower = text.to_lowercase();
        if lower.contains("upstairs") || lower.contains("bedroom") {
            return Some(Location::Upstairs);
        }
        if lower.contains("kitchen") {
            return Some(Location::Kitchen);
        }
        if lower.contains("storage") || lower.contains("cellar") {
            return Some(Location::StorageRoom);
        }
        if lower.contains("outside") || lower.contains("street") || lower.contains("porch") {
            return Some(Location::Outside);
        }
        if lower.contains("main hall") || lower.contains("into the hall") || lower.contains("to the hall")
        {
            return Some(Location::MainHall);
        }
        None
    }

    pub fn plan_tag(self) -> &'static str {
        match self {
            Location::Outside => "outside",
            Location::MainHall => "hall",
            Location::Kitchen => "kitchen",
            Location::Upstairs => "upstairs",
            Location::StorageRoom => "storage",
        }
    }

    pub fn is_interior(self) -> bool {
        !matches!(self, Location::Outside)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NpcId {
    Dewr,
    Sally,
    Groll,
    Dremis,
    Patron,
    Goblin,
}

impl NpcId {
    pub fn display_name(self) -> &'static str {
        match self {
            NpcId::Dewr => "Dewr",
            NpcId::Sally => "Sally",
            NpcId::Groll => "Groll",
            NpcId::Dremis => "Dremis",
            NpcId::Patron => "a patron",
            NpcId::Goblin => "a goblin",
        }
    }

    /// CLI / feed icon for this character.
    pub fn icon(self) -> &'static str {
        match self {
            NpcId::Dewr => "👺",
            NpcId::Sally => "🍺",
            NpcId::Groll => "🐉",
            NpcId::Dremis => "⚔️",
            NpcId::Patron => "🧔",
            NpcId::Goblin => "👹",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        let n = name.trim().to_lowercase();
        let n = crate::slang::resolve_npc_alias(&n).unwrap_or(n.as_str());
        match n {
            "dewr" => Some(NpcId::Dewr),
            "sally" | "wench" => Some(NpcId::Sally),
            "groll" | "dragon" | "newt" => Some(NpcId::Groll),
            "dremis" | "warlord" | "knight" => Some(NpcId::Dremis),
            "patron" => Some(NpcId::Patron),
            "goblin" => Some(NpcId::Goblin),
            _ => None,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            NpcId::Dewr => "dewr",
            NpcId::Sally => "sally",
            NpcId::Groll => "groll",
            NpcId::Dremis => "dremis",
            NpcId::Patron => "patron",
            NpcId::Goblin => "goblin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    /// Display name for the traveler (status bar + POV prompts).
    #[serde(default = "default_player_name")]
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub gold: i32,
    pub weapon: Weapon,
    /// SRD 5e-shaped build (abilities + skill proficiencies).
    #[serde(default)]
    pub build: crate::skills::CharacterBuild,
    pub has_password: bool,
    pub has_amulet: bool,
    pub sally_tip_discount: bool,
    pub sally_charmed: bool,
    pub dewr_backed_down: bool,
    pub aura_cleared: bool,
    pub met_dewr: bool,
    /// Dewr's door-tax scam has been paid, called out, or fought through.
    pub dewr_scam_cleared: bool,
}

impl PlayerState {
    pub fn skill_bonus(&self, skill: crate::skills::Skill) -> i32 {
        self.build.skill_bonus(skill)
    }
}

fn default_player_name() -> String {
    "You".into()
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            name: default_player_name(),
            hp: 10,
            max_hp: 10,
            gold: 100,
            weapon: Weapon::Knife,
            build: crate::skills::CharacterBuild::default(),
            has_password: false,
            has_amulet: false,
            sally_tip_discount: false,
            sally_charmed: false,
            dewr_backed_down: false,
            aura_cleared: false,
            met_dewr: false,
            dewr_scam_cleared: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcRuntime {
    pub id: NpcId,
    pub hp: i32,
    pub max_hp: i32,
    pub weapon: Weapon,
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    pub location: Location,
    pub player: PlayerState,
    pub dremis: NpcRuntime,
    pub dewr: NpcRuntime,
    pub groll: NpcRuntime,
    pub sally: NpcRuntime,
    pub sword_offer_available: bool,
    pub sword_offer_taken: bool,
    pub first_talk_pending: bool,
    /// Prop ids the player has noticed here (kitchen door, patron's trade, etc.).
    #[serde(default)]
    pub discovered_props: Vec<String>,
    /// Soft-flag ids from the world file the player has looked for or observed.
    #[serde(default)]
    pub discovered_soft: Vec<String>,
    /// Soft details whose local stake or check already succeeded.
    #[serde(default)]
    pub soft_resolved: Vec<String>,
    /// Seated actor ids who have already acted in this round.
    #[serde(default)]
    pub round_gone: Vec<String>,
    /// Short committed outcomes from this round, for the NPC phase.
    #[serde(default)]
    pub round_log: Vec<String>,
    /// Attitude, heat, time, declared runtime flags — not canon truth.
    #[serde(flatten)]
    pub situation: crate::worlds::Situation,
}

impl Default for WorldState {
    fn default() -> Self {
        Self {
            location: Location::Outside,
            player: PlayerState::default(),
            dremis: NpcRuntime {
                id: NpcId::Dremis,
                hp: 20,
                max_hp: 20,
                weapon: Weapon::BattleAxe,
                alive: true,
            },
            dewr: NpcRuntime {
                id: NpcId::Dewr,
                hp: 8,
                max_hp: 8,
                weapon: Weapon::Knife,
                alive: true,
            },
            groll: NpcRuntime {
                id: NpcId::Groll,
                hp: 8,
                max_hp: 8,
                weapon: Weapon::Mace,
                alive: true,
            },
            sally: NpcRuntime {
                id: NpcId::Sally,
                hp: 8,
                max_hp: 8,
                weapon: Weapon::None,
                alive: true,
            },
            sword_offer_available: true,
            sword_offer_taken: false,
            first_talk_pending: true,
            discovered_props: Vec::new(),
            discovered_soft: Vec::new(),
            soft_resolved: Vec::new(),
            round_gone: Vec::new(),
            round_log: Vec::new(),
            situation: crate::worlds::Situation::default(),
        }
    }
}

impl WorldState {
    pub fn attitude_toward(&self, npc: &str) -> i32 {
        self.situation.attitude_toward(npc)
    }

    pub fn npc_flag(&self, npc: &str, flag: &str) -> bool {
        self.situation.npc_flag(npc, flag)
    }

    pub fn check_fail_count(&self, secret_id: &str) -> u32 {
        self.situation.check_fail_count(secret_id)
    }

    pub fn set_npc_flag(&mut self, npc: &str, flag: &str) {
        if let Some(f) = crate::worlds::SituationFlag::parse(flag) {
            self.situation.apply(crate::worlds::SituationKnob::NpcFlag {
                npc: npc.to_string(),
                flag: f,
            });
        } else {
            // Canon sheet flags (sworn_silent, etc.) — seed only, not runtime minted.
            self.situation.seed_canon_npc_flag(npc, flag);
        }
    }
}
