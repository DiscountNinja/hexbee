//! Off-book generic social moves — situation knobs only; never canon.

use crate::scene::TalkFocus;
use crate::worlds::{SituationFlag, SituationKnob};
use crate::state::WorldState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericOutcome {
    Warmer,
    Colder,
    Alert,
    Quieter,
    Louder,
    TimePasses,
    MayStay,
    AskedToLeave,
}

impl GenericOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Warmer => "they warm to you a little",
            Self::Colder => "they cool toward you",
            Self::Alert => "they grow watchful",
            Self::Quieter => "the room quiets",
            Self::Louder => "heads turn — the room grows louder",
            Self::TimePasses => "a beat passes",
            Self::MayStay => "they let you stay for now",
            Self::AskedToLeave => "they ask you to leave",
        }
    }

    pub fn npc_fallback(self, success: bool) -> String {
        match (self, success) {
            (Self::Warmer, true) => "*a grudging smile* \"Fine — you've got a way with words.\"".into(),
            (Self::Colder, false) => "*eyes harden* \"Watch yourself.\"".into(),
            (Self::Alert, false) => "*hand drifts toward a weapon* \"I don't like your tone.\"".into(),
            (Self::MayStay, true) => "*shrugs* \"Stay. For now.\"".into(),
            (Self::AskedToLeave, false) => "*jerks chin at the door* \"Out.\"".into(),
            _ => "*considers you* \"Hmm.\"".into(),
        }
    }

    pub fn situation_knobs(self, npc: &str) -> Vec<SituationKnob> {
        match self {
            Self::Warmer => vec![SituationKnob::Attitude {
                npc: npc.into(),
                delta: 1,
            }],
            Self::Colder => vec![SituationKnob::Attitude {
                npc: npc.into(),
                delta: -1,
            }],
            Self::Alert => vec![SituationKnob::NpcFlag {
                npc: npc.into(),
                flag: SituationFlag::Alert,
            }],
            Self::Quieter => vec![SituationKnob::SceneHeatDelta(-1)],
            Self::Louder => vec![SituationKnob::SceneHeatDelta(1)],
            Self::TimePasses => vec![SituationKnob::ClockBeat],
            Self::MayStay => vec![SituationKnob::NpcFlag {
                npc: npc.into(),
                flag: SituationFlag::AllowsPresence,
            }],
            Self::AskedToLeave => vec![SituationKnob::NpcFlag {
                npc: npc.into(),
                flag: SituationFlag::AskedLeave,
            }],
        }
    }
}

/// Pick an allowed off-book outcome from the bag (deterministic from roll + focus).
pub fn pick_generic_outcome(success: bool, focus: &TalkFocus, roll: i32) -> GenericOutcome {
    let bucket = (roll as usize + attitude_bucket(focus)) % 4;
    if success {
        match bucket {
            0 => GenericOutcome::Warmer,
            1 => GenericOutcome::MayStay,
            2 => GenericOutcome::Quieter,
            _ => GenericOutcome::TimePasses,
        }
    } else {
        match bucket {
            0 => GenericOutcome::Colder,
            1 => GenericOutcome::Alert,
            2 => GenericOutcome::Louder,
            _ => GenericOutcome::AskedToLeave,
        }
    }
}

fn attitude_bucket(focus: &TalkFocus) -> usize {
    match focus {
        TalkFocus::Quest(_) => 1,
        TalkFocus::Ambient(_) => 2,
    }
}

/// Apply only declared situation knobs. Never mint canon.
pub fn apply_generic_outcome(world: &mut WorldState, focus: &TalkFocus, outcome: GenericOutcome) {
    let npc = crate::secrets::focus_holder_id(focus);
    for knob in outcome.situation_knobs(npc) {
        world.situation.apply(knob);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::NpcId;

    #[test]
    fn generic_move_only_touches_situation() {
        let mut world = WorldState::default();
        let before_password = world.player.has_password;
        let focus = TalkFocus::Quest(NpcId::Groll);
        apply_generic_outcome(&mut world, &focus, GenericOutcome::Warmer);
        assert_eq!(world.attitude_toward("groll"), 1);
        assert_eq!(world.player.has_password, before_password);
    }
}
