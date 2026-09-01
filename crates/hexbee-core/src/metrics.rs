//! Process-wide counters for seeing what Hexbee is doing.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde::Serialize;

static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn start() -> Instant {
    *START.get_or_init(Instant::now)
}

struct Counter(AtomicU64);

impl Counter {
    const fn new() -> Self {
        Self(AtomicU64::new(0))
    }
    fn inc(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

static INTENTS_CHAT: Counter = Counter::new();
static INTENTS_COMMAND: Counter = Counter::new();
static INTENTS_SETTINGS: Counter = Counter::new();
static INTENTS_PLAY_RPG: Counter = Counter::new();
static RPG_TURNS: Counter = Counter::new();
static RPG_CHECKS: Counter = Counter::new();
static RPG_CHECKS_SUCCESS: Counter = Counter::new();
static RPG_CHECKS_FAIL: Counter = Counter::new();
static RPG_COMBAT_ROUNDS: Counter = Counter::new();
static RPG_SCAM_PAID: Counter = Counter::new();
static RPG_SCAM_CLEARED: Counter = Counter::new();
static RPG_VICTORY: Counter = Counter::new();
static RPG_DEATH: Counter = Counter::new();
static RPG_BAD_ENDING: Counter = Counter::new();
static LLM_CALLS: Counter = Counter::new();
static LLM_SUCCESS: Counter = Counter::new();
static LLM_ERRORS: Counter = Counter::new();
static LLM_FALLBACKS: Counter = Counter::new();
static ETHICS_ALLOW: Counter = Counter::new();
static ETHICS_DENY: Counter = Counter::new();

pub fn inc_intent_chat() {
    INTENTS_CHAT.inc();
}
pub fn inc_intent_command() {
    INTENTS_COMMAND.inc();
}
pub fn inc_intent_settings() {
    INTENTS_SETTINGS.inc();
}
pub fn inc_intent_play_rpg() {
    INTENTS_PLAY_RPG.inc();
}
pub fn inc_rpg_turn() {
    RPG_TURNS.inc();
}
pub fn inc_rpg_check(success: bool) {
    RPG_CHECKS.inc();
    if success {
        RPG_CHECKS_SUCCESS.inc();
    } else {
        RPG_CHECKS_FAIL.inc();
    }
}
pub fn inc_rpg_combat_round() {
    RPG_COMBAT_ROUNDS.inc();
}
pub fn inc_rpg_scam_paid() {
    RPG_SCAM_PAID.inc();
}
pub fn inc_rpg_scam_cleared() {
    RPG_SCAM_CLEARED.inc();
}
pub fn inc_rpg_victory() {
    RPG_VICTORY.inc();
}
pub fn inc_rpg_death() {
    RPG_DEATH.inc();
}
pub fn inc_rpg_bad_ending() {
    RPG_BAD_ENDING.inc();
}
pub fn inc_llm_call() {
    LLM_CALLS.inc();
}
pub fn inc_llm_success() {
    LLM_SUCCESS.inc();
}
pub fn inc_llm_error() {
    LLM_ERRORS.inc();
}
pub fn inc_llm_fallback() {
    LLM_FALLBACKS.inc();
}
pub fn inc_ethics_allow() {
    ETHICS_ALLOW.inc();
}
pub fn inc_ethics_deny() {
    ETHICS_DENY.inc();
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    pub uptime_secs: u64,
    pub intents_chat: u64,
    pub intents_command: u64,
    pub intents_settings: u64,
    pub intents_play_rpg: u64,
    pub rpg_turns: u64,
    pub rpg_checks: u64,
    pub rpg_checks_success: u64,
    pub rpg_checks_fail: u64,
    pub rpg_combat_rounds: u64,
    pub rpg_scam_paid: u64,
    pub rpg_scam_cleared: u64,
    pub rpg_victory: u64,
    pub rpg_death: u64,
    pub rpg_bad_ending: u64,
    pub llm_calls: u64,
    pub llm_success: u64,
    pub llm_errors: u64,
    pub llm_fallbacks: u64,
    pub ethics_allow: u64,
    pub ethics_deny: u64,
}

pub fn snapshot() -> MetricsSnapshot {
    let _ = start();
    MetricsSnapshot {
        uptime_secs: start().elapsed().as_secs(),
        intents_chat: INTENTS_CHAT.get(),
        intents_command: INTENTS_COMMAND.get(),
        intents_settings: INTENTS_SETTINGS.get(),
        intents_play_rpg: INTENTS_PLAY_RPG.get(),
        rpg_turns: RPG_TURNS.get(),
        rpg_checks: RPG_CHECKS.get(),
        rpg_checks_success: RPG_CHECKS_SUCCESS.get(),
        rpg_checks_fail: RPG_CHECKS_FAIL.get(),
        rpg_combat_rounds: RPG_COMBAT_ROUNDS.get(),
        rpg_scam_paid: RPG_SCAM_PAID.get(),
        rpg_scam_cleared: RPG_SCAM_CLEARED.get(),
        rpg_victory: RPG_VICTORY.get(),
        rpg_death: RPG_DEATH.get(),
        rpg_bad_ending: RPG_BAD_ENDING.get(),
        llm_calls: LLM_CALLS.get(),
        llm_success: LLM_SUCCESS.get(),
        llm_errors: LLM_ERRORS.get(),
        llm_fallbacks: LLM_FALLBACKS.get(),
        ethics_allow: ETHICS_ALLOW.get(),
        ethics_deny: ETHICS_DENY.get(),
    }
}

impl MetricsSnapshot {
    pub fn render_text(&self) -> String {
        format!(
            "📊 Hexbee metrics (uptime {}s)\n\
             Intents — chat:{} command:{} settings:{} play_rpg:{}\n\
             RPG — turns:{} checks:{}/{}/{} combat_rounds:{} scam_paid:{} scam_cleared:{}\n\
             Endings — victory:{} death:{} bad:{}\n\
             LLM — calls:{} ok:{} err:{} fallbacks:{}\n\
             Ethics — allow:{} deny:{}",
            self.uptime_secs,
            self.intents_chat,
            self.intents_command,
            self.intents_settings,
            self.intents_play_rpg,
            self.rpg_turns,
            self.rpg_checks,
            self.rpg_checks_success,
            self.rpg_checks_fail,
            self.rpg_combat_rounds,
            self.rpg_scam_paid,
            self.rpg_scam_cleared,
            self.rpg_victory,
            self.rpg_death,
            self.rpg_bad_ending,
            self.llm_calls,
            self.llm_success,
            self.llm_errors,
            self.llm_fallbacks,
            self.ethics_allow,
            self.ethics_deny,
        )
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_move() {
        inc_rpg_turn();
        let s = snapshot();
        assert!(s.rpg_turns >= 1);
    }
}
