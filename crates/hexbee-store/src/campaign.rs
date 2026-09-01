use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info};

use crate::actor::ActorSheet;
use crate::config::HexConfig;
use crate::goals::GoalsDoc;
use crate::patch::{path_matches_pattern, StorePatch};
use crate::scene::SceneDef;
use crate::transcript::{TranscriptBeat, TranscriptLog};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("patch path not writable: {path}")]
    PathNotWritable { path: String },
    #[error("missing file: {0}")]
    Missing(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignState {
    pub current_scene: String,
    pub current_actor: String,
    #[serde(default = "default_continue_budget")]
    pub continue_budget: u8,
    #[serde(default)]
    pub rng_seed: Option<u64>,
    #[serde(default)]
    pub clock: u32,
    /// Hex-specific runtime blob (WorldState for tavern).
    #[serde(default)]
    pub runtime: serde_json::Value,
}

fn default_continue_budget() -> u8 {
    1
}

impl Default for CampaignState {
    fn default() -> Self {
        Self {
            current_scene: "outside".into(),
            current_actor: "pc".into(),
            continue_budget: 1,
            rng_seed: None,
            clock: 0,
            runtime: serde_json::Value::Object(Default::default()),
        }
    }
}

/// On-disk campaign tree — source of truth.
#[derive(Debug, Clone)]
pub struct Campaign {
    pub root: PathBuf,
    pub config: HexConfig,
    pub world_md: String,
    pub goals: GoalsDoc,
    pub scenes: HashMap<String, SceneDef>,
    pub actors: HashMap<String, ActorSheet>,
    pub state: CampaignState,
    pub transcript: TranscriptLog,
    pub dirty: bool,
}

impl Campaign {
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        info!(path = %root.display(), "loading campaign");

        let config = if root.join("hex.toml").exists() {
            let raw = fs::read_to_string(root.join("hex.toml"))?;
            toml::from_str(&raw)?
        } else {
            HexConfig::default()
        };

        let world_md = fs::read_to_string(root.join("world.md")).unwrap_or_default();

        let goals = if root.join("goals.json").exists() {
            serde_json::from_str(&fs::read_to_string(root.join("goals.json"))?)?
        } else {
            GoalsDoc { goals: vec![] }
        };

        let mut scenes = HashMap::new();
        let scenes_dir = root.join("scenes");
        if scenes_dir.is_dir() {
            for entry in fs::read_dir(&scenes_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    let scene: SceneDef = serde_json::from_str(&fs::read_to_string(&path)?)?;
                    scenes.insert(scene.id.clone(), scene);
                }
            }
        }

        let mut actors = HashMap::new();
        let actors_dir = root.join("actors");
        if actors_dir.is_dir() {
            for entry in fs::read_dir(&actors_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    let actor: ActorSheet = serde_json::from_str(&fs::read_to_string(&path)?)?;
                    actors.insert(actor.id.clone(), actor);
                }
            }
        }

        let state = if root.join("state.json").exists() {
            serde_json::from_str(&fs::read_to_string(root.join("state.json"))?)?
        } else {
            CampaignState::default()
        };

        let transcript = if root.join("transcript.jsonl").exists() {
            load_transcript_jsonl(&root.join("transcript.jsonl"))?
        } else {
            TranscriptLog::default()
        };

        Ok(Self {
            root,
            config,
            world_md,
            goals,
            scenes,
            actors,
            state,
            transcript,
            dirty: false,
        })
    }

    pub fn save(&mut self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(self.root.join("scenes"))?;
        fs::create_dir_all(self.root.join("actors"))?;
        fs::create_dir_all(self.root.join("locks"))?;
        fs::create_dir_all(self.root.join("snapshots"))?;

        let config_toml = toml::to_string_pretty(&self.config).map_err(|e| {
            StoreError::Json(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )))
        })?;
        fs::write(self.root.join("hex.toml"), config_toml)?;

        fs::write(self.root.join("world.md"), &self.world_md)?;
        fs::write(
            self.root.join("goals.json"),
            serde_json::to_string_pretty(&self.goals)?,
        )?;

        for scene in self.scenes.values() {
            let path = self.root.join("scenes").join(format!("{}.json", scene.id));
            fs::write(path, serde_json::to_string_pretty(scene)?)?;
        }

        for actor in self.actors.values() {
            let path = self.root.join("actors").join(format!("{}.json", actor.id));
            fs::write(path, serde_json::to_string_pretty(actor)?)?;
        }

        let state_tmp = self.root.join("state.json.tmp");
        fs::write(&state_tmp, serde_json::to_string_pretty(&self.state)?)?;
        fs::rename(state_tmp, self.root.join("state.json"))?;

        save_transcript_jsonl(&self.root.join("transcript.jsonl"), &self.transcript)?;

        // Last commit snapshot for undo (Stage 1 minimal).
        let snap = self.root.join("snapshots").join("last.json");
        fs::write(&snap, serde_json::to_string_pretty(&self.state)?)?;

        self.dirty = false;
        debug!(path = %self.root.display(), "campaign saved");
        Ok(())
    }

    pub fn apply_patches(&mut self, patches: &[StorePatch], writable: &[String]) -> Result<()> {
        for patch in patches {
            if !writable.iter().any(|p| path_matches_pattern(&patch.path, p)) {
                return Err(StoreError::PathNotWritable {
                    path: patch.path.clone(),
                });
            }
            apply_one_patch(self, patch)?;
        }
        self.dirty = true;
        Ok(())
    }

    pub fn append_transcript(&mut self, beat: TranscriptBeat) {
        self.transcript.append(beat);
        self.dirty = true;
    }

    /// Wipe progress: default scene/actor, empty runtime, empty transcript.
    /// World docs and hex.toml are kept. PC name + chargen reset so a new game re-prompts.
    pub fn reset_progress(&mut self) {
        self.state = CampaignState::default();
        self.transcript = TranscriptLog::default();
        if let Some(pc) = self.actors.get_mut("pc") {
            pc.name = "You".into();
            // Clear 5e sheet so character creator runs again.
            if let Some(obj) = pc.stats.as_object_mut() {
                for key in [
                    "str",
                    "dex",
                    "con",
                    "int",
                    "wis",
                    "cha",
                    "skills",
                    "preset",
                    "proficiency_bonus",
                    "chargen_done",
                    "might",
                    "nerve",
                    "wit",
                ] {
                    obj.remove(key);
                }
            }
        }
        self.dirty = true;
        info!(path = %self.root.display(), "campaign progress reset");
    }

    pub fn writable_paths(&self) -> Vec<String> {
        self.config
            .grants
            .iter()
            .find(|g| g.capability == "store.patch")
            .map(|g| g.writable_paths.clone())
            .unwrap_or_default()
    }

    pub fn controllable_actors(&self) -> Vec<&ActorSheet> {
        self.actors
            .values()
            .filter(|a| a.player_controllable || a.id == "pc")
            .collect()
    }

    pub fn next_actor_id(&self) -> Option<String> {
        let mut ids: Vec<_> = self
            .controllable_actors()
            .into_iter()
            .map(|a| a.id.clone())
            .collect();
        ids.sort();
        if ids.is_empty() {
            return None;
        }
        let cur = &self.state.current_actor;
        if let Some(pos) = ids.iter().position(|id| id == cur) {
            Some(ids[(pos + 1) % ids.len()].clone())
        } else {
            Some(ids[0].clone())
        }
    }
}

fn apply_one_patch(campaign: &mut Campaign, patch: &StorePatch) -> Result<()> {
    let parts: Vec<&str> = patch.path.split('.').collect();
    match parts.as_slice() {
        ["state", field] => set_state_field(&mut campaign.state, field, &patch.value),
        ["runtime", rest @ ..] => set_runtime_field(&mut campaign.state.runtime, rest, &patch.value),
        ["actors", id, field] => {
            if let Some(actor) = campaign.actors.get_mut(*id) {
                set_actor_field(actor, field, &patch.value);
            }
        }
        ["scenes", id, "flags", flag] => {
            if let Some(scene) = campaign.scenes.get_mut(*id) {
                if let Some(obj) = scene.flags.as_object_mut() {
                    obj.insert(flag.to_string(), patch.value.clone());
                }
            }
        }
        _ => {
            debug!(path = %patch.path, "patch applied to runtime fallback");
            set_runtime_field(
                &mut campaign.state.runtime,
                &[patch.path.as_str()],
                &patch.value,
            );
        }
    }
    Ok(())
}

fn set_state_field(state: &mut CampaignState, field: &str, value: &serde_json::Value) {
    match field {
        "current_scene" => {
            if let Some(s) = value.as_str() {
                state.current_scene = s.into();
            }
        }
        "current_actor" => {
            if let Some(s) = value.as_str() {
                state.current_actor = s.into();
            }
        }
        "continue_budget" => {
            if let Some(n) = value.as_u64() {
                state.continue_budget = n as u8;
            }
        }
        "clock" => {
            if let Some(n) = value.as_u64() {
                state.clock = n as u32;
            }
        }
        "rng_seed" => state.rng_seed = value.as_u64(),
        _ => {}
    }
}

fn set_runtime_field(runtime: &mut serde_json::Value, path: &[&str], value: &serde_json::Value) {
    if path.is_empty() {
        *runtime = value.clone();
        return;
    }
    if !runtime.is_object() {
        *runtime = serde_json::json!({});
    }
    let mut cur = runtime;
    for (i, key) in path.iter().enumerate() {
        if i == path.len() - 1 {
            if let Some(map) = cur.as_object_mut() {
                map.insert(key.to_string(), value.clone());
            }
        } else {
            if !cur.get(*key).map(|v| v.is_object()).unwrap_or(false) {
                if let Some(map) = cur.as_object_mut() {
                    map.insert(key.to_string(), serde_json::json!({}));
                }
            }
            cur = cur.get_mut(*key).unwrap();
        }
    }
}

fn set_actor_field(actor: &mut ActorSheet, field: &str, value: &serde_json::Value) {
    match field {
        "inventory" => {
            if let Some(arr) = value.as_array() {
                actor.inventory = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
            }
        }
        "flags" => {
            if let Some(arr) = value.as_array() {
                actor.flags = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
            }
        }
        "stats" => actor.stats = value.clone(),
        _ => {}
    }
}

fn load_transcript_jsonl(path: &Path) -> Result<TranscriptLog> {
    let raw = fs::read_to_string(path)?;
    let mut beats = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        beats.push(serde_json::from_str(line)?);
    }
    Ok(TranscriptLog { beats })
}

fn save_transcript_jsonl(path: &Path, log: &TranscriptLog) -> Result<()> {
    let mut out = String::new();
    for beat in &log.beats {
        out.push_str(&serde_json::to_string(beat)?);
        out.push('\n');
    }
    fs::write(path, out)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::StorePatch;

    #[test]
    fn round_trip_state() {
        let dir = std::env::temp_dir().join(format!("hexbee-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut campaign = Campaign {
            root: dir.clone(),
            config: HexConfig::default(),
            world_md: "# Test".into(),
            goals: GoalsDoc { goals: vec![] },
            scenes: HashMap::new(),
            actors: HashMap::new(),
            state: CampaignState {
                current_scene: "outside".into(),
                current_actor: "pc".into(),
                continue_budget: 1,
                rng_seed: Some(42),
                clock: 0,
                runtime: serde_json::json!({"hp": 10}),
            },
            transcript: TranscriptLog::default(),
            dirty: true,
        };

        campaign.save().unwrap();
        let loaded = Campaign::load(&dir).unwrap();
        assert_eq!(loaded.state.current_scene, "outside");
        assert_eq!(loaded.state.rng_seed, Some(42));
        assert_eq!(loaded.state.runtime["hp"], 10);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_patch_state() {
        let mut campaign = Campaign {
            root: PathBuf::from("/tmp/unused"),
            config: HexConfig::default(),
            world_md: String::new(),
            goals: GoalsDoc { goals: vec![] },
            scenes: HashMap::new(),
            actors: HashMap::new(),
            state: CampaignState::default(),
            transcript: TranscriptLog::default(),
            dirty: false,
        };
        let writable = campaign.writable_paths();
        campaign
            .apply_patches(
                &[StorePatch::set_str("state.current_scene", "main_hall")],
                &writable,
            )
            .unwrap();
        assert_eq!(campaign.state.current_scene, "main_hall");
    }

    #[test]
    fn reset_progress_clears_runtime() {
        let mut actors = HashMap::new();
        actors.insert(
            "pc".into(),
            crate::ActorSheet {
                id: "pc".into(),
                name: "Mira".into(),
                player_controllable: true,
                voice: String::new(),
                traits: vec![],
                stats: serde_json::json!({}),
                inventory: vec![],
                flags: vec![],
                known_facts: vec![],
                relationships: serde_json::json!({}),
                temperament: String::new(),
                description: String::new(),
                weapon: String::new(),
                likes: vec![],
                taboos: vec![],
            },
        );
        let mut campaign = Campaign {
            root: PathBuf::from("/tmp/unused"),
            config: HexConfig::default(),
            world_md: String::new(),
            goals: GoalsDoc { goals: vec![] },
            scenes: HashMap::new(),
            actors,
            state: CampaignState {
                current_scene: "main_hall".into(),
                current_actor: "sally".into(),
                continue_budget: 1,
                rng_seed: Some(1),
                clock: 9,
                runtime: serde_json::json!({"player":{"hp":0}}),
            },
            transcript: TranscriptLog {
                beats: vec![TranscriptBeat {
                    actor_id: "pc".into(),
                    intent_kind: "action".into(),
                    raw: "look".into(),
                    narration: None,
                    organ_trace: vec![],
                }],
            },
            dirty: false,
        };
        campaign.reset_progress();
        assert_eq!(campaign.state.current_scene, "outside");
        assert_eq!(campaign.state.current_actor, "pc");
        assert!(campaign.transcript.beats.is_empty());
        assert_eq!(
            campaign.actors.get("pc").map(|a| a.name.as_str()),
            Some("You")
        );
        assert!(
            campaign
                .state
                .runtime
                .as_object()
                .map(|o| o.is_empty())
                .unwrap_or(true)
        );
        assert!(campaign.dirty);
    }

    #[test]
    fn deny_forbidden_patch() {
        let mut campaign = Campaign {
            root: PathBuf::from("/tmp/unused"),
            config: HexConfig::default(),
            world_md: String::new(),
            goals: GoalsDoc { goals: vec![] },
            scenes: HashMap::new(),
            actors: HashMap::new(),
            state: CampaignState::default(),
            transcript: TranscriptLog::default(),
            dirty: false,
        };
        let writable = campaign.writable_paths();
        let err = campaign
            .apply_patches(
                &[StorePatch::set_i64("actors.pc.stats.might", 99)],
                &writable,
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::PathNotWritable { .. }));
    }
}
