//! Scene coordinator — who is here, who you're talking to, what the room is.
//!
//! Engine truth for presence. Quest NPCs are world-owned; ambient extras are
//! coordinator-owned cards the AI may invent on enter/look.

use crate::cards::CharacterCard;
use crate::state::{Location, NpcId, WorldState};

const QUEST_RESERVED: &[&str] = &[
    "dewr", "sally", "groll", "dremis", "goblin", "gobbo", "patron", "warlord", "wench",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TalkMiss {
    NobodyHere,
    NotPresent(NpcId),
    NotPresentName(String),
    Ambiguous,
}

impl TalkMiss {
    pub fn message(&self) -> String {
        match self {
            Self::NobodyHere => "There's no one here to talk to.".into(),
            Self::NotPresent(npc) => format!("{} isn't here right now.", npc.display_name()),
            Self::NotPresentName(name) => format!("{name} isn't here right now."),
            Self::Ambiguous => "A few people are nearby — say who you mean.".into(),
        }
    }
}

/// Who the player is addressing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TalkFocus {
    Quest(NpcId),
    Ambient(String),
}

impl TalkFocus {
    pub fn display_name<'a>(&'a self, coord: &'a SceneCoordinator) -> &'a str {
        match self {
            Self::Quest(id) => id.display_name(),
            Self::Ambient(id) => coord
                .ambient
                .iter()
                .find(|a| a.id == *id)
                .map(|a| a.card.name.as_str())
                .unwrap_or(id.as_str()),
        }
    }
}

/// Requested talk target before resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TalkRequest {
    Quest(NpcId),
    Name(String),
}

#[derive(Debug, Clone)]
pub struct AmbientNpc {
    pub id: String,
    pub location: Location,
    pub card: CharacterCard,
}

/// Immutable view of the current room for one turn.
#[derive(Debug, Clone)]
pub struct SceneSnapshot {
    pub location: Location,
    pub visible_npcs: Vec<NpcId>,
    pub ambient_ids: Vec<String>,
    pub facts: Vec<String>,
    /// Single present quest NPC when no ambient and exactly one quest face.
    pub default_talk: Option<NpcId>,
    pub fingerprint: u64,
}

impl SceneSnapshot {
    pub fn present_csv(&self) -> String {
        let mut names: Vec<String> = self
            .visible_npcs
            .iter()
            .map(|n| n.display_name().to_lowercase())
            .collect();
        names.extend(self.ambient_ids.iter().cloned());
        if names.is_empty() {
            "(nobody)".into()
        } else {
            names.join(",")
        }
    }

    pub fn contains(&self, npc: NpcId) -> bool {
        self.visible_npcs.contains(&npc)
    }

    pub fn interpret_facts_line(&self) -> String {
        let mut parts = self.facts.clone();
        parts.insert(0, format!("visible_npcs={}", self.present_csv()));
        parts.join("; ")
    }

    pub fn presence_count(&self) -> usize {
        self.visible_npcs.len() + self.ambient_ids.len()
    }
}

/// Sticky conversation focus + ambient cast for the live scene.
#[derive(Debug, Default, Clone)]
pub struct SceneCoordinator {
    pub last_talk: Option<NpcId>,
    pub last_talk_ambient: Option<String>,
    pub last_fingerprint: u64,
    pub ambient: Vec<AmbientNpc>,
    /// Locations that already received an ambient fill attempt.
    pub ambient_seeded: Vec<Location>,
}

impl SceneCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn visible_npcs(world: &WorldState) -> Vec<NpcId> {
        match world.location {
            Location::Outside => {
                if world.dewr.alive {
                    vec![NpcId::Dewr]
                } else {
                    vec![]
                }
            }
            Location::MainHall => {
                let mut v = Vec::new();
                if world.sally.alive {
                    v.push(NpcId::Sally);
                }
                if world.groll.alive {
                    v.push(NpcId::Groll);
                }
                if world.dewr.alive && world.player.dewr_scam_cleared {
                    v.push(NpcId::Dewr);
                }
                v
            }
            Location::StorageRoom => {
                if world.dremis.alive {
                    vec![NpcId::Dremis]
                } else {
                    vec![]
                }
            }
            Location::Kitchen | Location::Upstairs => vec![],
        }
    }

    pub fn ambient_here(&self, location: Location) -> Vec<&AmbientNpc> {
        self.ambient
            .iter()
            .filter(|a| a.location == location)
            .collect()
    }

    pub fn has_ambient_seeded(&self, location: Location) -> bool {
        self.ambient_seeded.contains(&location)
    }

    pub fn mark_ambient_seeded(&mut self, location: Location) {
        if !self.ambient_seeded.contains(&location) {
            self.ambient_seeded.push(location);
        }
    }

    /// Goblin at the door is Dewr when he's present.
    pub fn canonicalize_npc(world: &WorldState, id: NpcId) -> NpcId {
        match id {
            NpcId::Goblin if world.dewr.alive && Self::visible_npcs(world).contains(&NpcId::Dewr) => {
                NpcId::Dewr
            }
            other => other,
        }
    }

    pub fn slugify(raw: &str) -> String {
        let s: String = raw
            .chars()
            .map(|c| {
                let c = c.to_ascii_lowercase();
                if c.is_ascii_alphanumeric() {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let s = s.trim_matches('_').to_string();
        if s.is_empty() {
            "patron".into()
        } else {
            s
        }
    }

    fn is_reserved_id(id: &str) -> bool {
        QUEST_RESERVED.iter().any(|r| *r == id)
    }

    /// Register ambient cards for a location. Skips reserved / duplicate ids.
    pub fn register_ambient(&mut self, location: Location, cards: Vec<CharacterCard>) {
        for mut card in cards {
            let mut id = Self::slugify(&card.id);
            if id.is_empty() {
                id = Self::slugify(&card.name);
            }
            if Self::is_reserved_id(&id) {
                continue;
            }
            // Unique across ambient cast.
            let mut unique = id.clone();
            let mut n = 2u32;
            while self.ambient.iter().any(|a| a.id == unique) {
                unique = format!("{id}_{n}");
                n += 1;
            }
            card.id = unique.clone();
            if card.name.trim().is_empty() {
                card.name = unique.clone();
            }
            if card.weapon.trim().is_empty() {
                card.weapon = "none".into();
            }
            self.ambient.push(AmbientNpc {
                id: unique,
                location,
                card,
            });
        }
        self.mark_ambient_seeded(location);
    }

    pub fn find_ambient(&self, location: Location, name_or_id: &str) -> Option<&AmbientNpc> {
        let key = name_or_id.trim().to_lowercase();
        self.ambient_here(location).into_iter().find(|a| {
            a.id == key || a.card.name.to_lowercase() == key || a.card.id.to_lowercase() == key
        })
    }

    pub fn ambient_card(&self, id: &str) -> Option<&CharacterCard> {
        self.ambient.iter().find(|a| a.id == id).map(|a| &a.card)
    }

    pub fn fingerprint(&self, world: &WorldState) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        world.location.hash(&mut h);
        world.dewr.alive.hash(&mut h);
        world.sally.alive.hash(&mut h);
        world.groll.alive.hash(&mut h);
        world.dremis.alive.hash(&mut h);
        world.player.dewr_scam_cleared.hash(&mut h);
        world.player.aura_cleared.hash(&mut h);
        world.player.has_amulet.hash(&mut h);
        world.player.has_password.hash(&mut h);
        world.sword_offer_available.hash(&mut h);
        world.sword_offer_taken.hash(&mut h);
        for a in &self.ambient {
            a.id.hash(&mut h);
            a.location.hash(&mut h);
        }
        h.finish()
    }

    /// When to run transition + arrival paint — location or major room-state shift only.
    pub fn paint_fingerprint(&self, world: &WorldState) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        world.location.hash(&mut h);
        world.player.aura_cleared.hash(&mut h);
        h.finish()
    }

    pub fn snapshot(&self, world: &WorldState) -> SceneSnapshot {
        let visible_npcs = Self::visible_npcs(world);
        let ambient_ids: Vec<String> = self
            .ambient_here(world.location)
            .into_iter()
            .map(|a| a.id.clone())
            .collect();
        let default_talk = if visible_npcs.len() == 1 && ambient_ids.is_empty() {
            Some(visible_npcs[0])
        } else {
            None
        };
        let mut facts = vec![format!("location={}", world.location.display_name())];
        match world.location {
            Location::Outside => {
                if world.dewr.alive && !world.player.dewr_scam_cleared {
                    facts.push("dewr_blocks_door=true".into());
                    facts.push("dewr_demands_coin=true".into());
                } else if world.dewr.alive {
                    facts.push("dewr_nearby=true".into());
                } else {
                    facts.push("dewr_dead=true".into());
                }
                facts.push("tavern_door_visible=true".into());
            }
            Location::MainHall => {
                if world.sally.alive {
                    facts.push("sally_working=true".into());
                }
                if world.groll.alive {
                    facts.push("cloaked_patron_in_corner=true".into());
                }
                if world.dewr.alive && world.player.dewr_scam_cleared {
                    facts.push("dewr_inside_near_door=true".into());
                }
                facts.push("kitchen_door_visible=true".into());
                facts.push("stairs_up_visible=true".into());
                if world.sword_offer_available && !world.sword_offer_taken {
                    facts.push("patron_eyeing_knife_and_coin=true".into());
                }
            }
            Location::Kitchen => facts.push("food_and_drink_for_sale=true".into()),
            Location::Upstairs => facts.push("rooms_for_rent=true".into()),
            Location::StorageRoom => {
                if world.dremis.alive {
                    facts.push("dangerous_figure_present=true".into());
                }
                if !world.player.aura_cleared {
                    facts.push("crushing_dread_aura=true".into());
                } else {
                    facts.push("aura_gone=true".into());
                }
            }
        }
        for a in self.ambient_here(world.location) {
            facts.push(format!("ambient_{}={}", a.id, a.card.description_tag()));
        }
        facts.push(format!("player_hp={}", world.player.hp));
        facts.push(format!("player_gold={}", world.player.gold));

        SceneSnapshot {
            location: world.location,
            visible_npcs,
            ambient_ids,
            facts,
            default_talk,
            fingerprint: self.fingerprint(world),
        }
    }

    pub fn parse_talk_request(raw: Option<&str>) -> Option<TalkRequest> {
        let raw = raw?.trim();
        if raw.is_empty() {
            return None;
        }
        if let Some(id) = NpcId::parse(raw) {
            if matches!(id, NpcId::Patron | NpcId::Goblin) {
                return Some(TalkRequest::Name(raw.to_lowercase()));
            }
            return Some(TalkRequest::Quest(id));
        }
        Some(TalkRequest::Name(raw.to_lowercase()))
    }

    pub fn resolve_talk_target(
        &self,
        world: &WorldState,
        requested: Option<TalkRequest>,
    ) -> Result<TalkFocus, TalkMiss> {
        let quest = Self::visible_npcs(world);
        let amb = self.ambient_here(world.location);
        if quest.is_empty() && amb.is_empty() {
            return Err(TalkMiss::NobodyHere);
        }

        if let Some(req) = requested {
            match req {
                TalkRequest::Quest(id) => {
                    let id = Self::canonicalize_npc(world, id);
                    if quest.contains(&id) {
                        return Ok(TalkFocus::Quest(id));
                    }
                    if matches!(id, NpcId::Patron | NpcId::Goblin) {
                        if quest.len() + amb.len() == 1 {
                            if let Some(q) = quest.first() {
                                return Ok(TalkFocus::Quest(*q));
                            }
                            if let Some(a) = amb.first() {
                                return Ok(TalkFocus::Ambient(a.id.clone()));
                            }
                        }
                        return Err(TalkMiss::Ambiguous);
                    }
                    return Err(TalkMiss::NotPresent(id));
                }
                TalkRequest::Name(name) => {
                    if let Some(a) = self.find_ambient(world.location, &name) {
                        return Ok(TalkFocus::Ambient(a.id.clone()));
                    }
                    if let Some(id) = NpcId::parse(&name) {
                        let id = Self::canonicalize_npc(world, id);
                        if quest.contains(&id) {
                            return Ok(TalkFocus::Quest(id));
                        }
                        return Err(TalkMiss::NotPresent(id));
                    }
                    return Err(TalkMiss::NotPresentName(name));
                }
            }
        }

        if let Some(ref last_a) = self.last_talk_ambient {
            if amb.iter().any(|a| a.id == *last_a) {
                return Ok(TalkFocus::Ambient(last_a.clone()));
            }
        }
        if let Some(last) = self.last_talk {
            let last = Self::canonicalize_npc(world, last);
            if quest.contains(&last) {
                return Ok(TalkFocus::Quest(last));
            }
        }

        // Prefer the sole quest face even when ambient extras fill the room.
        if quest.len() == 1 {
            return Ok(TalkFocus::Quest(quest[0]));
        }
        if quest.is_empty() && amb.len() == 1 {
            return Ok(TalkFocus::Ambient(amb[0].id.clone()));
        }
        Err(TalkMiss::Ambiguous)
    }

    /// Back-compat wrapper used by older call sites that only know quest ids.
    pub fn resolve_talk_quest(
        &self,
        world: &WorldState,
        requested: Option<NpcId>,
    ) -> Result<NpcId, TalkMiss> {
        let req = requested.map(TalkRequest::Quest);
        match self.resolve_talk_target(world, req)? {
            TalkFocus::Quest(id) => Ok(id),
            TalkFocus::Ambient(_) => Err(TalkMiss::Ambiguous),
        }
    }

    pub fn note_talk(&mut self, focus: &TalkFocus) {
        match focus {
            TalkFocus::Quest(id) => {
                self.last_talk = Some(*id);
                self.last_talk_ambient = None;
            }
            TalkFocus::Ambient(id) => {
                self.last_talk_ambient = Some(id.clone());
                self.last_talk = None;
            }
        }
    }

    pub fn clear_focus_if_absent(&mut self, world: &WorldState) {
        if let Some(last) = self.last_talk {
            if !Self::visible_npcs(world).contains(&last) {
                self.last_talk = None;
            }
        }
        if let Some(ref last_a) = self.last_talk_ambient.clone() {
            if self.find_ambient(world.location, last_a).is_none() {
                self.last_talk_ambient = None;
            }
        }
    }

    pub fn header_needs_refresh(&self, world: &WorldState) -> bool {
        self.last_fingerprint != self.fingerprint(world)
    }

    pub fn mark_header_synced(&mut self, world: &WorldState) {
        self.last_fingerprint = self.fingerprint(world);
    }

    /// Deterministic ambient cast when the LLM fails or is absent.
    pub fn fallback_ambient(location: Location) -> Vec<CharacterCard> {
        match location {
            Location::Outside => vec![CharacterCard::ambient(
                "street_dog",
                "a mangy dog",
                "wary, hungry",
                "A thin dog watches from the gutter, ears half-up.",
                "whine, soft growl",
            )],
            Location::MainHall => vec![
                CharacterCard::ambient(
                    "carter",
                    "a tired carter",
                    "weary, blunt",
                    "A road-dusted carter nurses a mug by the hearth.",
                    "plain, tired",
                ),
                CharacterCard::ambient(
                    "piper",
                    "a pipe smoker",
                    "quiet, watchful",
                    "Someone in a patched coat smokes a long pipe near the wall.",
                    "soft, measured",
                ),
            ],
            Location::Kitchen => vec![CharacterCard::ambient(
                "cook_hand",
                "a kitchen hand",
                "busy, short-tempered",
                "A flour-dusted kitchen hand stirs a pot and watches the door.",
                "brisk, clipped",
            )],
            Location::Upstairs => vec![CharacterCard::ambient(
                "chambermaid",
                "a chambermaid",
                "polite, hurried",
                "A chambermaid carries linens down the narrow hall.",
                "soft, quick",
            )],
            Location::StorageRoom => vec![],
        }
    }

    /// Presence-aware room description (no HP line).
    pub fn look_prose(&self, world: &WorldState) -> String {
        let visible = Self::visible_npcs(world);
        let mut base: String = match world.location {
            Location::Outside => {
                if world.dewr.alive && !world.player.dewr_scam_cleared {
                    "You're outside the tavern. A goblin blocks the door and grins at your purse."
                        .into()
                } else if world.dewr.alive {
                    "You're outside. The door is clear; the goblin lingers nearby.".into()
                } else {
                    "You're outside the tavern. The doorway stands empty.".into()
                }
            }
            Location::MainHall => {
                let mut bits = vec!["The main hall is smoky and loud.".to_string()];
                if visible.contains(&NpcId::Sally) {
                    bits.push("A woman works the tables.".into());
                }
                if visible.contains(&NpcId::Groll) {
                    bits.push("A cloaked figure sits apart.".into());
                }
                if visible.contains(&NpcId::Dewr) {
                    bits.push("The door goblin loiters near the entrance.".into());
                }
                bits.push("Stairs and a kitchen door are visible.".into());
                bits.join(" ")
            }
            Location::Kitchen => {
                "The kitchen smells of stew. Someone will sell you a bite for coin.".into()
            }
            Location::Upstairs => "Narrow upstairs hall. Rooms can be rented for the night.".into(),
            Location::StorageRoom => {
                if !world.player.aura_cleared {
                    "A crushing dread pins you in place. Someone waits in the dark ahead.".into()
                } else {
                    "The dread has lifted. A winged, smoke-wreathed figure regards you.".into()
                }
            }
        };
        let extras: Vec<String> = self
            .ambient_here(world.location)
            .into_iter()
            .map(|a| a.card.description.clone())
            .filter(|d| !d.is_empty())
            .take(2)
            .collect();
        if !extras.is_empty() {
            base.push(' ');
            base.push_str(&extras.join(" "));
        }
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::WorldState;

    #[test]
    fn outside_default_is_dewr_when_alone() {
        let world = WorldState::default();
        let coord = SceneCoordinator::new();
        let snap = coord.snapshot(&world);
        assert_eq!(snap.visible_npcs, vec![NpcId::Dewr]);
        assert_eq!(snap.default_talk, Some(NpcId::Dewr));
    }

    #[test]
    fn hall_is_ambiguous_with_sally_and_groll() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        world.player.dewr_scam_cleared = true;
        let coord = SceneCoordinator::new();
        let snap = coord.snapshot(&world);
        assert!(snap.visible_npcs.contains(&NpcId::Sally));
        assert!(snap.visible_npcs.contains(&NpcId::Groll));
        assert!(snap.default_talk.is_none());
        assert_eq!(
            coord.resolve_talk_target(&world, None),
            Err(TalkMiss::Ambiguous)
        );
        assert_eq!(
            coord.resolve_talk_target(&world, Some(TalkRequest::Quest(NpcId::Groll))),
            Ok(TalkFocus::Quest(NpcId::Groll))
        );
    }

    #[test]
    fn goblin_canonicalizes_to_dewr_outside() {
        let world = WorldState::default();
        assert_eq!(
            SceneCoordinator::canonicalize_npc(&world, NpcId::Goblin),
            NpcId::Dewr
        );
    }

    #[test]
    fn last_talk_sticky_when_still_present() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        world.player.dewr_scam_cleared = true;
        let mut coord = SceneCoordinator::new();
        coord.note_talk(&TalkFocus::Quest(NpcId::Groll));
        assert_eq!(
            coord.resolve_talk_target(&world, None),
            Ok(TalkFocus::Quest(NpcId::Groll))
        );
    }

    #[test]
    fn ambient_register_and_talk_by_name() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        world.player.dewr_scam_cleared = true;
        let mut coord = SceneCoordinator::new();
        coord.register_ambient(
            Location::MainHall,
            vec![CharacterCard::ambient(
                "carter",
                "a tired carter",
                "weary",
                "A carter by the hearth.",
                "plain",
            )],
        );
        assert!(coord.find_ambient(Location::MainHall, "carter").is_some());
        assert_eq!(
            coord.resolve_talk_target(&world, Some(TalkRequest::Name("carter".into()))),
            Ok(TalkFocus::Ambient("carter".into()))
        );
        // Reserved ids dropped.
        coord.register_ambient(
            Location::MainHall,
            vec![CharacterCard::ambient("dewr", "Fake", "x", "x", "x")],
        );
        assert!(coord.find_ambient(Location::MainHall, "dewr").is_none());
    }

    #[test]
    fn paint_fingerprint_ignores_ambient_seed() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        let mut coord = SceneCoordinator::new();
        let before = coord.paint_fingerprint(&world);
        coord.register_ambient(
            Location::MainHall,
            vec![crate::cards::CharacterCard::ambient(
                "carter",
                "a tired carter",
                "weary",
                "A road-dusted carter nurses a mug.",
                "plain",
            )],
        );
        assert_eq!(before, coord.paint_fingerprint(&world));
        assert_ne!(before, coord.fingerprint(&world));
    }

    #[test]
    fn fingerprint_includes_ambient() {
        let world = WorldState::default();
        let mut coord = SceneCoordinator::new();
        let before = coord.fingerprint(&world);
        coord.register_ambient(
            Location::Outside,
            SceneCoordinator::fallback_ambient(Location::Outside),
        );
        assert_ne!(before, coord.fingerprint(&world));
    }
}
