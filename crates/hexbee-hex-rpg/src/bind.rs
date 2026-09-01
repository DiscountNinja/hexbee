//! Entity binding — scene store truth when a line targets something not here.

use crate::scene::SceneCoordinator;
use crate::state::{Location, WorldState};
use crate::world_doc::WorldDoc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PropLayer {
    Obvious,
    Hidden,
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
        layer: PropLayer::Obvious,
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
        id: "hall_seating",
        label: "stools & tables",
        layer: PropLayer::Obvious,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_hearth",
        label: "hearth",
        layer: PropLayer::Obvious,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_bar",
        label: "bar",
        layer: PropLayer::Obvious,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_kitchen_door",
        label: "kitchen door",
        layer: PropLayer::Hidden,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_stairs",
        label: "stairs up",
        layer: PropLayer::Hidden,
        location: Location::MainHall,
        available: |_| true,
    },
    SceneProp {
        id: "hall_tavern_door",
        label: "tavern door",
        layer: PropLayer::Hidden,
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
        id: "kitchen_stew",
        label: "stew pot",
        layer: PropLayer::Obvious,
        location: Location::Kitchen,
        available: |_| true,
    },
    SceneProp {
        id: "kitchen_food",
        label: "food for sale",
        layer: PropLayer::Obvious,
        location: Location::Kitchen,
        available: |_| true,
    },
    SceneProp {
        id: "upstairs_rooms",
        label: "room doors",
        layer: PropLayer::Obvious,
        location: Location::Upstairs,
        available: |_| true,
    },
    SceneProp {
        id: "storage_crates",
        label: "crates",
        layer: PropLayer::Obvious,
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

/// After a deliberate look around, reveal hidden fixtures for this room.
pub fn survey_location(world: &mut WorldState) {
    for p in props_here(world)
        .into_iter()
        .filter(|p| p.layer == PropLayer::Hidden)
    {
        mark_discovered(world, p.id);
    }
}

fn visible_props(world: &WorldState) -> Vec<&'static str> {
    props_here(world)
        .into_iter()
        .filter(|p| p.layer == PropLayer::Obvious || is_discovered(world, p.id))
        .map(|p| p.label)
        .collect()
}

fn obvious_props(world: &WorldState) -> Vec<&'static str> {
    props_here(world)
        .into_iter()
        .filter(|p| p.layer == PropLayer::Obvious)
        .map(|p| p.label)
        .collect()
}

/// Obvious fixtures for the sticky Present roster (not a full inventory).
pub fn obvious_interactables(world: &WorldState) -> Vec<&'static str> {
    obvious_props(world)
}

/// Props the player may target in act-bind prompts (obvious + discovered).
pub fn interactables_for_location(world: &WorldState) -> Vec<&'static str> {
    visible_props(world)
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
    let obvious = obvious_props(world);
    if obvious.is_empty() {
        return "Nothing obvious in reach.".into();
    }
    let mut out = obvious.join(" · ");
    let hidden_left = props_here(world)
        .iter()
        .filter(|p| p.layer == PropLayer::Hidden && !is_discovered(world, p.id))
        .count();
    if hidden_left > 0 {
        out.push_str(" — you could look closer.");
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
    if mentions_any(lower, &["person", "people", "anyone", "somebody", "npc"]) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::WorldState;

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
    fn act_unbound_lists_obvious_only_until_survey() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        let line = act_unbound_line(&world);
        assert!(line.contains("stools"));
        assert!(line.contains("hearth"));
        assert!(!line.contains("kitchen door"));
        assert!(line.contains("…"));
        survey_location(&mut world);
        let after = act_unbound_line(&world);
        assert!(after.contains("kitchen door"));
    }

    #[test]
    fn obvious_roster_is_short() {
        let mut world = WorldState::default();
        world.location = Location::MainHall;
        let items = obvious_interactables(&world);
        assert!(items.len() <= 3);
        assert!(items.contains(&"hearth"));
    }
}
