//! **Situation** — what has changed in play (attitude, heat, time, declared flags).
//!
//! Expands constantly during play, but only along knobs the hex declared.
//! Organs and generic moves must use [`Situation::apply`], not poke world fields.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Declared knobs the tavern hex allows to move at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SituationKnob {
    Attitude { npc: String, delta: i32 },
    SceneHeatDelta(i32),
    ClockBeat,
    NpcFlag { npc: String, flag: SituationFlag },
    CheckFail { secret_id: String },
}

/// Runtime flags situation may set. Canon sheet flags (e.g. `sworn_silent`) are not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SituationFlag {
    Alert,
    AllowsPresence,
    AskedLeave,
}

impl SituationFlag {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Alert => "alert",
            Self::AllowsPresence => "allows_presence",
            Self::AskedLeave => "asked_leave",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "alert" => Some(Self::Alert),
            "allows_presence" => Some(Self::AllowsPresence),
            "asked_leave" => Some(Self::AskedLeave),
            _ => None,
        }
    }
}

/// Play-state that may change every turn along declared knobs only.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Situation {
    #[serde(default)]
    pub attitude: HashMap<String, i32>,
    #[serde(default)]
    pub npc_flags: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub check_fail_counts: HashMap<String, u32>,
    /// 0 quiet … 3 rowdy
    #[serde(default = "default_heat")]
    pub scene_heat: i32,
    #[serde(default)]
    pub clock_beats: u32,
}

fn default_heat() -> i32 {
    1
}

impl Situation {
    pub fn attitude_toward(&self, npc: &str) -> i32 {
        *self.attitude.get(npc).unwrap_or(&0)
    }

    pub fn npc_flag(&self, npc: &str, flag: &str) -> bool {
        self.npc_flags
            .get(npc)
            .is_some_and(|v| v.iter().any(|f| f == flag))
    }

    pub fn check_fail_count(&self, secret_id: &str) -> u32 {
        *self.check_fail_counts.get(secret_id).unwrap_or(&0)
    }

    /// Apply one declared knob. Returns false if the knob is not allowed.
    pub fn apply(&mut self, knob: SituationKnob) -> bool {
        match knob {
            SituationKnob::Attitude { npc, delta } => {
                let e = self.attitude.entry(npc).or_insert(0);
                *e = (*e + delta).clamp(-2, 2);
                true
            }
            SituationKnob::SceneHeatDelta(delta) => {
                self.scene_heat = (self.scene_heat + delta).clamp(0, 3);
                true
            }
            SituationKnob::ClockBeat => {
                self.clock_beats = self.clock_beats.saturating_add(1);
                true
            }
            SituationKnob::NpcFlag { npc, flag } => {
                let v = self.npc_flags.entry(npc).or_default();
                let s = flag.as_str();
                if !v.iter().any(|f| f == s) {
                    v.push(s.into());
                }
                true
            }
            SituationKnob::CheckFail { secret_id } => {
                *self.check_fail_counts.entry(secret_id).or_insert(0) += 1;
                true
            }
        }
    }

    /// Load a canon sheet flag into situation storage for lock checks (read-only at runtime).
    pub fn seed_canon_npc_flag(&mut self, npc: &str, flag: &str) {
        let v = self.npc_flags.entry(npc.to_string()).or_default();
        if !v.iter().any(|f| f == flag) {
            v.push(flag.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_declared_knobs_move_situation() {
        let mut s = Situation::default();
        assert!(s.apply(SituationKnob::Attitude {
            npc: "groll".into(),
            delta: 1,
        }));
        assert_eq!(s.attitude_toward("groll"), 1);
        assert!(s.apply(SituationKnob::NpcFlag {
            npc: "groll".into(),
            flag: SituationFlag::Alert,
        }));
        assert!(s.npc_flag("groll", "alert"));
    }
}
