//! Entity binding — scene store truth when a line targets something not here.
//!
//! Observation decision tree (player language → what the store lists):
//!
//! ```text
//! who / anyone / Present     → People (NPC roster; not props)
//! look around / smell / hear → Noticeable (animals, machines, hearth, smells, sounds)
//! look / what's here / exits → Furniture (stools, bar, doors — mundane interactables)
//! look closer / details      → Fixture (textures, plants, background room nature)
//! look for / search / watch  → Hidden (secrets; skill or aimed trigger only)
//! ```
//!
//! Shallower looks never dump Hidden. Ways out are Furniture so `go` / exit queries
//! work without a survey. Sticky Present is people only.

use crate::scene::SceneCoordinator;
use crate::state::{Location, WorldState};
use crate::world_doc::WorldDoc;

/// How deep a look / query should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ObserveDepth {
    People = 0,
    Noticeable = 1,
    Furniture = 2,
    Fixture = 3,
    Hidden = 4,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PropLayer {
    Noticeable = 1,
    Furniture = 2,
    Fixture = 3,
    Hidden = 4,
}

struct SceneProp {
    id: &'static str,
    label: &'static str,
    layer: PropLayer,
    location: Location,
    /// When false the prop is not in the scene at all.
    available: fn(&WorldState) -> bool,
}

const PROPS: &[SceneProp] = &[
    SceneProp {
        id: "tavern_door",
        label: "tavern door",
        layer: PropLayer::Furniture,
        location: Location::Outside,
        available: |_| true,
    },
    SceneProp {
        id: "lantern_glow",
        label: "lantern light",
        layer: PropLayer::Noticeable,
        location: Location::Outside,
        available: |_| true,
    },
    SceneProp {
        id: "dewr_knife",
        label: "Dewr's knife",
        layer: PropLayer::Hidden,
        location: Location::Outside,
        available: |w| w.dewr.alive,
    },
    SceneProp {
        id: "dewr_coin",
        label: "coin in Dewr's palm",
        layer: PropLayer::Hidden,
        location: Location::Outside,
        available: |w| w.dewr.alive && !w.player.dewr_scam_cleared,
    },
    SceneProp {
        id: "hall_hearth",
        label: "hearth",
        layer: PropLayer::Noticeable,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_smoke",
        label: "woodsmoke",
        layer: PropLayer::Noticeable,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_clatter",
        label: "kitchen clatter",
        layer: PropLayer::Noticeable,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_seating",
        label: "stools & tables",
        layer: PropLayer::Furniture,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_bar",
        label: "bar",
        layer: PropLayer::Furniture,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_kitchen_door",
        label: "kitchen door",
        layer: PropLayer::Furniture,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_stairs",
        label: "stairs up",
        layer: PropLayer::Furniture,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_tavern_door",
        label: "tavern door",
        layer: PropLayer::Furniture,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_beams",
        label: "soot-stained beams",
        layer: PropLayer::Fixture,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_rushes",
        label: "rush-strewn floor",
        layer: PropLayer::Fixture,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_sword_offer",
        label: "a patron eyeing your knife",
        layer: PropLayer::Hidden,
        location: Location::MainHall,
        available: |w| w.sword_offer_available && !w.sword_offer_taken,
    },
    SceneProp {
        id: "kitchen_smell",
        label: "stew smell",
        layer: PropLayer::Noticeable,
        location: Location::Kitchen,
        available: |_| true,
    },
    SceneProp {
        id: "kitchen_stew",
        label: "stew pot",
        layer: PropLayer::Furniture,
        location: Location::Kitchen,
        available: |_| true,
    },
    SceneProp {
        id: "kitchen_food",
        label: "food for sale",
        layer: PropLayer::Furniture,
        location: Location::Kitchen,
        available: |_| true,
    },
    SceneProp {
        id: "upstairs_rooms",
        label: "room doors",
        layer: PropLayer::Furniture,
        location: Location::Upstairs,
        available: |_| true,
    },
    SceneProp {
        id: "storage_crates",
        label: "crates",
        layer: PropLayer::Furniture,
        location: Location::StorageRoom,
        available: |_| true,
    },
];

fn props_here(world: &WorldState) -> Vec<&'static SceneProp> {
    PROPS
        .iter()
        .filter(|p| p.location == world.location && (p.available)(world))
        .collect()
}

fn is_discovered(world: &WorldState, id: &str) -> bool {
    world.discovered_props.iter().any(|d| d == id)
}

fn mark_discovered(world: &mut WorldState, id: &str) {
    if !is_discovered(world, id) {
        world.discovered_props.push(id.into());
    }
}

/// Map a look / query line onto the observation tree.
pub fn observe_depth(lower: &str) -> ObserveDepth {
    let t = lower.trim();
    if t.contains("look for")
        || t.contains("search for")
        || t.contains("search the")
        || t.starts_with("search ")
        || t.contains("watch ")
    {
        return ObserveDepth::Hidden;
    }
    if t.contains("look closer")
        || t.contains("look closely")
        || t.contains("examine room")
        || t.contains("examine the room")
        || t.contains("room detail")
        || t.contains("texture")
        || t.contains("plants")
        || t.contains("look at the room")
        || t.contains("study the room")
    {
        return ObserveDepth::Fixture;
    }
    if t.contains("look around")
        || t.contains("observe the room")
        || t.contains("observe room")
        || t.contains("observe surroundings")
        || t.contains("smell")
        || t.contains("listen")
        || word_in_text(t, "sound")
        || word_in_text(t, "sounds")
        || t.contains("animal")
        || t.contains("animals")
    {
        return ObserveDepth::Noticeable;
    }
    if t.starts_with("who ")
        || t.starts_with("anyone")
        || t.contains("anyone here")
        || t.contains("who is here")
    {
        return ObserveDepth::People;
    }
    ObserveDepth::Furniture
}

fn props_listed_at(world: &WorldState, depth: ObserveDepth) -> Vec<&'static str> {
    props_here(world)
        .into_iter()
        .filter(|p| match p.layer {
            PropLayer::Hidden => {
                depth == ObserveDepth::Hidden && is_discovered(world, p.id)
            }
            other => (other as u8) <= (depth as u8) && depth >= ObserveDepth::Noticeable,
        })
        .map(|p| p.label)
        .collect()
}

fn furniture_props(world: &WorldState) -> Vec<&'static str> {
    props_here(world)
        .into_iter()
        .filter(|p| p.layer == PropLayer::Furniture)
        .map(|p| p.label)
        .collect()
}

fn exit_props_here(world: &WorldState) -> Vec<&'static str> {
    furniture_props(world)
        .into_iter()
        .filter(|p| is_exit_label(p))
        .collect()
}

/// Labels of the ways out of the current room.
pub fn ways_out(world: &WorldState) -> Vec<&'static str> {
    exit_props_here(world)
}

/// One exit in this room named by the line. Two matches stay unresolved.
pub fn travel_exit(world: &WorldState, text: &str) -> Option<Location> {
    let lower = text.to_lowercase();
    let mut hits = Vec::new();
    for label in exit_props_here(world) {
        let Some(dest) = exit_leads_to(world.location, label) else {
            continue;
        };
        if exit_mentioned(&lower, label) && !hits.contains(&dest) {
            hits.push(dest);
        }
    }
    if hits.len() == 1 {
        Some(hits[0])
    } else {
        None
    }
}

fn exit_leads_to(here: Location, label: &str) -> Option<Location> {
    let label = label.to_lowercase();
    if label.contains("stairs") {
        return Some(Location::Upstairs);
    }
    if label.contains("kitchen") {
        return Some(Location::Kitchen);
    }
    if label.contains("storage") || label.contains("cellar") {
        return Some(Location::StorageRoom);
    }
    if label.contains("tavern") {
        return Some(if here == Location::Outside {
            Location::MainHall
        } else {
            Location::Outside
        });
    }
    if label.contains("hall") {
        return Some(Location::MainHall);
    }
    None
}

fn exit_mentioned(line: &str, label: &str) -> bool {
    let tokens: Vec<&str> = label
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 4 && *word != "door")
        .collect();
    if tokens.is_empty() {
        return false;
    }
    let words: Vec<&str> = line
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    tokens.iter().any(|token| {
        words.iter().any(|word| word == token || word.starts_with(token))
    })
}

/// Look-around does not dump Hidden. Kept for call sites / tests.
pub fn survey_location(_world: &mut WorldState) {}

pub fn try_discover_hidden(world: &mut WorldState, query: &str) -> Vec<&'static str> {
    let q = query.to_lowercase();
    let hits: Vec<(&'static str, &'static str)> = props_here(world)
        .into_iter()
        .filter(|p| p.layer == PropLayer::Hidden && !is_discovered(world, p.id))
        .filter(|p| {
            let label = p.label.to_lowercase();
            q.split(|c: char| !c.is_alphanumeric()).any(|w| {
                !w.is_empty() && w.len() > 2 && label.contains(w)
            })
        })
        .map(|p| (p.id, p.label))
        .collect();
    let mut found = Vec::new();
    for (id, label) in hits {
        mark_discovered(world, id);
        found.push(label);
    }
    found
}

/// Sticky Present is people only — props are listed by look/query depth.
pub fn obvious_interactables(_world: &WorldState) -> Vec<&'static str> {
    Vec::new()
}

/// Props the player may target in act-bind prompts (furniture + discovered hidden).
pub fn interactables_for_location(world: &WorldState) -> Vec<&'static str> {
    let mut out = furniture_props(world);
    for p in props_here(world)
        .into_iter()
        .filter(|p| p.layer == PropLayer::Hidden && is_discovered(world, p.id))
    {
        out.push(p.label);
    }
    out
}

pub fn scene_has_animals(world: &WorldState, scene: &SceneCoordinator, doc: &WorldDoc) -> bool {
    if doc.discovered_has_animals(world) {
        return true;
    }
    if world.location == Location::Outside {
        return scene.ambient_here(world.location).iter().any(|a| {
            let id = a.id.to_lowercase();
            let name = a.card.name.to_lowercase();
            id.contains("dog") || name.contains("dog") || name.contains("animal")
        });
    }
    false
}

fn mentions_any(lower: &str, words: &[&str]) -> bool {
    words.iter().any(|w| word_in_text(lower, w))
}

fn word_in_text(hay: &str, word: &str) -> bool {
    hay.split(|c: char| !c.is_alphanumeric())
        .any(|w| w == word)
}

fn is_room_question(lower: &str) -> bool {
    lower.starts_with("are there")
        || lower.starts_with("is there")
        || lower.starts_with("do you have any")
        || lower.starts_with("anyone ")
        || lower.starts_with("anybody ")
        || lower == "anyone"
        || lower == "anybody"
        || lower.contains("anyone here")
        || lower.contains("anybody here")
        || lower.contains("anyone around")
        || lower.contains("anybody around")
        || lower.contains("who is here")
        || lower.contains("who's here")
        || lower.contains("whos here")
        || (lower.contains('?') && (lower.contains("any ") || lower.contains("are there")))
}

/// Physical line clearly aimed at a specific object/category (not a room-wide question).
pub fn is_entity_directed_action(lower: &str) -> bool {
    if is_room_question(lower) {
        return false;
    }
    if lower.contains("look for") || lower.contains("search for") {
        return false;
    }
    lower.contains("pet ")
        || lower.contains("stroke ")
        || lower.contains("pat ")
        || lower.contains("touch ")
        || lower.contains("grab ")
        || lower.contains("pick up")
        || lower.contains("take the ")
        || lower.contains("the sleeping")
}

/// Named thing in the line that is not in lexicon + scene props + present cast.
pub fn detect_missing_entity(
    normalized: &str,
    world: &WorldState,
    scene: &SceneCoordinator,
    doc: &WorldDoc,
) -> Option<String> {
    let lower = normalized.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }

    if is_entity_directed_action(&lower) {
        if let Some(label) = doc.undiscovered_action_target(world, scene, &lower) {
            return Some(label);
        }
    }

    if mentions_any(&lower, &["kitty", "kitten", "cat", "cats"]) && !scene_has_animals(world, scene, doc) {
        let label = if mentions_any(&lower, &["kitty", "kitten"]) {
            "kitty"
        } else {
            "cat"
        };
        return Some(label.into());
    }
    if mentions_any(&lower, &["pets", "pet"]) && !scene_has_animals(world, scene, doc) {
        return Some("pets".into());
    }
    if mentions_any(&lower, &["animals", "animal"])
        && !is_room_question(&lower)
        && !scene_has_animals(world, scene, doc)
    {
        return Some("animals".into());
    }

    for (name, aliases) in [
        ("Dremis", &["dremis", "warlord"][..]),
        ("Sally", &["sally", "wench"][..]),
        ("Groll", &["groll"][..]),
    ] {
        if aliases.iter().any(|a| word_in_text(&lower, a))
            && !SceneCoordinator::visible_npcs(world)
                .iter()
                .any(|id| id.display_name().eq_ignore_ascii_case(name))
        {
            return Some(name.into());
        }
    }

    None
}

pub fn missing_entity_line(name: &str) -> String {
    match name {
        "pets" | "animals" => format!("There are no {name} here."),
        _ => format!("There is no {name} here."),
    }
}

pub fn talk_unbound_line(world: &WorldState, scene: &SceneCoordinator) -> String {
    let mut names: Vec<String> = SceneCoordinator::visible_npcs(world)
        .iter()
        .map(|id| id.display_name().to_string())
        .collect();
    for a in scene.ambient_here(world.location) {
        let n = a.card.name.trim();
        if !n.is_empty() {
            names.push(n.to_string());
        }
    }
    if names.is_empty() {
        return "There's no one here to talk to.".into();
    }
    format!("Who are you speaking to? {} / room", names.join(" / "))
}

pub fn act_unbound_line(world: &WorldState) -> String {
    let props = interactables_for_location(world);
    if props.is_empty() {
        return "Act on what? Say what you mean to touch, use, or move.".into();
    }
    let mut line = props.join(" / ");
    if props_here(world)
        .iter()
        .any(|p| p.layer == PropLayer::Hidden && !is_discovered(world, p.id))
    {
        line.push_str(" / …");
    }
    format!("Act on what? {line}")
}

/// Short store read for look-bind / search misses — not a full inventory dump.
pub fn look_scene_line(world: &WorldState, _scene: &SceneCoordinator) -> String {
    look_scene_line_at(world, ObserveDepth::Furniture)
}

pub fn look_scene_line_at(world: &WorldState, depth: ObserveDepth) -> String {
    if depth == ObserveDepth::People {
        return "People are listed under Present.".into();
    }
    let listed = props_listed_at(world, depth);
    if listed.is_empty() {
        return "Nothing at that glance.".into();
    }
    let mut out = listed.join(" · ");
    match depth {
        ObserveDepth::Noticeable => {
            out.push_str(" — look (or ask what's here) for furniture and ways out.");
        }
        ObserveDepth::Furniture => {
            if props_here(world)
                .iter()
                .any(|p| p.layer == PropLayer::Fixture)
            {
                out.push_str(" — look closer for room details.");
            }
        }
        ObserveDepth::Fixture => {}
        ObserveDepth::Hidden | ObserveDepth::People => {}
    }
    out
}

/// Answer “are there X here?” from store truth (one line).
pub fn room_presence_answer(
    lower: &str,
    world: &WorldState,
    scene: &SceneCoordinator,
    doc: &WorldDoc,
) -> Option<String> {
    if !is_room_question(lower) {
        return None;
    }
    if mentions_any(
        lower,
        &[
            "animal", "animals", "pet", "pets", "kitty", "kitten", "cat", "cats", "dog", "dogs",
        ],
    ) {
        if let Some(ans) = doc.discovered_room_answer(world, lower) {
            return Some(ans);
        }
        if scene_has_animals(world, scene, doc) {
            let animal = scene
                .ambient_here(world.location)
                .iter()
                .find(|a| {
                    let id = a.id.to_lowercase();
                    let name = a.card.name.to_lowercase();
                    id.contains("dog") || name.contains("dog")
                })
                .map(|a| a.card.name.clone())
                .unwrap_or_else(|| "something".into());
            return Some(format!("Yes — {animal}."));
        }
        return Some("No animals here.".into());
    }
    if mentions_any(
        lower,
        &["person", "people", "anyone", "anybody", "somebody", "npc"],
    ) || ((lower.contains("who is here")
        || lower.contains("who's here")
        || lower.contains("whos here")
        || lower.contains("around here")
        || lower.contains("around in"))
        && !lower.contains("look around"))
    {
        let who = SceneCoordinator::visible_npcs(world)
            .iter()
            .map(|id| id.display_name())
            .collect::<Vec<_>>()
            .join(", ");
        if who.is_empty() {
            return Some("No one notable here.".into());
        }
        return Some(format!("Yes — {who}."));
    }
    None
}

fn is_exit_label(label: &str) -> bool {
    let l = label.to_lowercase();
    l.contains("door") || l.contains("stairs") || l.contains("exit")
}

fn hidden_props_remaining(world: &WorldState) -> usize {
    props_here(world)
        .into_iter()
        .filter(|p| p.layer == PropLayer::Hidden && !is_discovered(world, p.id))
        .count()
}

fn people_here(world: &WorldState, scene: &SceneCoordinator) -> Vec<String> {
    let mut people: Vec<String> = SceneCoordinator::visible_npcs(world)
        .iter()
        .map(|id| id.display_name().to_string())
        .collect();
    for a in scene.ambient_here(world.location) {
        let n = a.card.name.trim();
        if !n.is_empty() {
            people.push(n.to_string());
        }
    }
    people
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SceneSnapshotFocus {
    Exits,
    General,
}

fn scene_snapshot_focus(lower: &str) -> SceneSnapshotFocus {
    if mentions_any(
        lower,
        &[
            "exit", "exits", "door", "doors", "stairs", "way out", "ways out", "leave from",
            "get out", "go out",
        ],
    ) {
        SceneSnapshotFocus::Exits
    } else {
        SceneSnapshotFocus::General
    }
}

/// Store-truth room read for Query mode — depth from the observation tree.
pub fn room_scene_snapshot(
    lower: &str,
    world: &WorldState,
    scene: &SceneCoordinator,
) -> String {
    let loc = world.location.display_name();
    let people = people_here(world, scene);
    let depth = observe_depth(lower);

    match scene_snapshot_focus(lower) {
        SceneSnapshotFocus::Exits => {
            let exits = exit_props_here(world);
            if exits.is_empty() {
                return format!("{loc} — no ways out from here.");
            }
            return format!("{loc} — ways out: {}.", exits.join(" · "));
        }
        SceneSnapshotFocus::General => {}
    }

    if depth == ObserveDepth::People {
        if people.is_empty() {
            return format!("{loc} — no one notable here.");
        }
        return format!("{loc}\nPeople: {}.", people.join(", "));
    }

    let listed = props_listed_at(world, depth);
    let exits = exit_props_here(world);
    let mut out = loc.to_string();
    if !people.is_empty() && depth >= ObserveDepth::Noticeable {
        out.push_str(&format!("\nPeople: {}", people.join(", ")));
    }
    let notice: Vec<&str> = listed
        .iter()
        .copied()
        .filter(|p| !is_exit_label(p))
        .collect();
    if !notice.is_empty() {
        let header = match depth {
            ObserveDepth::Noticeable => "Noticeable",
            ObserveDepth::Furniture => "Here",
            ObserveDepth::Fixture => "Details",
            ObserveDepth::Hidden => "Found",
            ObserveDepth::People => "People",
        };
        out.push_str(&format!("\n{header}: {}", notice.join(" · ")));
    }
    if depth >= ObserveDepth::Furniture && !exits.is_empty() {
        out.push_str(&format!("\nWays out: {}", exits.join(" · ")));
    } else if depth == ObserveDepth::Noticeable {
        out.push_str("\n(Look or ask what's here for furniture and ways out.)");
    }
    if depth < ObserveDepth::Fixture
        && props_here(world)
            .iter()
            .any(|p| p.layer == PropLayer::Fixture)
    {
        out.push_str("\n(Look closer for room details.)");
    }
    if hidden_props_remaining(world) > 0 && depth != ObserveDepth::Hidden {
        out.push_str("\n(Search or look for something specific to find secrets.)");
    }
    if people.is_empty() && listed.is_empty() && (depth < ObserveDepth::Furniture || exits.is_empty())
    {
        out.push_str("\nNothing at that glance.");
    }
    out
}

/// True when a Query line is about the player's sheet, not the room.
pub fn is_player_self_query(lower: &str) -> bool {
    lower == "status"
        || lower == "sheet"
        || lower == "skills"
        || lower.contains("do i have")
        || lower.contains("what do i have")
        || lower.contains("my hp")
        || lower.contains("my health")
        || lower.contains("my stats")
        || lower.contains("my skills")
        || lower.contains("my gold")
        || lower.contains("my weapon")
        || lower.contains("my gear")
        || lower.contains("my equipment")
        || lower.contains("check myself")
        || lower.contains("character sheet")
        || lower.contains("my ability")
        || lower.contains("my proficien")
}

/// Answer room/scene questions from store truth (one block, no LLM).
pub fn room_scene_answer(
    lower: &str,
    world: &WorldState,
    scene: &SceneCoordinator,
) -> Option<String> {
    if is_player_self_query(lower) {
        return None;
    }
    Some(room_scene_snapshot(lower, world, scene))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::WorldState;

    #[test]
    fn stairs_in_the_hall_lead_upstairs() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        assert_eq!(travel_exit(&world, "go up the stairs"), Some(Location::Upstairs));
        assert_eq!(travel_exit(&world, "move up the stairs"), Some(Location::Upstairs));
        assert!(travel_exit(&world, "go to the moon").is_none());
        let ways = ways_out(&world);
        assert!(ways.iter().any(|w| w.contains("stairs")));
    }

    #[test]
    fn kitty_missing_until_discovered() {
        let mut w = WorldState::default();
        w.location = Location::MainHall;
        w.player.dewr_scam_cleared = true;
        let scene = SceneCoordinator::new();
        let doc = WorldDoc::bundled();
        assert_eq!(
            detect_missing_entity("i pet the sleeping kitty", &w, &scene, &doc).as_deref(),
            Some("sleeping")
        );
    }

    #[test]
    fn kitty_interact_after_discover() {
        let mut w = WorldState::default();
        w.location = Location::MainHall;
        w.player.dewr_scam_cleared = true;
        let scene = SceneCoordinator::new();
        let doc = WorldDoc::bundled();
        doc.discover(&mut w, &doc.soft_flags[0].id);
        assert!(
            detect_missing_entity("i pet the sleeping kitty", &w, &scene, &doc).is_none()
        );
    }

    #[test]
    fn room_question_not_missing_entity() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        world.player.dewr_scam_cleared = true;
        let scene = SceneCoordinator::new();
        let doc = WorldDoc::bundled();
        assert!(detect_missing_entity("are there any animals in the tavern", &world, &scene, &doc).is_none());
        let ans = room_presence_answer("are there any animals in the tavern", &world, &scene, &doc);
        assert_eq!(ans.as_deref(), Some("No animals here."));
    }

    #[test]
    fn room_question_sees_discovered_soft() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        world.player.dewr_scam_cleared = true;
        let scene = SceneCoordinator::new();
        let doc = WorldDoc::bundled();
        doc.discover(&mut world, &doc.soft_flags[0].id);
        let ans = room_presence_answer("are there any animals in the tavern", &world, &scene, &doc);
        assert!(ans.as_deref().unwrap().starts_with("Yes —"));
    }

    #[test]
    fn act_unbound_lists_furniture_and_exits() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        let line = act_unbound_line(&world);
        assert!(line.contains("stools"));
        assert!(line.contains("kitchen door"));
        assert!(line.contains("stairs up"));
        assert!(!line.contains("hearth"), "hearth is noticeable, not furniture bind, got {line}");
        assert!(!line.contains("beams"), "fixtures stay off act bind, got {line}");
    }

    #[test]
    fn present_is_people_not_furniture() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        assert!(obvious_interactables(&world).is_empty());
    }

    #[test]
    fn look_around_is_noticeable_not_exits() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        let line = look_scene_line_at(&world, observe_depth("look around, observe the room"));
        assert!(line.contains("hearth"), "got {line}");
        assert!(line.contains("woodsmoke"), "got {line}");
        assert!(!line.contains("kitchen door"), "notice glance must not dump furniture, got {line}");
    }

    #[test]
    fn look_lists_furniture_and_exits() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        let line = look_scene_line_at(&world, observe_depth("look"));
        assert!(line.contains("kitchen door"), "got {line}");
        assert!(line.contains("stools"), "got {line}");
        assert!(!line.contains("beams"), "fixtures need look closer, got {line}");
    }

    #[test]
    fn look_closer_lists_fixtures() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        let line = look_scene_line_at(&world, observe_depth("look closer"));
        assert!(line.contains("beams"), "got {line}");
        assert!(line.contains("kitchen door"), "got {line}");
    }

    #[test]
    fn survey_does_not_dump_hidden() {
        let mut world = WorldState::default();
        world.location = Location::Outside;
        world.dewr.alive = true;
        survey_location(&mut world);
        let line = look_scene_line_at(&world, ObserveDepth::Fixture);
        assert!(!line.contains("knife"), "got {line}");
    }

    #[test]
    fn obvious_exits_are_furniture() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        let scene = SceneCoordinator::new();
        let ans = room_scene_answer("what exits are obvious", &world, &scene).unwrap();
        assert!(ans.contains("ways out"), "got {ans}");
        assert!(ans.contains("kitchen door"), "got {ans}");
    }

    #[test]
    fn exits_after_survey() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        survey_location(&mut world);
        let scene = SceneCoordinator::new();
        let ans = room_scene_answer("what exits are obvious", &world, &scene).unwrap();
        assert!(ans.contains("kitchen door"), "got {ans}");
        assert!(ans.contains("stairs up"), "got {ans}");
    }

    #[test]
    fn anyone_around_is_presence() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        world.player.dewr_scam_cleared = true;
        let scene = SceneCoordinator::new();
        let doc = WorldDoc::bundled();
        let ans = room_presence_answer("anyone around in this hall", &world, &scene, &doc);
        assert!(
            ans.as_deref().unwrap_or("").contains("Sally"),
            "got {ans:?}"
        );
    }

    #[test]
    fn player_self_query_not_scene() {
        assert!(is_player_self_query("what are my stats"));
        assert!(!is_player_self_query("what exits are obvious"));
    }
}
