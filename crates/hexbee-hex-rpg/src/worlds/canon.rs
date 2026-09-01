//! **Canon** — what is true (sheets, secrets, inventory, goals, quest flags).
//!
//! May change only through authored apply paths (checks, acts, store patches).
//! Models and presentation organs must not call [`apply_canon`].

use crate::checks::CheckKind;
use crate::state::WorldState;

/// Authored world mutations. No runtime minting of secrets, items, or factions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonPatch {
    GrantDoorPassword,
    CharmSally,
    DewrBackedDown,
    GrantAmulet,
    ClearAura,
    DewrScamCleared,
    MetDewr,
    SpendGold(i32),
    Heal(i32),
    SetLocation(crate::state::Location),
}

/// Apply a single canon patch. Returns false if the patch is redundant or illegal.
pub fn apply_canon(world: &mut WorldState, patch: CanonPatch) -> bool {
    match patch {
        CanonPatch::GrantDoorPassword => {
            if world.player.has_password {
                return false;
            }
            world.player.has_password = true;
            true
        }
        CanonPatch::CharmSally => {
            world.player.sally_charmed = true;
            world.player.sally_tip_discount = false;
            true
        }
        CanonPatch::DewrBackedDown => {
            world.player.dewr_backed_down = true;
            true
        }
        CanonPatch::GrantAmulet => {
            world.player.has_amulet = true;
            true
        }
        CanonPatch::ClearAura => {
            world.player.aura_cleared = true;
            true
        }
        CanonPatch::DewrScamCleared => {
            world.player.dewr_scam_cleared = true;
            true
        }
        CanonPatch::MetDewr => {
            world.player.met_dewr = true;
            world.first_talk_pending = false;
            true
        }
        CanonPatch::SpendGold(amount) => {
            if world.player.gold < amount {
                return false;
            }
            world.player.gold -= amount;
            true
        }
        CanonPatch::Heal(amount) => {
            world.player.hp = (world.player.hp + amount).min(world.player.max_hp);
            true
        }
        CanonPatch::SetLocation(loc) => {
            world.location = loc;
            true
        }
    }
}

/// Success effects for authored social checks — the only check path that may touch canon.
pub fn apply_check_success(world: &mut WorldState, kind: CheckKind) {
    match kind {
        CheckKind::PersuadeGrollPassword => {
            apply_canon(world, CanonPatch::GrantDoorPassword);
        }
        CheckKind::CharmSally => {
            apply_canon(world, CanonPatch::CharmSally);
        }
        CheckKind::ChallengeDewr | CheckKind::SeeThroughScam => {
            apply_canon(world, CanonPatch::DewrBackedDown);
        }
        CheckKind::ImpressDremis => {
            apply_canon(world, CanonPatch::GrantAmulet);
        }
        CheckKind::SneakPastDewr => {}
    }
}

/// Situation-only bookkeeping on failed authored checks (not canon).
pub fn note_authored_check_fail(world: &mut WorldState, secret_id: &str) {
    world
        .situation
        .apply(crate::worlds::situation::SituationKnob::CheckFail {
            secret_id: secret_id.to_string(),
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canon_does_not_double_grant_password() {
        let mut w = WorldState::default();
        assert!(apply_canon(&mut w, CanonPatch::GrantDoorPassword));
        assert!(!apply_canon(&mut w, CanonPatch::GrantDoorPassword));
        assert!(w.player.has_password);
    }
}
