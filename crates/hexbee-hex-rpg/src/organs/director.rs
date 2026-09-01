//! Director organ — scene beat, focus, presence (no invented items/kills).
//! Runs after committed patches; paints framing for Narrator, never store writes.
#![allow(dead_code)]

use crate::scene::SceneCoordinator;
use crate::state::WorldState;

pub struct DirectorOrgan;

impl DirectorOrgan {
    pub fn beat_summary(world: &WorldState, scene: &SceneCoordinator) -> String {
        format!(
            "scene={} present={}",
            world.location.display_name(),
            scene.snapshot(world).visible_npcs.len()
        )
    }

    pub fn beat_plan(world: &WorldState, scene: &SceneCoordinator, outcome: &str) -> String {
        format!(
            "Director beat: {} | outcome={} | {}",
            world.location.display_name(),
            outcome,
            Self::beat_summary(world, scene)
        )
    }

    /// Continue budget: allow at most `budget` follow-up organ beats (0–1 typical).
    pub fn should_continue(budget: u8) -> bool {
        budget > 0
    }
}
