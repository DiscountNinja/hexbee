//! Thin hex rule table — no LLM. Hits before Purpose organ.

use serde::{Deserialize, Serialize};

use crate::organs::interpret::wants_leave_command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnMode {
    Talk,
    Act,
    Query,
    Ambiguous,
}

impl TurnMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "talk" | "speech" | "chat" => Some(Self::Talk),
            "act" | "action" | "attack" => Some(Self::Act),
            "query" => Some(Self::Query),
            "ambiguous" => Some(Self::Ambiguous),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Talk => "talk",
            Self::Act => "act",
            Self::Query => "query",
            Self::Ambiguous => "ambiguous",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleHit {
    pub mode: TurnMode,
    pub targets: Vec<String>,
    pub summary: Option<String>,
    pub action_hint: Option<String>,
    pub social_act: bool,
}

/// Match cheap deterministic patterns on **already normalized** text.
pub fn match_rules_normalized(lower: &str) -> Option<RuleHit> {
    let trimmed = lower.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed;

    // Query
    if is_query(&lower) {
        return Some(RuleHit {
            mode: TurnMode::Query,
            targets: vec![],
            summary: Some("query".into()),
            action_hint: None,
            social_act: false,
        });
    }

    // Meta acts
    if lower == "help" || lower == "?" || lower == "commands" {
        return Some(act("help", false));
    }
    if lower == "look"
        || lower == "look around"
        || lower == "l"
        || lower == "survey"
        || lower == "examine room"
        || lower.contains("look for")
        || lower.contains("search for")
        || (lower.starts_with("look ") && !lower.contains("at me"))
    {
        return Some(act("look", false));
    }
    if lower.contains("take a seat")
        || lower.contains("sit down")
        || word_in_text(lower, "sit")
        || lower.contains("have a seat")
    {
        return Some(act("sit", false));
    }
    if lower.contains("pet ")
        || lower.contains("stroke ")
        || lower.contains("pat the")
        || lower.contains("pat a")
    {
        return Some(act("pet", false));
    }
    if lower == "inventory" || lower == "inv" || lower == "i" && trimmed.len() == 1 {
        return Some(RuleHit {
            mode: TurnMode::Query,
            targets: vec![],
            summary: Some("inventory".into()),
            action_hint: Some("inventory".into()),
            social_act: false,
        });
    }

    // Leave / go / enter
    if wants_leave_command(&lower) {
        return Some(act("leave", false));
    }
    if is_enter_or_go(&lower) {
        return Some(RuleHit {
            mode: TurnMode::Act,
            targets: vec![],
            summary: Some("go".into()),
            action_hint: Some("go".into()),
            social_act: false,
        });
    }

    // Pay / buy / sleep
    if is_pay(&lower) {
        return Some(act("pay_scam", false));
    }
    if lower.contains("buy food")
        || lower.contains("buy a meal")
        || lower == "eat"
        || lower.contains("order food")
    {
        return Some(act("buy_food", false));
    }
    if lower == "sleep" || lower.starts_with("sleep ") || lower.contains("go to bed") {
        return Some(act("sleep", false));
    }

    // Combat
    if is_attack(&lower) {
        let targets = extract_npc_targets(&lower);
        return Some(RuleHit {
            mode: TurnMode::Act,
            targets: targets.clone(),
            summary: Some("fight".into()),
            action_hint: Some("fight".into()),
            social_act: false,
        });
    }

    // Social pressure → Talk (skill roll decided in social.rs, not Purpose LLM).
    if let Some(hint) = social_pressure_hint(&lower) {
        return Some(RuleHit {
            mode: TurnMode::Talk,
            targets: extract_npc_targets(&lower),
            summary: Some("social".into()),
            action_hint: Some(hint.into()),
            social_act: false,
        });
    }

    // Tip / charm / password / impress — clear verbs
    if lower.contains("tip sally") || (lower.contains("tip") && lower.contains("sally")) {
        return Some(act("tip_sally", false));
    }
    if lower.contains("charm sally") || (lower.contains("charm") && lower.contains("sally")) {
        return Some(RuleHit {
            mode: TurnMode::Talk,
            targets: vec!["sally".into()],
            summary: Some("charm".into()),
            action_hint: Some("charm_sally".into()),
            social_act: false,
        });
    }
    if lower.contains("give password")
        || lower.contains("say the password")
        || lower.contains("speak the word")
        || lower == "password"
    {
        return Some(act("give_password", false));
    }
    if lower.contains("accept sword") || lower.contains("take the sword") {
        return Some(act("accept_sword", false));
    }

    // Explicit talk cues
    if is_explicit_talk(&lower) {
        return Some(RuleHit {
            mode: TurnMode::Talk,
            targets: extract_npc_targets(&lower),
            summary: Some("talk".into()),
            action_hint: Some("talk".into()),
            social_act: false,
        });
    }

    // Greetings / bare social openers → Talk (SceneCoordinator resolves who).
    if is_greeting(&lower) {
        return Some(RuleHit {
            mode: TurnMode::Talk,
            targets: extract_npc_targets(&lower),
            summary: Some("greeting".into()),
            action_hint: Some("talk".into()),
            social_act: false,
        });
    }

    // NPC-directed questions → Talk
    if is_npc_directed_question(&lower) {
        return Some(RuleHit {
            mode: TurnMode::Talk,
            targets: extract_npc_targets(&lower),
            summary: Some("ask npc".into()),
            action_hint: Some("talk".into()),
            social_act: false,
        });
    }

    // Named NPC + social/menu line (e.g. "hey sally, what's on the menu?").
    if is_npc_addressed_social(&lower) {
        return Some(RuleHit {
            mode: TurnMode::Talk,
            targets: extract_npc_targets(&lower),
            summary: Some("talk".into()),
            action_hint: Some("talk".into()),
            social_act: false,
        });
    }

    // Quoted speech → Talk
    if trimmed.starts_with('"') || trimmed.starts_with('\u{201c}') {
        return Some(RuleHit {
            mode: TurnMode::Talk,
            targets: extract_npc_targets(&lower),
            summary: Some("quoted speech".into()),
            action_hint: Some("talk".into()),
            social_act: false,
        });
    }

    None
}

fn act(hint: &str, social: bool) -> RuleHit {
    RuleHit {
        mode: TurnMode::Act,
        targets: vec![],
        summary: Some(hint.into()),
        action_hint: Some(hint.into()),
        social_act: social,
    }
}

fn is_query(lower: &str) -> bool {
    // NPC-directed questions are Talk, not store Query.
    if lower.starts_with("what do you")
        || lower.starts_with("what are you")
        || lower.starts_with("why do you")
        || lower.starts_with("who are you")
        || lower.starts_with("how are you")
    {
        return false;
    }
    lower.starts_with("where ")
        || lower.starts_with("who ")
        || lower.starts_with("what ")
        || lower.starts_with("how much ")
        || lower.starts_with("are there")
        || lower.starts_with("is there")
        || lower.contains("do i have")
        || lower == "inventory"
        || lower == "inv"
        || lower == "status"
        || lower == "sheet"
        || lower == "skills"
        || lower.contains("what do i have")
}

fn word_in_text(hay: &str, word: &str) -> bool {
    hay.split(|c: char| !c.is_alphanumeric())
        .any(|w| w == word)
}

/// Physical attempt to slip past a blocker into the tavern (Stealth check).
pub fn wants_sneak_past(lower: &str) -> bool {
    let lower = lower.trim();
    let sneakish = lower.contains("sneak")
        || lower.contains("sneek")
        || lower.contains("slip past")
        || lower.contains("creep past")
        || lower.contains("edge past")
        || lower.contains("steal past")
        || lower.contains("dart past")
        || lower.contains("sidle")
        || (word_in_text(lower, "past")
            && (word_in_text(lower, "goblin")
                || lower.contains("dewr")
                || word_in_text(lower, "him")
                || word_in_text(lower, "guard")
                || word_in_text(lower, "door")));
    sneakish
        && (word_in_text(lower, "past")
            || lower.contains("around")
            || lower.contains("by him")
            || lower.contains("into")
            || lower.contains("inside")
            || lower.contains("tavern")
            || lower.contains("door"))
}

fn is_enter_or_go(lower: &str) -> bool {
    matches!(
        lower,
        "enter" | "go in" | "go inside" | "walk in" | "step inside" | "head inside"
    ) || lower.starts_with("go to ")
        || lower.starts_with("go ")
        || lower.contains("sneak")
        || lower.contains("sneek")
        || lower.contains("slip past")
        || lower.contains("creep past")
        || (lower.contains("let") && (lower.contains("in") || lower.contains("pass")))
}

fn is_pay(lower: &str) -> bool {
    let pay = lower == "pay"
        || lower.starts_with("pay ")
        || lower.contains("pay the")
        || lower.contains("pay him")
        || lower.contains("pay the goblin")
        || lower.contains("pay the toll")
        || lower.contains("pay the tax")
        || lower.contains("hand over") && lower.contains("coin");
    pay && !lower.contains("not pay")
        && !lower.contains("won't pay")
        && !lower.contains("wont pay")
        && !lower.contains("don't pay")
}

fn is_attack(lower: &str) -> bool {
    lower.contains("attack")
        || lower.contains("fight")
        || lower.contains("strike")
        || lower.contains("kill")
        || (lower.contains("hit ") && !lower.contains("hit on"))
}

fn is_explicit_talk(lower: &str) -> bool {
    lower.starts_with("say ")
        || lower.starts_with("tell ")
        || lower.starts_with("ask ")
        || lower.contains("talk to")
        || lower.contains("speak to")
        || lower == "talk"
}

fn is_greeting(lower: &str) -> bool {
    matches!(
        lower,
        "hello" | "hi" | "hey" | "howdy" | "greetings" | "good day" | "good evening"
    ) || lower.starts_with("hello ")
        || lower.starts_with("hi ")
        || lower.starts_with("hey ")
}

fn is_npc_directed_question(lower: &str) -> bool {
    lower.starts_with("what do you")
        || lower.starts_with("what are you")
        || lower.starts_with("why do you")
        || lower.starts_with("who are you")
        || lower.starts_with("how are you")
        || lower.starts_with("can you ")
        || lower.starts_with("could you ")
        || lower.starts_with("would you ")
}

/// Line names a present NPC and is clearly social (not a room query or physical act).
pub fn is_npc_addressed_social(lower: &str) -> bool {
    if extract_npc_targets(lower).is_empty() {
        return false;
    }
    lower.contains('?')
        || lower.contains("menu")
        || lower.contains("wares")
        || lower.contains("hello")
        || lower.contains("thanks")
        || lower.contains("thank you")
        || lower.contains("how much")
        || lower.contains("what's on")
        || lower.contains("whats on")
        || lower == "okay"
        || lower == "ok"
        || lower.ends_with(" okay")
        || lower.ends_with(" ok")
}

/// True when the line should use the Actor talk path, not narrator action beats.
pub fn is_talk_routing_line(lower: &str) -> bool {
    if let Some(hit) = match_rules_normalized(lower) {
        return hit.mode == TurnMode::Talk;
    }
    is_npc_addressed_social(lower)
}

fn social_pressure_hint(lower: &str) -> Option<&'static str> {
    if lower.contains("intimidate")
        || lower.contains("threaten")
        || lower.contains("creep out")
        || lower.contains("step aside")
        || lower.contains("move aside")
        || lower.contains("or else")
        || (lower.contains("draw") && lower.contains("knife") && !lower.contains("attack"))
    {
        return Some("challenge_dewr");
    }
    if (lower.contains("not pay")
        || lower.contains("won't pay")
        || lower.contains("wont pay")
        || lower.contains("refuse")
        || lower.contains("scam")
        || lower.contains("call out")
        || lower.contains("call-out"))
        && (crate::slang::mentions_npc(lower, "goblin")
            || lower.contains("dewr")
            || lower.contains("tax")
            || lower.contains("toll")
            || lower.contains("door")
            || lower.contains("scam"))
    {
        return Some("call_out_scam");
    }
    if lower.contains("persuade") || lower.contains("convince") || lower.contains("talk into")
    {
        if lower.contains("sally") {
            return Some("charm_sally");
        }
        if lower.contains("groll") || lower.contains("cloak") || lower.contains("password") {
            return Some("persuade_groll");
        }
        if lower.contains("dremis") || lower.contains("impress") {
            return Some("impress_dremis");
        }
        return Some("challenge_dewr");
    }
    if lower.contains("impress") {
        return Some("impress_dremis");
    }
    None
}

/// Normalize then match deterministic rules.
pub fn match_rules(input: &str) -> Option<RuleHit> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = crate::slang::normalize(trimmed);
    match_rules_normalized(&lower)
}

pub fn extract_npc_targets(lower: &str) -> Vec<String> {
    crate::slang::extract_npc_targets(lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pet_is_act_after_lexicon_normalize() {
        let n = crate::lexicon::Lexicon::bundled().normalize("I pet the sleeping kitty.");
        let h = match_rules_normalized(&n).expect("pet rule");
        assert_eq!(h.mode, TurnMode::Act);
        assert_eq!(h.action_hint.as_deref(), Some("pet"));
    }

    #[test]
    fn inventory_is_query() {
        let h = match_rules("inventory").unwrap();
        assert_eq!(h.mode, TurnMode::Query);
    }

    #[test]
    fn gobbo_pejorative_targets_goblin() {
        let h = match_rules("Hey Gobbo, move aside or you'll get hurt!").unwrap();
        assert_eq!(h.mode, TurnMode::Talk);
        assert!(h.targets.contains(&"goblin".into()));
        assert_eq!(h.action_hint.as_deref(), Some("challenge_dewr"));
    }

    #[test]
    fn asside_typo_intimidation_routes() {
        let h = match_rules("Move asside before you get hurt.").unwrap();
        assert_eq!(h.mode, TurnMode::Talk);
        assert_eq!(h.action_hint.as_deref(), Some("challenge_dewr"));
    }

    #[test]
    fn intimidate_is_talk_then_check() {
        let h = match_rules("I intimidate the goblin — move aside").unwrap();
        assert_eq!(h.mode, TurnMode::Talk);
        assert!(!h.social_act);
        assert_eq!(h.action_hint.as_deref(), Some("challenge_dewr"));
    }

    #[test]
    fn look_is_act() {
        let h = match_rules("look around").unwrap();
        assert_eq!(h.action_hint.as_deref(), Some("look"));
    }

    #[test]
    fn ambiguous_freeform_misses() {
        assert!(match_rules("hmm maybe something weird").is_none());
    }

    #[test]
    fn sneak_past_goblin_is_sneak_intent() {
        assert!(wants_sneak_past("sneak past the goblin"));
        assert!(!wants_sneak_past("enter the hall"));
    }

    #[test]
    fn hey_sally_menu_routes_talk() {
        let n = crate::lexicon::Lexicon::bundled().normalize("Hey Sally, what's on the menu?");
        let h = match_rules_normalized(&n).expect("menu talk");
        assert_eq!(h.mode, TurnMode::Talk);
        assert!(h.targets.iter().any(|t| t == "sally"));
        assert!(is_talk_routing_line(&n));
    }
}
