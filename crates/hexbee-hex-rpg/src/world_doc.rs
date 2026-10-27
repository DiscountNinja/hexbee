//! World file — YAML soft details, with a Markdown reader for older notes.
//!
//! Plain notes stay hidden until the player looks for them. A detail the
//! narrator just added is marked noticed by the caller.

use serde::{Deserialize, Serialize};

use crate::scene::SceneCoordinator;
use crate::state::{Location, WorldState};

/// What a soft detail is, when it is more than a one-line note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftKind {
    Object,
    Person,
    Animal,
}

/// Local stake on one soft detail. Never a plot grant.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SoftStake {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attitude: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heat: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<i32>,
    /// Bounded check, 8–16. Success changes only this detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dc: Option<i32>,
}

/// One soft detail. Kind, name, and voice are omitted on an old one-line note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftFlag {
    pub id: String,
    pub location: Location,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<SoftKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stake: Option<SoftStake>,
}

/// Champion proposal for one new soft detail. The program validates before append.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftProposal {
    pub kind: SoftKind,
    pub name: String,
    pub text: String,
    pub voice: String,
    pub stake: SoftStake,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorldDoc {
    pub title: String,
    pub blurb: String,
    pub tone: String,
    pub hard_flags: Vec<String>,
    pub soft_flags: Vec<SoftFlag>,
    dirty: bool,
}

impl WorldDoc {
    pub fn parse(raw: &str) -> Self {
        if looks_like_yaml_world(raw) {
            if let Some(doc) = parse_yaml_world(raw) {
                return doc;
            }
        }
        Self {
            title: String::new(),
            blurb: String::new(),
            tone: String::new(),
            hard_flags: parse_section_bullets(raw, "Hard flags"),
            soft_flags: parse_section_bullets(raw, "Soft flags")
                .into_iter()
                .filter_map(|line| parse_soft_line(&line))
                .collect(),
            dirty: false,
        }
    }

    pub fn bundled() -> Self {
        Self::parse(include_str!("../campaign/world.yaml"))
    }

    pub fn take_dirty(&mut self) -> bool {
        let dirty = self.dirty;
        self.dirty = false;
        dirty
    }

    pub fn to_yaml(&self) -> String {
        let file = WorldFile {
            title: self.title.clone(),
            blurb: self.blurb.clone(),
            tone: self.tone.clone(),
            hard_flags: self.hard_flags.clone(),
            soft_details: self
                .soft_flags
                .iter()
                .filter_map(|flag| flag.to_file())
                .collect(),
        };
        serde_yaml::to_string(&file).unwrap_or_default()
    }

    /// Person and animal details at this location — the addressable cast.
    pub fn cast_at(&self, loc: Location) -> Vec<&SoftFlag> {
        self.at_location(loc)
            .filter(|f| matches!(f.kind, Some(SoftKind::Person | SoftKind::Animal)))
            .collect()
    }

    /// Append one validated detail. Rejects quest names and plot grants.
    pub fn accept_proposal(
        &mut self,
        location: Location,
        proposal: SoftProposal,
    ) -> Result<SoftFlag, String> {
        let name = proposal.name.trim();
        if name.is_empty() && matches!(proposal.kind, SoftKind::Person | SoftKind::Animal) {
            return Err("a person or animal needs a name".into());
        }
        let text = proposal.text.trim();
        if text.is_empty() {
            return Err("soft detail needs text".into());
        }
        validate_stake(&proposal.stake)?;
        if mentions_blocked(name) || mentions_blocked(text) {
            return Err("that would change the story".into());
        }
        let mut id = slug_id(if name.is_empty() { text } else { name });
        let mut n = 2u32;
        while self.soft_flags.iter().any(|f| f.id == id) {
            id = format!("{}_{n}", slug_id(if name.is_empty() { text } else { name }));
            n += 1;
            if n > 20 {
                return Err("too many details with that name".into());
            }
        }
        let flag = SoftFlag {
            id,
            location,
            text: text.to_string(),
            kind: Some(proposal.kind),
            name: if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            },
            voice: {
                let voice = proposal.voice.trim();
                if voice.is_empty() {
                    None
                } else {
                    Some(voice.to_string())
                }
            },
            stake: stake_or_none(&proposal.stake),
        };
        self.soft_flags.push(flag.clone());
        self.dirty = true;
        Ok(flag)
    }

    pub fn at_location(&self, loc: Location) -> impl Iterator<Item = &SoftFlag> {
        self.soft_flags
            .iter()
            .filter(move |f| f.location == loc)
    }

    pub fn is_discovered(&self, world: &WorldState, id: &str) -> bool {
        world.discovered_soft.iter().any(|d| d == id)
    }

    pub fn discover(&self, world: &mut WorldState, id: &str) {
        if self.soft_flags.iter().any(|f| f.id == id)
            && !self.is_discovered(world, id)
        {
            world.discovered_soft.push(id.to_string());
        }
    }

    /// Facts for the LLM — only soft details the player has already noticed here.
    pub fn discovered_facts(&self, world: &WorldState) -> Vec<String> {
        self.at_location(world.location)
            .filter(|f| self.is_discovered(world, &f.id))
            .map(|f| format!("noticed={}", f.text))
            .collect()
    }

    /// Sticky Present lines for details already observed in this room.
    pub fn discovered_present_lines(&self, world: &WorldState) -> Vec<String> {
        self.at_location(world.location)
            .filter(|f| self.is_discovered(world, &f.id))
            .map(|f| format!("· {}", f.text))
            .collect()
    }

    /// Best undiscovered soft flag matching a look / watch / search line.
    pub fn match_undiscovered_search(
        &self,
        world: &WorldState,
        scene: &SceneCoordinator,
        query: &str,
    ) -> Option<&SoftFlag> {
        self.at_location(world.location)
            .filter(|f| !self.is_discovered(world, &f.id))
            .filter(|f| npc_requirement_met(f, world, scene))
            .filter(|f| query_matches_flag(query, &f.text))
            .max_by_key(|f| overlap_score(query, &f.text))
    }

    /// Discovered soft detail the player is acting on (pet, touch, etc.).
    pub fn match_discovered_interaction(
        &self,
        world: &WorldState,
        query: &str,
    ) -> Option<&SoftFlag> {
        self.at_location(world.location)
            .filter(|f| self.is_discovered(world, &f.id))
            .filter(|f| query_matches_flag(query, &f.text))
            .max_by_key(|f| overlap_score(query, &f.text))
    }

    /// Undiscovered soft detail named in an action — binding miss label.
    pub fn undiscovered_action_target(
        &self,
        world: &WorldState,
        scene: &SceneCoordinator,
        query: &str,
    ) -> Option<String> {
        let flag = self
            .at_location(world.location)
            .filter(|f| !self.is_discovered(world, &f.id))
            .filter(|f| npc_requirement_met(f, world, scene))
            .filter(|f| query_matches_flag(query, &f.text))
            .max_by_key(|f| overlap_score(query, &f.text))?;
        Some(flag.short_label())
    }

    /// Room-wide “are there X?” — only answered from discovered soft truth.
    pub fn discovered_room_answer(&self, world: &WorldState, query: &str) -> Option<String> {
        let q = query.to_lowercase();
        let flag = self
            .at_location(world.location)
            .filter(|f| self.is_discovered(world, &f.id))
            .filter(|f| category_matches_query(&q, &f.text))
            .max_by_key(|f| overlap_score(query, &f.text))?;
        Some(format!("Yes — {}.", soften_for_answer(&flag.text)))
    }

    pub fn discovered_has_animals(&self, world: &WorldState) -> bool {
        self.at_location(world.location)
            .filter(|f| self.is_discovered(world, &f.id))
            .any(|f| mentions_animal(&f.text))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorldFile {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    blurb: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    tone: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hard_flags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    soft_details: Vec<SoftDetailFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SoftDetailFile {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    id: String,
    location: String,
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<SoftKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stake: Option<SoftStake>,
}

impl SoftFlag {
    fn to_file(&self) -> Option<SoftDetailFile> {
        Some(SoftDetailFile {
            id: self.id.clone(),
            location: yaml_location(self.location).to_string(),
            text: self.text.clone(),
            kind: self.kind,
            name: self.name.clone(),
            voice: self.voice.clone(),
            stake: self.stake.clone().filter(|s| stake_or_none(s).is_some()),
        })
    }
}

fn yaml_location(loc: Location) -> &'static str {
    match loc {
        Location::Outside => "outside",
        Location::MainHall => "main_hall",
        Location::Kitchen => "kitchen",
        Location::Upstairs => "upstairs",
        Location::StorageRoom => "storage_room",
    }
}

fn looks_like_yaml_world(raw: &str) -> bool {
    let t = raw.trim_start();
    t.starts_with("title:")
        || t.starts_with("tone:")
        || t.starts_with("hard_flags:")
        || t.starts_with("soft_details:")
        || t.starts_with("---")
}

fn parse_yaml_world(raw: &str) -> Option<WorldDoc> {
    let file: WorldFile = serde_yaml::from_str(raw).ok()?;
    let mut soft_flags = Vec::new();
    for detail in file.soft_details {
        let Some(location) = Location::parse(&detail.location) else {
            continue;
        };
        let text = detail.text.trim();
        if text.is_empty() {
            continue;
        }
        let id = if detail.id.trim().is_empty() {
            slug_id(text)
        } else {
            detail.id.trim().to_string()
        };
        soft_flags.push(SoftFlag {
            id,
            location,
            text: text.to_string(),
            kind: detail.kind,
            name: detail.name.filter(|n| !n.trim().is_empty()),
            voice: detail.voice.filter(|n| !n.trim().is_empty()),
            stake: detail.stake.as_ref().and_then(stake_or_none),
        });
    }
    Some(WorldDoc {
        title: file.title,
        blurb: file.blurb,
        tone: file.tone,
        hard_flags: file.hard_flags,
        soft_flags,
        dirty: false,
    })
}

fn stake_or_none(stake: &SoftStake) -> Option<SoftStake> {
    if stake.attitude.is_none() && stake.heat.is_none() && stake.price.is_none() && stake.dc.is_none()
    {
        None
    } else {
        Some(stake.clone())
    }
}

fn validate_stake(stake: &SoftStake) -> Result<(), String> {
    if let Some(dc) = stake.dc {
        if !(8..=16).contains(&dc) {
            return Err("dc must be from 8 to 16".into());
        }
    }
    if let Some(price) = stake.price {
        if !(0..=50).contains(&price) {
            return Err("price must be from 0 to 50".into());
        }
    }
    if let Some(attitude) = stake.attitude {
        if !(-2..=2).contains(&attitude) {
            return Err("attitude must be from -2 to 2".into());
        }
    }
    if let Some(heat) = stake.heat {
        if !(-1..=1).contains(&heat) {
            return Err("heat must be -1, 0, or 1".into());
        }
    }
    Ok(())
}

fn mentions_blocked(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("door tax") || lower.contains("amulet") || lower.contains("password") {
        return true;
    }
    for word in ["dewr", "sally", "groll", "dremis", "warlord", "wench"] {
        if word_in_text(&lower, word) {
            return true;
        }
    }
    false
}

fn parse_section_bullets(md: &str, section: &str) -> Vec<String> {
    let needle = format!("## {section}");
    let Some(start) = md.find(&needle) else {
        return Vec::new();
    };
    let rest = &md[start + needle.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest[..end]
        .lines()
        .filter_map(|line| {
            let t = line.trim();
            t.strip_prefix("- ")
                .or_else(|| t.strip_prefix("* "))
                .map(str::trim)
                .map(str::to_string)
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// `main_hall: sleeping kitty by the hearth`
fn parse_soft_line(line: &str) -> Option<SoftFlag> {
    let (loc_str, text) = line.split_once(':')?;
    let location = Location::parse(loc_str.trim())?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(SoftFlag {
        id: slug_id(text),
        location,
        text: text.to_string(),
        kind: None,
        name: None,
        voice: None,
        stake: None,
    })
}

fn slug_id(text: &str) -> String {
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .take(5)
        .collect();
    if words.is_empty() {
        "soft".into()
    } else {
        words.join("_").to_lowercase()
    }
}

fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(str::to_string)
        .collect()
}

const STOP: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "there", "will", "while", "when", "from", "your",
    "you", "are", "has", "have",
];

fn significant_tokens(s: &str) -> Vec<String> {
    tokenize(s)
        .into_iter()
        .filter(|w| !STOP.contains(&w.as_str()))
        .collect()
}

fn overlap_score(query: &str, flag_text: &str) -> usize {
    let q = significant_tokens(query);
    let f = significant_tokens(flag_text);
    q.iter().filter(|w| f.iter().any(|t| t == *w || t.contains(w.as_str()))).count()
}

fn query_matches_flag(query: &str, flag_text: &str) -> bool {
    let q = significant_tokens(query);
    let f = significant_tokens(flag_text);
    if q.is_empty() || f.is_empty() {
        return false;
    }
    let hits = overlap_score(query, flag_text);
    if hits >= 2 {
        return true;
    }
    if hits == 1 {
        return q.iter().any(|w| f.iter().any(|t| t == w));
    }
    // Category search: "pets" / "animals" against an animal soft flag.
    if mentions_animal(query) && mentions_animal(flag_text) {
        return true;
    }
    false
}

fn category_matches_query(query: &str, flag_text: &str) -> bool {
    if mentions_animal(query) {
        return mentions_animal(flag_text);
    }
    query_matches_flag(query, flag_text)
}

fn mentions_animal(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "animal", "animals", "pet", "pets", "kitty", "kitten", "cat", "cats", "dog", "dogs",
    ]
    .iter()
    .any(|w| word_in_text(&lower, w))
}

fn word_in_text(hay: &str, word: &str) -> bool {
    hay.split(|c: char| !c.is_alphanumeric())
        .any(|w| w == word)
}

fn npc_requirement_met(flag: &SoftFlag, world: &WorldState, _scene: &SceneCoordinator) -> bool {
    let lower = flag.text.to_lowercase();
    let visible: Vec<String> = SceneCoordinator::visible_npcs(world)
        .iter()
        .map(|id| id.display_name().to_lowercase())
        .collect();
    // Named quest cast must be on stage when the flag is about them — not species words in prose.
    for name in ["sally", "dewr", "groll", "dremis"] {
        if word_in_text(&lower, name) {
            return visible.iter().any(|v| v.contains(name));
        }
    }
    true
}

fn soften_for_answer(text: &str) -> String {
    let t = text.trim();
    let lower = t.to_lowercase();
    if let Some(rest) = lower.strip_prefix("there's a ") {
        return rest.to_string();
    }
    if let Some(rest) = lower.strip_prefix("there is a ") {
        return rest.to_string();
    }
    t.to_string()
}

impl SoftFlag {
    pub fn short_label(&self) -> String {
        significant_tokens(&self.text)
            .first()
            .cloned()
            .unwrap_or_else(|| "that".into())
    }
}

/// True for lines that hunt a specific soft detail — not a room-wide survey.
pub fn is_soft_search_line(lower: &str) -> bool {
    if is_room_survey_line(lower) {
        return false;
    }
    lower.contains("look for")
        || lower.contains("search for")
        || lower.contains("watch ")
        || lower.contains("observe ")
        || lower.contains("look at")
        || lower.contains("look around the")
        || lower.contains("examine ")
}

/// Room-wide look / survey — reveals fixtures; must not skip `survey_location`.
pub fn is_room_survey_line(lower: &str) -> bool {
    let t = lower.trim();
    t == "look"
        || t == "l"
        || t == "survey"
        || t == "look around"
        || t == "examine room"
        || t.contains("look around")
        || t.contains("survey the")
        || t.contains("survey room")
        || t.contains("observe the room")
        || t.contains("observe room")
        || t.contains("observe surroundings")
        || t.contains("look closer")
        || t.contains("look closely")
        || t.contains("look over the room")
        || t.contains("look about")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# Test

## Soft flags
- main_hall: There's a sleeping kitty by the fireplace
- main_hall: Sally will hum while working
- kitchen: Kitchen cook is a female goblin
"#;

    #[test]
    fn parses_location_prefixed_soft_flags() {
        let doc = WorldDoc::parse(SAMPLE);
        assert_eq!(doc.soft_flags.len(), 3);
        assert_eq!(doc.soft_flags[0].location, Location::MainHall);
    }

    #[test]
    fn soft_hidden_until_discovered() {
        let doc = WorldDoc::parse(SAMPLE);
        let world = WorldState::default();
        assert!(doc.discovered_present_lines(&world).is_empty());
        assert!(doc.discovered_facts(&world).is_empty());
    }

    #[test]
    fn look_for_matches_undiscovered() {
        let doc = WorldDoc::parse(SAMPLE);
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        let scene = SceneCoordinator::new();
        let hit = doc.match_undiscovered_search(&world, &scene, "i look for pets");
        assert!(hit.is_some());
    }

    #[test]
    fn ignores_lines_without_location_prefix() {
        let doc = WorldDoc::parse(
            "## Soft flags\n- There's a sleeping kitty by the fireplace\n",
        );
        assert!(doc.soft_flags.is_empty());
    }

    #[test]
    fn look_for_cook_in_kitchen() {
        let doc = WorldDoc::parse(SAMPLE);
        let mut world = WorldState::default();
        world.location = Location::Kitchen;
        let scene = SceneCoordinator::new();
        let hit = doc.match_undiscovered_search(&world, &scene, "i look for the cook");
        assert!(hit.is_some(), "{:?}", doc.soft_flags);
    }

    #[test]
    fn yaml_world_keeps_plain_notes_and_headers() {
        let doc = WorldDoc::bundled();
        assert_eq!(doc.title, "The Crooked Lantern");
        assert_eq!(doc.soft_flags.len(), 3);
        assert!(doc.soft_flags.iter().all(|f| f.kind.is_none()));
        assert!(doc.hard_flags.len() >= 4);
    }

    #[test]
    fn proposal_rejects_plot_and_accepts_an_animal() {
        let mut doc = WorldDoc::bundled();
        let bad = doc.accept_proposal(
            Location::MainHall,
            SoftProposal {
                kind: SoftKind::Object,
                name: "amulet".into(),
                text: "The Amulet of Light sits on the bar.".into(),
                voice: String::new(),
                stake: SoftStake::default(),
            },
        );
        assert!(bad.is_err());
        let ok = doc
            .accept_proposal(
                Location::MainHall,
                SoftProposal {
                    kind: SoftKind::Animal,
                    name: "Moth".into(),
                    text: "A grey cat sleeps by the hearth.".into(),
                    voice: "purr".into(),
                    stake: SoftStake {
                        dc: Some(10),
                        ..SoftStake::default()
                    },
                },
            )
            .expect("animal");
        assert_eq!(ok.kind, Some(SoftKind::Animal));
        assert!(doc.take_dirty());
        let again = WorldDoc::parse(&doc.to_yaml());
        assert!(again.cast_at(Location::MainHall).iter().any(|f| f.name.as_deref() == Some("Moth")));
    }

    #[test]
    fn watch_sally_matches_hum_flag() {
        let doc = WorldDoc::parse(SAMPLE);
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        let scene = SceneCoordinator::new();
        let hit = doc.match_undiscovered_search(&world, &scene, "i watch sally");
        assert!(hit.is_some());
    }
}
