//! Parse `campaign/world.md` — soft flags are hidden until looked for or observed.

use crate::scene::SceneCoordinator;
use crate::state::{Location, WorldState};

/// One authored soft detail — not shown until the player discovers it in play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftFlag {
    pub id: String,
    pub location: Location,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorldDoc {
    pub soft_flags: Vec<SoftFlag>,
}

impl WorldDoc {
    pub fn parse(md: &str) -> Self {
        Self {
            soft_flags: parse_section_bullets(md, "Soft flags")
                .into_iter()
                .filter_map(|line| parse_soft_line(&line))
                .collect(),
        }
    }

    pub fn bundled() -> Self {
        Self::parse(include_str!("../campaign/world.md"))
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

pub fn is_soft_search_line(lower: &str) -> bool {
    lower.contains("look for")
        || lower.contains("search for")
        || lower.contains("watch ")
        || lower.contains("observe ")
        || lower.contains("look at")
        || lower.contains("look around the")
        || lower.contains("examine ")
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
    fn watch_sally_matches_hum_flag() {
        let doc = WorldDoc::parse(SAMPLE);
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        let scene = SceneCoordinator::new();
        let hit = doc.match_undiscovered_search(&world, &scene, "i watch sally");
        assert!(hit.is_some());
    }
}
