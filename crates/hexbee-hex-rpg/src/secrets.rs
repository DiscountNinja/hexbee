//! Authored secrets — base DC on the bible, modifiers from store only, locks block rolls.
//!
//! Binding maps player lines onto named secrets/props/people. No runtime secret minting.

use serde::Deserialize;

use crate::checks::CheckKind;
use crate::scene::TalkFocus;
use crate::state::{NpcId, WorldState};

#[derive(Debug, Clone, Deserialize)]
pub struct SecretsDoc {
    pub generic: GenericMoveDef,
    pub secrets: Vec<SecretDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenericMoveDef {
    pub label: String,
    pub skill: String,
    pub base_dc: i32,
    #[serde(default)]
    pub modifiers: Vec<DcModifier>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecretDef {
    pub id: String,
    pub label: String,
    pub holder: String,
    pub check: String,
    pub skill: String,
    pub base_dc: i32,
    #[serde(default)]
    pub bind_any: Vec<String>,
    #[serde(default)]
    pub modifiers: Vec<DcModifier>,
    #[serde(default)]
    pub locks: Vec<SecretLock>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DcModifier {
    pub when: String,
    pub delta: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecretLock {
    pub when: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TalkBind {
    /// Pure banter — Actor only, no roll.
    Banter,
    /// Player is pushing socially but nothing authored binds — generic move bag.
    OffBook,
    /// A named secret / move from the catalog.
    Tagged(String),
}

#[derive(Debug, Clone)]
pub enum BindBlock {
    /// Lock fired — no roll; show message (flavor allowed).
    Locked(String),
}

#[derive(Debug, Clone)]
pub struct SecretsCatalog {
    doc: SecretsDoc,
}

impl SecretsCatalog {
    pub fn bundled() -> Self {
        let raw = include_str!("../campaign/secrets.yaml");
        let doc: SecretsDoc = serde_yaml::from_str(raw).expect("bundled secrets.yaml");
        Self { doc }
    }

    /// Campaign file on disk, so a hand edit applies without a rebuild.
    pub fn load_path(path: &std::path::Path) -> Option<Self> {
        let raw = std::fs::read_to_string(path).ok()?;
        let doc: SecretsDoc = serde_yaml::from_str(&raw).ok()?;
        Some(Self { doc })
    }

    pub fn secret(&self, id: &str) -> Option<&SecretDef> {
        self.doc.secrets.iter().find(|s| s.id == id)
    }

    pub fn generic(&self) -> &GenericMoveDef {
        &self.doc.generic
    }

    /// Map player line + focus onto banter, off-book, or a tagged secret.
    pub fn bind_talk(
        &self,
        input: &str,
        focus: &TalkFocus,
        world: &WorldState,
    ) -> Result<TalkBind, BindBlock> {
        let lower = crate::slang::normalize(input);
        if crate::social::is_pure_banter(input) {
            return Ok(TalkBind::Banter);
        }

        let holder = focus_holder_id(focus);
        for secret in &self.doc.secrets {
            if secret.holder != holder {
                continue;
            }
            if !secret.bind_any.iter().any(|k| lower.contains(k)) {
                continue;
            }
            if let Some(msg) = self.evaluate_locks(&secret.locks, world, focus) {
                return Err(BindBlock::Locked(msg));
            }
            if secret_revealed(secret, world) {
                continue;
            }
            return Ok(TalkBind::Tagged(secret.id.clone()));
        }

        if crate::social::is_off_book_social(&lower) {
            return Ok(TalkBind::OffBook);
        }
        if crate::social::wants_social_outcome(&lower) {
            return Ok(TalkBind::OffBook);
        }
        Ok(TalkBind::Banter)
    }

    pub fn resolve_dc(
        &self,
        secret_id: &str,
        world: &WorldState,
        focus: &TalkFocus,
    ) -> Option<i32> {
        let secret = self.secret(secret_id)?;
        let mut dc = secret.base_dc;
        for m in &secret.modifiers {
            dc += self.eval_modifier_delta(&m.when, m.delta, world, focus, Some(secret_id));
        }
        Some(dc)
    }

    pub fn resolve_generic_dc(&self, world: &WorldState, focus: &TalkFocus) -> i32 {
        let g = &self.doc.generic;
        let mut dc = g.base_dc;
        for m in &g.modifiers {
            dc += self.eval_modifier_delta(&m.when, m.delta, world, focus, None);
        }
        dc
    }

    pub fn check_to_kind(check: &str) -> Option<CheckKind> {
        match check {
            "persuade_groll" => Some(CheckKind::PersuadeGrollPassword),
            "charm_sally" => Some(CheckKind::CharmSally),
            "challenge_dewr" => Some(CheckKind::ChallengeDewr),
            "call_out_scam" => Some(CheckKind::SeeThroughScam),
            "impress_dremis" => Some(CheckKind::ImpressDremis),
            _ => None,
        }
    }

    fn evaluate_locks(
        &self,
        locks: &[SecretLock],
        world: &WorldState,
        focus: &TalkFocus,
    ) -> Option<String> {
        for lock in locks {
            if self.eval_condition(&lock.when, world, focus, None) {
                return Some(lock.message.clone());
            }
        }
        None
    }

    fn eval_modifier_delta(
        &self,
        when: &str,
        delta: i32,
        world: &WorldState,
        focus: &TalkFocus,
        secret_id: Option<&str>,
    ) -> i32 {
        if self.eval_condition(when, world, focus, secret_id) {
            delta
        } else {
            0
        }
    }

    /// Store-only predicates. No LLM, no invented flags.
    fn eval_condition(
        &self,
        when: &str,
        world: &WorldState,
        focus: &TalkFocus,
        secret_id: Option<&str>,
    ) -> bool {
        let when = when.trim();
        if let Some(rest) = when.strip_prefix('!') {
            return !self.eval_condition(rest, world, focus, secret_id);
        }
        if let Some((lhs, rhs)) = when.split_once(">=") {
            return self.eval_numeric(lhs, world, focus, secret_id) >= rhs.trim().parse().unwrap_or(0);
        }
        if let Some((lhs, rhs)) = when.split_once('<') {
            return self.eval_numeric(lhs, world, focus, secret_id) < rhs.trim().parse().unwrap_or(0);
        }
        if let Some((lhs, rhs)) = when.split_once('>') {
            return self.eval_numeric(lhs, world, focus, secret_id) > rhs.trim().parse().unwrap_or(0);
        }
        self.eval_bool(when, world, focus, secret_id)
    }

    fn eval_bool(&self, path: &str, world: &WorldState, focus: &TalkFocus, secret_id: Option<&str>) -> bool {
        match path {
            "player.has_password" => world.player.has_password,
            "player.has_amulet" => world.player.has_amulet,
            "player.sally_tip_discount" => world.player.sally_tip_discount,
            "player.sally_charmed" => world.player.sally_charmed,
            "player.dewr_backed_down" => world.player.dewr_backed_down,
            "player.dewr_scam_cleared" => world.player.dewr_scam_cleared,
            "player.aura_cleared" => world.player.aura_cleared,
            "dewr.alive" => world.dewr.alive,
            "!player.aura_cleared" => !world.player.aura_cleared,
            p if p.starts_with("actor.") => {
                let rest = p.strip_prefix("actor.").unwrap_or(p);
                if let Some((npc, flag)) = rest.split_once('.') {
                    world.npc_flag(npc, flag)
                } else {
                    false
                }
            }
            _ => {
                if let Some(sid) = secret_id {
                    if let Some(rest) = path.strip_prefix("checks_failed.") {
                        if rest == sid {
                            return world.check_fail_count(sid) >= 1;
                        }
                    }
                }
                if path == "attitude.focus < 0" {
                    return world.attitude_toward(focus_holder_id(focus)) < 0;
                }
                if path == "attitude.focus > 0" {
                    return world.attitude_toward(focus_holder_id(focus)) > 0;
                }
                if path == "scene_heat >= 2" {
                    return world.situation.scene_heat >= 2;
                }
                false
            }
        }
    }

    fn eval_numeric(
        &self,
        path: &str,
        world: &WorldState,
        focus: &TalkFocus,
        secret_id: Option<&str>,
    ) -> i32 {
        let path = path.trim();
        if path == "scene_heat" {
            return world.situation.scene_heat;
        }
        if path == "attitude.focus" {
            return world.attitude_toward(focus_holder_id(focus));
        }
        if let Some(sid) = secret_id {
            if let Some(rest) = path.strip_prefix("checks_failed.") {
                if rest == sid {
                    return world.check_fail_count(sid) as i32;
                }
            }
        }
        0
    }
}

pub fn focus_holder_id(focus: &TalkFocus) -> &'static str {
    match focus {
        TalkFocus::Quest(NpcId::Groll) => "groll",
        TalkFocus::Quest(NpcId::Sally) => "sally",
        TalkFocus::Quest(NpcId::Dewr) => "dewr",
        TalkFocus::Quest(NpcId::Dremis) => "dremis",
        TalkFocus::Quest(NpcId::Goblin) => "dewr",
        TalkFocus::Quest(NpcId::Patron) => "patron",
        TalkFocus::Ambient(id) => {
            if id.contains("carter") {
                "carter"
            } else {
                "ambient"
            }
        }
    }
}

fn secret_revealed(secret: &SecretDef, world: &WorldState) -> bool {
    match secret.check.as_str() {
        "persuade_groll" => world.player.has_password,
        "charm_sally" => world.player.sally_charmed,
        "impress_dremis" => world.player.has_amulet,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::TalkFocus;

    #[test]
    fn bind_cellar_question_to_password_secret() {
        let cat = SecretsCatalog::bundled();
        let world = WorldState::default();
        let focus = TalkFocus::Quest(NpcId::Groll);
        let line = "Why is the cellar locked?";
        let bind = cat.bind_talk(line, &focus, &world).unwrap();
        assert!(matches!(bind, TalkBind::Tagged(id) if id == "groll_storage_password"));
    }

    #[test]
    fn sworn_silent_is_lock_not_modifier() {
        let cat = SecretsCatalog::bundled();
        let mut world = WorldState::default();
        world.set_npc_flag("groll", "sworn_silent");
        let focus = TalkFocus::Quest(NpcId::Groll);
        let err = cat
            .bind_talk("tell me the password", &focus, &world)
            .unwrap_err();
        assert!(matches!(err, BindBlock::Locked(_)));
    }

    #[test]
    fn tip_discount_lowers_sally_dc() {
        let cat = SecretsCatalog::bundled();
        let mut world = WorldState::default();
        world.player.sally_tip_discount = true;
        let dc = cat.resolve_dc("sally_charm", &world, &TalkFocus::Quest(NpcId::Sally)).unwrap();
        assert_eq!(dc, 10);
    }
}
