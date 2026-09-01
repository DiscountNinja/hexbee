//! Contested checks — tavern hex uses d20 + ability + skill prof vs DC.
//!
//! Kernel owns [`hexbee_kernel::CheckRequest`] / [`hexbee_kernel::CheckOutcome`];
//! this module maps quest beats onto SRD skills and difficulties.

use hexbee_kernel::CheckOutcome;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::skills::{Ability, CharacterBuild, Skill};

/// Contested social / world checks (story beats → skill + DC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckKind {
    PersuadeGrollPassword,
    CharmSally,
    ImpressDremis,
    ChallengeDewr,
    /// Call out Dewr's door scam / refuse to pay.
    SeeThroughScam,
    /// Slip past Dewr at the door without paying (Stealth).
    SneakPastDewr,
}

impl CheckKind {
    pub fn label(self) -> &'static str {
        match self {
            CheckKind::PersuadeGrollPassword => "persuade Groll for the password",
            CheckKind::CharmSally => "charm Sally",
            CheckKind::ImpressDremis => "impress Dremis",
            CheckKind::ChallengeDewr => "challenge Dewr",
            CheckKind::SeeThroughScam => "see through Dewr's scam",
            CheckKind::SneakPastDewr => "sneak past Dewr",
        }
    }

    pub fn skill(self) -> Skill {
        match self {
            CheckKind::PersuadeGrollPassword => Skill::Persuasion,
            CheckKind::CharmSally => Skill::Persuasion,
            CheckKind::ImpressDremis => Skill::Persuasion,
            CheckKind::ChallengeDewr => Skill::Intimidation,
            CheckKind::SeeThroughScam => Skill::Insight,
            CheckKind::SneakPastDewr => Skill::Stealth,
        }
    }

    pub fn ability(self) -> Ability {
        self.skill().ability()
    }
}

/// Base DC for each check (before tip discounts).
pub fn dc_for(kind: CheckKind, sally_tip_discount: bool) -> i32 {
    match kind {
        CheckKind::PersuadeGrollPassword => 12,
        CheckKind::CharmSally => {
            if sally_tip_discount {
                10
            } else {
                14
            }
        }
        CheckKind::ImpressDremis => 16,
        CheckKind::ChallengeDewr => 10,
        CheckKind::SeeThroughScam => 12,
        CheckKind::SneakPastDewr => 14,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub kind: CheckKind,
    pub skill: Skill,
    pub ability: Ability,
    pub ability_mod: i32,
    pub proficient: bool,
    pub proficiency_bonus: i32,
    pub modifier: i32,
    pub dc: i32,
    pub roll: i32,
    pub total: i32,
    pub success: bool,
}

impl CheckResult {
    pub fn to_outcome(&self) -> CheckOutcome {
        CheckOutcome::from_d20(self.roll, self.modifier, self.dc, self.kind.label())
    }
}

/// Roll d20 + ability_mod + (prof if skilled) vs DC.
pub fn resolve_d20_check<R: Rng>(
    kind: CheckKind,
    build: &CharacterBuild,
    dc: i32,
    rng: &mut R,
) -> CheckResult {
    let skill = kind.skill();
    let ability = skill.ability();
    let ability_mod = build.abilities.modifier(ability);
    let proficient = build.is_proficient(skill);
    let proficiency_bonus = if proficient {
        build.proficiency_bonus
    } else {
        0
    };
    let modifier = ability_mod + proficiency_bonus;
    let roll = rng.gen_range(1..=20);
    let total = roll + modifier;
    CheckResult {
        kind,
        skill,
        ability,
        ability_mod,
        proficient,
        proficiency_bonus,
        modifier,
        dc,
        roll,
        total,
        success: total >= dc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::{BuildPreset, CharacterBuild};
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn sally_tip_lowers_dc() {
        assert_eq!(dc_for(CheckKind::CharmSally, false), 14);
        assert_eq!(dc_for(CheckKind::CharmSally, true), 10);
    }

    #[test]
    fn challenge_uses_intimidation() {
        assert_eq!(CheckKind::ChallengeDewr.skill(), Skill::Intimidation);
    }

    #[test]
    fn face_usually_beats_challenge_dc() {
        let mut rng = StdRng::seed_from_u64(42);
        // Bruiser: Intimidation proficient, CHA 10 → +0+2 = +2 vs DC 10
        let build = CharacterBuild::from_preset(BuildPreset::Bruiser);
        let mut successes = 0;
        for _ in 0..40 {
            let r = resolve_d20_check(CheckKind::ChallengeDewr, &build, 10, &mut rng);
            if r.success {
                successes += 1;
            }
        }
        assert!(successes >= 20, "expected most successes, got {successes}");
    }

    #[test]
    fn outcome_report_mentions_d20() {
        let o = CheckOutcome::from_d20(15, 3, 12, "test");
        assert!(o.success);
        assert!(o.report.contains("d20"));
    }
}
