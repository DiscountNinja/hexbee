//! Sync WorldState with campaign store runtime blob.

use hexbee_store::{ActorSheet, Campaign, StorePatch};
use serde_json::json;

use crate::skills::CharacterBuild;
use crate::state::{Location, WorldState};

pub fn location_to_scene_id(loc: Location) -> &'static str {
    match loc {
        Location::Outside => "outside",
        Location::MainHall => "main_hall",
        Location::Kitchen => "kitchen",
        Location::Upstairs => "upstairs",
        Location::StorageRoom => "storage_room",
    }
}

pub fn scene_id_to_location(id: &str) -> Location {
    match id {
        "main_hall" => Location::MainHall,
        "kitchen" => Location::Kitchen,
        "upstairs" => Location::Upstairs,
        "storage_room" => Location::StorageRoom,
        _ => Location::Outside,
    }
}

pub fn world_to_runtime(world: &WorldState) -> serde_json::Value {
    serde_json::to_value(world).unwrap_or(json!({}))
}

pub fn runtime_to_world(runtime: &serde_json::Value) -> WorldState {
    serde_json::from_value(runtime.clone()).unwrap_or_default()
}

pub fn sync_world_to_campaign(campaign: &mut Campaign, world: &WorldState) {
    campaign.state.current_scene = location_to_scene_id(world.location).into();
    campaign.state.runtime = world_to_runtime(world);
    campaign.dirty = true;
}

pub fn sync_campaign_to_world(campaign: &Campaign) -> WorldState {
    let mut world = runtime_to_world(&campaign.state.runtime);
    world.location = scene_id_to_location(&campaign.state.current_scene);
    world
}

pub fn patches_from_world(_old: &WorldState, new: &WorldState) -> Vec<StorePatch> {
    vec![
        StorePatch::new("runtime", world_to_runtime(new)),
        StorePatch::set_str("state.current_scene", location_to_scene_id(new.location)),
    ]
}

pub fn default_world_from_campaign(campaign: &Campaign) -> WorldState {
    if campaign.state.runtime.is_object() && !campaign.state.runtime.as_object().unwrap().is_empty() {
        let mut world = sync_campaign_to_world(campaign);
        if is_default_player_name(&world.player.name) {
            if let Some(pc) = campaign.actors.get("pc") {
                if !is_default_player_name(&pc.name) {
                    world.player.name = pc.name.clone();
                }
            }
        }
        if let Some(pc) = campaign.actors.get("pc") {
            apply_sheet_build(&mut world, pc);
        }
        world
    } else {
        let mut world = WorldState::default();
        if let Some(seed) = campaign.state.rng_seed {
            let _ = seed;
        }
        world.location = scene_id_to_location(&campaign.state.current_scene);
        if let Some(pc) = campaign.actors.get("pc") {
            if !pc.name.trim().is_empty() {
                world.player.name = pc.name.clone();
            }
            apply_sheet_build(&mut world, pc);
        }
        world
    }
}

fn apply_sheet_build(world: &mut WorldState, pc: &ActorSheet) {
    if let Some(build) = CharacterBuild::from_stats_json(&pc.stats) {
        world.player.build = build;
    }
}

pub fn is_default_player_name(name: &str) -> bool {
    let n = name.trim();
    n.is_empty() || n.eq_ignore_ascii_case("you") || n.eq_ignore_ascii_case("traveler")
}

/// True when the PC sheet still needs character creation.
pub fn needs_chargen(pc: &ActorSheet) -> bool {
    if is_default_player_name(&pc.name) {
        return true;
    }
    let done = pc
        .stats
        .get("chargen_done")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if done {
        return false;
    }
    CharacterBuild::from_stats_json(&pc.stats).is_none()
}
