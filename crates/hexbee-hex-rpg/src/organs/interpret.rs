//! Catalog planner — retired from hot path; `heuristic_interpret_plan` + tests remain.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use hexbee_core::metrics;

use hexbee_provider::{complete_json_with_retry, DEFAULT_JSON_ATTEMPTS};
use hexbee_provider::LlmClient;

use crate::rules::wants_sneak_past;

/// One step mapped into the rules engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionTag {
    /// Snake_case catalog id, e.g. `call_out_scam`, `persuade_groll`, `go`.
    pub action: String,
    #[serde(default)]
    pub npc: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub confidence: Option<u8>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Ordered breakdown of what the player is trying to do this turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPlan {
    pub steps: Vec<ActionTag>,
    #[serde(default)]
    pub summary: Option<String>,
}

pub const MAX_STEPS_PER_TURN: usize = 2;

/// Coarse turn kind — AI decides this first, then picks a catalog step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnKind {
    /// Physical / inventory / travel / pay / fight / leave.
    Action,
    /// Conversation / banter with someone present.
    Chat,
    /// Contested social push (intimidate, charm, persuade, call-out, impress).
    Check,
}

impl TurnKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Chat => "chat",
            Self::Check => "check",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "action" | "act" | "do" => Some(Self::Action),
            "chat" | "talk" | "say" | "dialogue" => Some(Self::Chat),
            "check" | "social" | "contest" | "skill" => Some(Self::Check),
            _ => None,
        }
    }
}

/// Fine-grained speech act used for heuristic fallbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechIntent {
    Look,
    Inventory,
    Help,
    Approach,
    Threaten,
    // Insult,
    Pay,
    Refuse,
    Talk,
    Go,
    Buy,
    Sleep,
    Tip,
    Charm,
    Persuade,
    Fight,
    GivePassword,
    Impress,
    // AcceptSword,
    Leave,
    // Other,
}

impl SpeechIntent {
    // pub fn as_str(self) -> &'static str { … }

    pub fn turn_kind(self) -> TurnKind {
        match self {
            Self::Talk => TurnKind::Chat,
            Self::Threaten
            | Self::Refuse
            | Self::Charm
            | Self::Persuade
            | Self::Impress
            | Self::GivePassword => TurnKind::Check,
            _ => TurnKind::Action,
        }
    }
}

const CATALOG_IDS: &[&str] = &[
    "look",
    "inventory",
    "help",
    "go",
    "talk",
    "buy_food",
    "sleep",
    "tip_sally",
    "charm_sally",
    "persuade_groll",
    "challenge_dewr",
    "give_password",
    "impress_dremis",
    "fight",
    "accept_sword",
    "pay_scam",
    "call_out_scam",
    "leave",
];

pub fn is_catalog_action(id: &str) -> bool {
    let normalized = id.trim().to_lowercase().replace('-', "_");
    CATALOG_IDS.contains(&normalized.as_str())
}

fn normalize_tag(mut tag: ActionTag) -> Option<ActionTag> {
    tag.action = tag.action.trim().to_lowercase().replace('-', "_");
    if tag.action == "pay" {
        tag.action = "pay_scam".into();
    }
    if tag.action == "refuse" {
        tag.action = "call_out_scam".into();
    }
    if !is_catalog_action(&tag.action) {
        return None;
    }
    tag.npc = tag.npc.map(|n| n.to_lowercase());
    tag.location = tag.location.map(|l| l.to_lowercase());
    tag.confidence = tag.confidence.map(|c| c.min(5));
    Some(tag)
}

fn guard_plan(mut plan: ActionPlan) -> ActionPlan {
    let mut steps = Vec::new();
    for tag in plan.steps.drain(..) {
        match normalize_tag(tag) {
            Some(t) => steps.push(t),
            None => {
                warn!("dropped non-catalog action from plan");
                metrics::inc_llm_fallback();
            }
        }
        if steps.len() >= MAX_STEPS_PER_TURN {
            break;
        }
    }
    if steps.is_empty() {
        steps.push(tag("talk", None, None, "guard: empty plan → talk"));
    }
    ActionPlan {
        steps,
        summary: plan.summary,
    }
}

/// Offline / fallback breakdown when the LLM is unavailable.
pub fn heuristic_interpret_plan(player_text: &str, location: &str) -> ActionPlan {
    let lower = player_text.trim().to_lowercase();
    let loc = location.to_lowercase();
    let mut steps = Vec::new();

    // Compound: tip then charm
    if lower.contains("tip")
        && (lower.contains("charm")
            || lower.contains("flirt")
            || lower.contains("seduce")
            || lower.contains("sweet"))
    {
        steps.push(tag("tip_sally", Some("sally"), None, "heuristic: tip then charm"));
        steps.push(tag("charm_sally", Some("sally"), None, "heuristic: tip then charm"));
        return guard_plan(ActionPlan {
            steps,
            summary: Some("tip then charm Sally".into()),
        });
    }

    steps.push(heuristic_interpret_action(player_text, &loc));
    guard_plan(ActionPlan {
        steps,
        summary: Some("heuristic single-step".into()),
    })
}

/// True if `word` appears as a whole token (avoids `meat`⊃`eat`, `multiple`⊃`tip`).
fn has_word(hay: &str, word: &str) -> bool {
    hay.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .any(|tok| tok == word)
}

fn has_any_word(hay: &str, words: &[&str]) -> bool {
    words.iter().any(|w| has_word(hay, w))
}

/// Violence / intimidation cues — must beat food/economy heuristics.
fn looks_like_threat(lower: &str) -> bool {
    lower.contains("intimidate")
        || lower.contains("threaten")
        || lower.contains("creep out")
        || lower.contains("scare")
        || lower.contains("stare down")
        || lower.contains("mean mug")
        || lower.contains("stabbing")
        || lower.contains("unless you move")
        || lower.contains("move aside")
        || lower.contains("step aside")
        || lower.contains("scram")
        || lower.contains("loom")
        || lower.contains("creepy smile")
        || lower.contains("paint the floor")
        || lower.contains("with your blood")
        || lower.contains("your blood")
        || lower.contains("cut yourself")
        || lower.contains("cutyourself")
        || (lower.contains("lick") && (lower.contains("blade") || lower.contains("knife")))
        || lower.contains("pull out my knife")
        || lower.contains("draw my knife")
        || lower.contains("stick you")
        || lower.contains("stick 'em")
        || has_any_word(lower, &["stab", "kill", "murder", "slit"])
        || (has_word(lower, "knife")
            && has_any_word(lower, &["pull", "draw", "wave", "point", "lick"]))
}

/// Single-step heuristic (also used as the default step builder).
pub fn heuristic_interpret_action(player_text: &str, location: &str) -> ActionTag {
    let lower = player_text.trim().to_lowercase();
    let loc = location.to_lowercase();

    // Exact short aliases (optional shortcuts — still go through the catalog).
    match lower.as_str() {
        "help" | "?" | "commands" => return tag("help", None, None, "alias"),
        "look" | "l" | "examine" => return tag("look", None, None, "alias"),
        "inventory" | "inv" | "i" | "stats" => return tag("inventory", None, None, "alias"),
        "pay" => return tag("pay_scam", None, None, "alias"),
        "refuse" => return tag("call_out_scam", Some("dewr"), None, "alias"),
        "enter" => return tag("go", None, Some("hall"), "alias"),
        "leave" | "exit" => return tag("leave", None, None, "alias"),
        "sleep" => return tag("sleep", None, None, "alias"),
        _ => {}
    }

    let refusing_pay = (has_word(&lower, "not") && has_word(&lower, "pay"))
        || lower.contains("won't pay")
        || lower.contains("wont pay")
        || has_word(&lower, "refuse")
        || lower.contains("ain't paying")
        || lower.contains("aint paying")
        || lower.contains("don't pay")
        || lower.contains("dont pay");

    // Threat BEFORE food. Intimidation → challenge; only clear lethal commit → fight.
    if looks_like_threat(&lower)
        || has_any_word(&lower, &["fight", "attack", "stab"])
        || lower.contains("draw steel")
    {
        let intimidate_primary = lower.contains("intimidate")
            || lower.contains("creep out")
            || lower.contains("threaten")
            || lower.contains("scare")
            || lower.contains("move aside")
            || lower.contains("step aside")
            || lower.contains("unless you move")
            || lower.contains("scram")
            || lower.contains("loom")
            || lower.contains("stabbing discount")
            || lower.contains("daily stabbing")
            || lower.contains("paint the floor")
            || lower.contains("your blood")
            || (lower.contains("stabbing") && !has_word(&lower, "stab"))
            || (lower.contains("draw") && lower.contains("knife") && !has_word(&lower, "attack"));
        // Lethal only when clearly committing to a kill, not a scare-off.
        let lethal = !intimidate_primary
            && (has_any_word(&lower, &["kill", "attack", "fight", "murder"])
                || (has_word(&lower, "stab") && !lower.contains("stabbing"))
                || lower.contains("stick you")
                || lower.contains("stick 'em"));
        let npc = if lower.contains("dremis") || lower.contains("warlord") {
            Some("dremis")
        } else if lower.contains("groll") {
            Some("groll")
        } else if lower.contains("dewr")
            || lower.contains("goblin")
            || loc.contains("outside")
        {
            Some("dewr")
        } else {
            None
        };
        if lethal {
            return tag("fight", npc, None, "heuristic: lethal→fight");
        }
        if loc.contains("outside") || npc == Some("dewr") {
            return tag(
                "challenge_dewr",
                Some("dewr"),
                None,
                "heuristic: intimidate→challenge",
            );
        }
        return tag("fight", npc, None, "heuristic: threat→fight");
    }

    if lower.contains("scam")
        || lower.contains("bluff")
        || has_word(&lower, "fake")
        || refusing_pay
        || lower.contains("see through")
        || lower.contains("call out")
        || (lower.contains("door tax")
            && (has_word(&lower, "no") || lower.contains("real") || has_word(&lower, "lie")))
    {
        return tag("call_out_scam", Some("dewr"), None, "heuristic: refuse scam");
    }

    if loc.contains("outside")
        && has_word(&lower, "pay")
        && !refusing_pay
        && (has_word(&lower, "gold")
            || has_word(&lower, "fee")
            || has_word(&lower, "tax")
            || has_word(&lower, "coin")
            || has_word(&lower, "him")
            || lower.contains("dewr"))
    {
        return tag("pay_scam", None, None, "heuristic: pay fee");
    }

    if (lower.contains("sweet")
        && (lower.contains("sally") || lower.contains("wench") || has_word(&lower, "her")))
        || lower.contains("flirt")
        || lower.contains("seduce")
        || has_word(&lower, "charm")
        || (lower.contains("compliment") && lower.contains("sally"))
    {
        return tag("charm_sally", Some("sally"), None, "heuristic: charm");
    }

    if ((lower.contains("password") || lower.contains("storage") || lower.contains("secret"))
        && (lower.contains("groll") || lower.contains("cloak") || lower.contains("newt")))
        || (has_word(&lower, "ask") && lower.contains("password"))
        || (lower.contains("convince") && (lower.contains("groll") || lower.contains("cloak")))
    {
        return tag(
            "persuade_groll",
            Some("groll"),
            None,
            "heuristic: password from groll",
        );
    }

    if has_word(&lower, "impress")
        || (has_word(&lower, "prove") && (lower.contains("dremis") || lower.contains("warlord")))
        || (has_word(&lower, "honor") && lower.contains("dremis"))
    {
        return tag("impress_dremis", Some("dremis"), None, "heuristic: impress");
    }

    if lower.contains("password")
        && (has_word(&lower, "give")
            || has_word(&lower, "tell")
            || has_word(&lower, "speak")
            || has_word(&lower, "say")
            || lower.contains("dremis"))
    {
        return tag(
            "give_password",
            Some("dremis"),
            None,
            "heuristic: give password",
        );
    }

    if lower.contains("step aside")
        || lower.contains("move aside")
        || lower.contains("pint size")
        || (loc.contains("outside")
            && (lower.contains("back down") || lower.contains("get out of my way")))
    {
        return tag("challenge_dewr", Some("dewr"), None, "heuristic: step aside");
    }

    if has_word(&lower, "challenge")
        || lower.contains("back down")
        || lower.contains("stand up to")
    {
        return tag("challenge_dewr", Some("dewr"), None, "heuristic: challenge");
    }

    if lower.contains("approach")
        && (lower.contains("door") || lower.contains("entrance") || lower.contains("tavern"))
    {
        return tag("go", None, Some("hall"), "heuristic: approach door");
    }

    // Sneak / slip / creep past the door guard = try to enter, not chat.
    if wants_sneak_past(&lower) {
        return tag("go", None, Some("hall"), "heuristic: sneak past → go");
    }

    if lower.contains("enter")
        || lower.contains("go in")
        || lower.contains("head inside")
        || lower.contains("into the tavern")
        || lower.contains("through the door")
    {
        return tag("go", None, Some("hall"), "heuristic: enter");
    }
    if lower.contains("upstairs") || lower.contains("bedroom") {
        return tag("go", None, Some("upstairs"), "heuristic: go upstairs");
    }
    if has_word(&lower, "kitchen") {
        return tag("go", None, Some("kitchen"), "heuristic: go kitchen");
    }
    if lower.contains("storage") || lower.contains("hidden door") {
        return tag("go", None, Some("storage"), "heuristic: go storage");
    }
    if has_word(&lower, "outside") || lower.contains("back out") {
        return tag("go", None, Some("outside"), "heuristic: go outside");
    }

    // Word-boundary food — never match via "meat"⊃"eat".
    if has_any_word(&lower, &["eat", "food", "hungry", "meal", "ale", "drink"])
        && !lower.contains("goblin meat")
        && !lower.contains("meat in a while")
    {
        return tag("buy_food", None, None, "heuristic: food");
    }
    if has_word(&lower, "sleep")
        || lower.contains("rent a room")
        || (has_word(&lower, "rest") && loc.contains("upstairs"))
    {
        return tag("sleep", None, None, "heuristic: sleep");
    }
    if has_word(&lower, "tip") || lower.contains("tip sally") {
        return tag("tip_sally", Some("sally"), None, "heuristic: tip");
    }
    if has_word(&lower, "sword")
        && (has_word(&lower, "buy") || has_word(&lower, "trade") || has_word(&lower, "accept"))
    {
        return tag("accept_sword", None, None, "heuristic: sword trade");
    }
    // "let me in" / get inside = try the door — never the leave/ambush ending.
    if wants_enter_attempt(&lower) {
        return tag("go", None, Some("hall"), "heuristic: let me in → go");
    }
    if wants_leave_command(&lower) {
        return tag("leave", None, None, "heuristic: leave");
    }
    // Sit / claim a seat in the hall — survey the room, don't default-talk Dewr.
    if lower.contains("seat")
        || (has_word(&lower, "sit") && !lower.contains("aside"))
        || (lower.contains("claim") && (lower.contains("spot") || lower.contains("table")))
    {
        return tag("look", None, None, "heuristic: claim seat / look around");
    }
    // Look only as a real survey command — never "Oh look what we have here".
    if wants_look_command(&lower) {
        return tag("look", None, None, "heuristic: look");
    }
    if lower.contains("inventory") || lower.contains("what do i have") || lower.contains("my stuff")
    {
        return tag("inventory", None, None, "heuristic: inventory");
    }
    if has_word(&lower, "help") {
        return tag("help", None, None, "heuristic: help");
    }

    let npc = if lower.contains("sally") || lower.contains("wench") {
        Some("sally")
    } else if lower.contains("groll") || lower.contains("cloak") {
        Some("groll")
    } else if lower.contains("dremis") || lower.contains("warlord") {
        Some("dremis")
    } else if lower.contains("dewr") || lower.contains("goblin") {
        Some("dewr")
    } else if loc.contains("storage") {
        Some("dremis")
    } else if loc.contains("outside") {
        Some("dewr")
    } else {
        None
    };
    tag("talk", npc, None, "heuristic: default talk")
}

/// Trying to get inside — not abandoning the scene.
fn wants_enter_attempt(lower: &str) -> bool {
    lower.contains("let me in")
        || lower.contains("let us in")
        || lower.contains("letting me in")
        || lower.contains("refusing to let")
        || lower.contains("won't let me")
        || lower.contains("wont let me")
        || lower.contains("won't let us")
        || (lower.contains("why")
            && lower.contains("let")
            && (lower.contains("in") || lower.contains("pass") || lower.contains("enter")))
}

/// True only for abandoning the scene / walking away from the grounds.
/// Never match "let me in", "leave him alone", etc.
pub fn wants_leave_command(text: &str) -> bool {
    let lower = text.to_lowercase();
    let lower = lower.as_str();
    if wants_enter_attempt(lower) {
        return false;
    }
    let trimmed = lower.trim().trim_end_matches(|c: char| matches!(c, '.' | '!' | '?'));
    if matches!(trimmed, "leave" | "exit" | "quit scene" | "i'm done" | "im done") {
        return true;
    }
    if lower.contains("walk away")
        || lower.contains("get out of here")
        || lower.contains("leave this place")
        || lower.contains("leave the tavern")
        || lower.contains("leave the area")
        || lower.contains("abandon")
        || lower.contains("i'm leaving")
        || lower.contains("im leaving")
        || lower.contains("i am leaving")
        || has_word(lower, "leaving")
    {
        return true;
    }
    // Bare "leave" as a verb of departure, not "leave me alone" / "leave him".
    has_word(lower, "leave")
        && !lower.contains("leave me")
        && !lower.contains("leave him")
        && !lower.contains("leave her")
        && !lower.contains("leave it")
        && !lower.contains("leave them")
        && !lower.contains("leave the goblin")
        && !lower.contains("leave dewr")
}

/// True only for genuine look/survey commands, not conversational "oh look…".
fn wants_look_command(lower: &str) -> bool {
    if lower.contains("where am i")
        || lower.contains("look around")
        || lower.contains("look about")
        || has_word(lower, "survey")
    {
        return true;
    }
    if lower.contains("look what")
        || lower.contains("look who")
        || lower.contains("oh look")
        || lower.contains("well look")
        || lower.contains("hey look")
    {
        return false;
    }
    let trimmed = lower.trim();
    trimmed == "look"
        || trimmed == "l"
        || trimmed == "examine"
        || trimmed.starts_with("look at ")
        || trimmed.starts_with("look to ")
        || (trimmed.starts_with("look ") && trimmed.split_whitespace().count() <= 4)
}

/// Classify a coarse speech intent (heuristic; used before / instead of fat catalog prompts).
pub fn classify_speech_intent(player_text: &str, location: &str) -> SpeechIntent {
    let lower = player_text.trim().to_lowercase();
    let loc = location.to_lowercase();
    match lower.as_str() {
        "help" | "?" | "commands" => return SpeechIntent::Help,
        "look" | "l" | "examine" => return SpeechIntent::Look,
        "inventory" | "inv" | "i" | "stats" => return SpeechIntent::Inventory,
        "pay" => return SpeechIntent::Pay,
        "refuse" => return SpeechIntent::Refuse,
        "enter" => return SpeechIntent::Approach,
        "leave" | "exit" => return SpeechIntent::Leave,
        "sleep" => return SpeechIntent::Sleep,
        _ => {}
    }
    if looks_like_threat(&lower)
        || lower.contains("intimidate")
        || lower.contains("step aside")
        || lower.contains("move aside")
        || lower.contains("pint size")
        || lower.contains("stare down")
        || lower.contains("mean mug")
        || lower.contains("eye'n")
        || lower.contains("eyen me")
        || lower.contains("eyein")
    {
        let lethal = has_any_word(&lower, &["attack", "fight", "kill", "murder"])
            && !lower.contains("intimidate")
            && !lower.contains("stare");
        return if lethal {
            SpeechIntent::Fight
        } else {
            SpeechIntent::Threaten
        };
    }
    let refusing_pay = (has_word(&lower, "not") && has_word(&lower, "pay"))
        || lower.contains("won't pay")
        || has_word(&lower, "refuse");
    if refusing_pay || lower.contains("scam") || lower.contains("fake door") {
        return SpeechIntent::Refuse;
    }
    if loc.contains("outside")
        && (has_word(&lower, "pay")
            || lower.contains("take your gold")
            || lower.contains("here's the gold")
            || lower.contains("heres the gold")
            || lower.contains("hand over"))
        && !refusing_pay
    {
        return SpeechIntent::Pay;
    }
    if wants_look_command(&lower) {
        return SpeechIntent::Look;
    }
    if wants_sneak_past(&lower)
        || (lower.contains("approach") && (lower.contains("door") || lower.contains("tavern")))
    {
        return SpeechIntent::Approach;
    }
    if has_word(&lower, "tip") {
        return SpeechIntent::Tip;
    }
    if has_word(&lower, "charm") || lower.contains("flirt") {
        return SpeechIntent::Charm;
    }
    if lower.contains("password") && (lower.contains("groll") || lower.contains("cloak")) {
        return SpeechIntent::Persuade;
    }
    if lower.contains("password")
        && (has_word(&lower, "give") || has_word(&lower, "speak") || has_word(&lower, "say"))
    {
        return SpeechIntent::GivePassword;
    }
    if has_word(&lower, "impress") {
        return SpeechIntent::Impress;
    }
    if has_any_word(&lower, &["eat", "food", "meal", "drink", "ale"]) {
        return SpeechIntent::Buy;
    }
    if wants_leave_command(&lower) {
        return SpeechIntent::Leave;
    }
    if has_any_word(&lower, &["fight", "attack", "kill"]) {
        return SpeechIntent::Fight;
    }
    if wants_enter_attempt(&lower)
        || lower.contains("enter")
        || lower.contains("go in")
        || lower.contains("upstairs")
    {
        return SpeechIntent::Go;
    }
    SpeechIntent::Talk
}

fn candidates_for_intent(intent: SpeechIntent, location: &str) -> Vec<&'static str> {
    let loc = location.to_lowercase();
    let outside = loc.contains("outside");
    match intent {
        SpeechIntent::Look => vec!["look"],
        SpeechIntent::Inventory => vec!["inventory"],
        SpeechIntent::Help => vec!["help"],
        SpeechIntent::Approach if outside => vec!["go", "talk", "challenge_dewr"],
        SpeechIntent::Approach => vec!["go", "talk"],
        SpeechIntent::Threaten if outside => {
            vec!["challenge_dewr", "call_out_scam", "talk"]
        }
        SpeechIntent::Threaten => vec!["fight", "talk", "challenge_dewr"],
        SpeechIntent::Pay => vec!["pay_scam"],
        SpeechIntent::Refuse => vec!["call_out_scam", "challenge_dewr"],
        SpeechIntent::Talk => vec!["talk"],
        SpeechIntent::Go => vec!["go"],
        SpeechIntent::Buy => vec!["buy_food"],
        SpeechIntent::Sleep => vec!["sleep"],
        SpeechIntent::Tip => vec!["tip_sally"],
        SpeechIntent::Charm => vec!["charm_sally", "tip_sally"],
        SpeechIntent::Persuade => vec!["persuade_groll", "talk"],
        SpeechIntent::Fight => vec!["fight", "challenge_dewr"],
        SpeechIntent::GivePassword => vec!["give_password"],
        SpeechIntent::Impress => vec!["impress_dremis", "talk"],
        // SpeechIntent::AcceptSword => vec!["accept_sword"],
        SpeechIntent::Leave => vec!["leave"],
        // SpeechIntent::Other => vec!["talk", "look"],
    }
}

fn plan_from_intent(intent: SpeechIntent, player_text: &str, location: &str) -> ActionPlan {
    // Prefer the full heuristic mapper (intent-aware ordering), then clamp to candidates.
    let mut plan = heuristic_interpret_plan(player_text, location);
    let allowed = candidates_for_intent(intent, location);
    plan.steps
        .retain(|s| allowed.iter().any(|a| *a == s.action.as_str()));

    // Soft "talk" must not win when the turn is a forced social check/threat.
    if matches!(intent, SpeechIntent::Threaten | SpeechIntent::Refuse)
        && plan
            .steps
            .first()
            .is_some_and(|s| s.action == "talk")
    {
        plan.steps.clear();
    }

    if plan.steps.is_empty() {
        let action = allowed.first().copied().unwrap_or("talk");
        let npc = match action {
            "challenge_dewr" | "call_out_scam" | "pay_scam" => Some("dewr"),
            "charm_sally" | "tip_sally" => Some("sally"),
            "persuade_groll" => Some("groll"),
            "impress_dremis" | "give_password" => Some("dremis"),
            _ => None,
        };
        plan.steps.push(tag(
            action,
            npc,
            if action == "go" { Some("hall") } else { None },
            "intent fallback",
        ));
    }
    if intent != SpeechIntent::Charm {
        plan.steps.truncate(1);
    }
    guard_plan(plan)
}

/// Lowercase + strip trailing sentence punctuation for alias matching.
fn normalize_alias(player_text: &str) -> String {
    player_text
        .trim()
        .trim_end_matches(|c: char| matches!(c, '.' | '!' | '?' | ','))
        .trim()
        .to_lowercase()
}

fn tag(
    action: &str,
    npc: Option<&str>,
    location: Option<&str>,
    reason: &str,
) -> ActionTag {
    ActionTag {
        action: action.into(),
        npc: npc.map(str::to_string),
        location: location.map(str::to_string),
        confidence: Some(3),
        reason: Some(reason.into()),
    }
}

fn candidates_for_kind(kind: TurnKind, location: &str) -> Vec<&'static str> {
    let loc = location.to_lowercase();
    let outside = loc.contains("outside");
    let hall = loc.contains("hall") || loc.contains("main");
    let storage = loc.contains("storage");
    match kind {
        TurnKind::Chat => {
            if outside {
                vec!["talk", "challenge_dewr"]
            } else if hall {
                vec!["talk", "tip_sally", "charm_sally"]
            } else if storage {
                vec!["talk", "give_password", "impress_dremis"]
            } else {
                vec!["talk"]
            }
        }
        TurnKind::Check => {
            let mut v = Vec::new();
            if outside || hall {
                v.extend(["challenge_dewr", "call_out_scam"]);
            }
            if hall {
                v.extend(["charm_sally", "persuade_groll"]);
            }
            if storage {
                v.extend(["impress_dremis", "give_password"]);
            }
            if v.is_empty() {
                v.push("talk");
            }
            v
        }
        TurnKind::Action => {
            let mut v = vec![
                "look",
                "inventory",
                "help",
                "go",
                "leave",
                "fight",
                "buy_food",
                "sleep",
                "pay_scam",
                "accept_sword",
                "tip_sally",
            ];
            if outside {
                v.push("challenge_dewr");
            }
            v
        }
    }
}

/// Heuristic fallback when the reasoning agent fails.
pub fn classify_turn_kind_heuristic(player_text: &str, location: &str) -> TurnKind {
    classify_speech_intent(player_text, location).turn_kind()
}

/// Interpret open-ended player chat into a guarded [`ActionPlan`].
///
/// Pipeline: reasoning agent picks kind + catalog id → Rust sanitizes.
#[cfg(test)]
pub async fn interpret_player_plan(
    llm: &dyn LlmClient,
    player_text: &str,
    location: &str,
    visible_facts: &str,
) -> ActionPlan {
    interpret_player_plan_with_memory(llm, player_text, location, visible_facts, "").await
}

pub async fn interpret_player_plan_with_memory(
    llm: &dyn LlmClient,
    player_text: &str,
    location: &str,
    visible_facts: &str,
    memory: &str,
) -> ActionPlan {
    let lower = normalize_alias(player_text);
    // Tiny exact shortcuts only — everything else goes through the reasoning agent.
    if matches!(
        lower.as_str(),
        "help"
            | "?"
            | "look"
            | "l"
            | "look around"
            | "look about"
            | "examine"
            | "inventory"
            | "inv"
            | "i"
            | "pay"
            | "leave"
            | "exit"
            | "enter"
    ) {
        let intent = classify_speech_intent(player_text, location);
        let plan = plan_from_intent(intent, player_text, location);
        info!(kind = "shortcut", first = %plan.steps[0].action, "plan from alias");
        return plan;
    }

    // Physical sneak/slip past must not depend on the reasoner (it often maps to talk).
    if wants_sneak_past(&lower) || wants_sneak_past(&player_text.to_lowercase()) {
        let plan = plan_from_intent(SpeechIntent::Approach, player_text, location);
        info!(kind = "shortcut", first = %plan.steps[0].action, "plan from sneak-past");
        return plan;
    }

    // "Let me in" / blocked-entry questions must never become leave/ambush.
    if wants_enter_attempt(&lower) || wants_enter_attempt(&player_text.to_lowercase()) {
        let plan = plan_from_intent(SpeechIntent::Go, player_text, location);
        info!(kind = "shortcut", first = %plan.steps[0].action, "plan from enter-attempt");
        return plan;
    }

    let present = visible_facts
        .split(';')
        .map(str::trim)
        .find_map(|s| s.strip_prefix("visible_npcs="))
        .unwrap_or("(unknown)");
    let facts = visible_facts
        .split(';')
        .take(8)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("; ");

    // Full catalog for this place (reasoner chooses kind + id together).
    let catalog = catalog_for_location(location);
    let catalog_list = catalog.join(", ");

    let system = format!(
        "You are the reasoning agent for a tavern RPG rules layer.\n\
Decide what the traveler is trying to do, then map it to ONE catalog id.\n\
\n\
Kinds (pick exactly one):\n\
- chat = conversation only (hello, ask, banter) — no contested push\n\
- check = social contest / pressure (intimidate, threaten, scare off, charm, persuade, call out a scam, impress, give a secret word)\n\
- action = physical do (go, look, pay, fight, buy, sleep, leave, inventory, tip)\n\
\n\
Rules:\n\
- Outside: making the goblin move/scram/back down/yield = check → challenge_dewr (not talk).\n\
- Outside: clear lethal attack = action → fight.\n\
- Outside: sneak/slip/creep past the goblin or doorway = action → go (hall), not talk.\n\
- Outside: asking to be let in / why blocked = action → go (hall) or chat → talk. NEVER leave.\n\
- leave = abandon the whole scene / walk away from the tavern grounds (bad ending if no amulet). Not \"let me in\".\n\
- Mere greeting or questions = chat → talk.\n\
- Paying the door fee = action → pay_scam.\n\
- Calling the fee a scam/lie = check → call_out_scam.\n\
\n\
Catalog ids for this place: {catalog_list}\n\
npc: only someone in Present (dewr|sally|groll|dremis|goblin|null). Ambient names talk-only.\n\
go.location: outside|hall|kitchen|upstairs|storage.\n\
Never invent ids. Never decide success or failure.\n\
Return ONLY JSON: {{\"reason\":\"short why\",\"kind\":\"chat|check|action\",\"action\":\"<catalog id>\",\"npc\":null,\"location\":null,\"confidence\":0-5}}"
    );
    let user = format!(
        "Place: {location}\nPresent: {present}\nRecent: {}\nNotes: {}\nTraveler: {player_text}",
        if memory.is_empty() { "(none)" } else { memory },
        if facts.is_empty() { "(none)" } else { &facts },
    );

    let catalog_owned: Vec<String> = catalog.iter().map(|s| (*s).to_string()).collect();
    if let Some((kind, plan)) = complete_json_with_retry(
        llm,
        &system,
        &user,
        DEFAULT_JSON_ATTEMPTS,
        |text| accept_reasoned_plan(text, location, &catalog_owned),
    )
    .await
    {
        info!(
            kind = kind.as_str(),
            first = %plan.steps[0].action,
            reason = ?plan.steps[0].reason,
            "reasoned plan from llm"
        );
        plan
    } else {
        let kind = classify_turn_kind_heuristic(player_text, location);
        warn!(kind = kind.as_str(), "reasoned plan exhausted; heuristic");
        plan_from_kind(kind, player_text, location)
    }
}

/// Catalog union for a place — what the reasoner may pick.
fn catalog_for_location(location: &str) -> Vec<&'static str> {
    let mut ids = Vec::new();
    for kind in [TurnKind::Chat, TurnKind::Check, TurnKind::Action] {
        for id in candidates_for_kind(kind, location) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

fn accept_reasoned_plan(
    text: &str,
    location: &str,
    catalog: &[String],
) -> Option<(TurnKind, ActionPlan)> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let slice = &text[start..=end];

    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        reason: Option<String>,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        action: Option<String>,
        #[serde(default)]
        npc: Option<String>,
        #[serde(default)]
        location: Option<String>,
        #[serde(default)]
        confidence: Option<u8>,
        // Also accept nested single-step shapes.
        #[serde(default)]
        plan: Vec<ActionTag>,
        #[serde(default)]
        steps: Vec<ActionTag>,
        #[serde(default)]
        summary: Option<String>,
    }

    let raw: Raw = serde_json::from_str(slice).ok()?;

    let tag = if let Some(action) = raw.action {
        ActionTag {
            action,
            npc: raw.npc,
            location: raw.location,
            confidence: raw.confidence,
            reason: raw.reason.clone(),
        }
    } else if let Some(t) = raw.plan.into_iter().next().or_else(|| raw.steps.into_iter().next()) {
        let mut t = t;
        if t.reason.is_none() {
            t.reason = raw.reason.clone();
        }
        t
    } else {
        return None;
    };

    // Reject kind-names mistaken for actions (tiny-model failure mode).
    let act = tag.action.trim().to_lowercase();
    if matches!(
        act.as_str(),
        "action" | "chat" | "check" | "..." | "act" | "social" | "contest"
    ) {
        return None;
    }

    let tag = normalize_tag(tag)?;
    if !catalog.iter().any(|c| c == &tag.action) {
        return None;
    }

    // Prefer the catalog action; derive kind from it. Junk kinds like
    // "action|check" are ignored when the action id is valid.
    let derived = kind_for_action(&tag.action);
    if let Some(reported) = raw.kind.as_deref().and_then(TurnKind::parse) {
        if reported != derived {
            info!(
                reported = reported.as_str(),
                derived = derived.as_str(),
                action = %tag.action,
                "reasoner kind disagreed with action; trusting action"
            );
        }
    }
    if !candidates_for_kind(derived, location)
        .iter()
        .any(|c| *c == tag.action.as_str())
    {
        return None;
    }

    let mut plan = guard_plan(ActionPlan {
        steps: vec![tag],
        summary: raw.summary.or(raw.reason),
    });
    plan.steps.truncate(1);
    if plan.steps.is_empty() {
        return None;
    }
    let kind = kind_for_action(&plan.steps[0].action);
    Some((kind, plan))
}

fn kind_for_action(action: &str) -> TurnKind {
    match action {
        "talk" => TurnKind::Chat,
        "challenge_dewr" | "call_out_scam" | "charm_sally" | "persuade_groll"
        | "impress_dremis" | "give_password" => TurnKind::Check,
        _ => TurnKind::Action,
    }
}

fn plan_from_kind(kind: TurnKind, player_text: &str, location: &str) -> ActionPlan {
    let intent = classify_speech_intent(player_text, location);
    let intent = match (kind, intent.turn_kind()) {
        (k, actual) if k == actual => intent,
        (TurnKind::Chat, _) => SpeechIntent::Talk,
        (TurnKind::Check, _) => {
            let loc = location.to_lowercase();
            if loc.contains("outside") {
                SpeechIntent::Threaten
            } else if loc.contains("storage") {
                SpeechIntent::Impress
            } else {
                SpeechIntent::Charm
            }
        }
        (TurnKind::Action, _) => intent,
    };
    plan_from_intent(intent, player_text, location)
}

// /// Back-compat: first step of the plan.
// pub async fn interpret_player_action(
//     llm: &dyn LlmClient,
//     player_text: &str,
//     location: &str,
//     visible_facts: &str,
// ) -> ActionTag {
//     interpret_player_plan(llm, player_text, location, visible_facts)
//         .await
//         .steps
//         .into_iter()
//         .next()
//         .unwrap_or_else(|| tag("talk", None, None, "empty"))
// }

#[cfg(test)]
mod tests {
    use super::*;
    use hexbee_provider::FakeLlm;

    #[test]
    fn goblin_meat_intimidate_is_not_food() {
        let t = heuristic_interpret_action(
            "I haven't had Goblin meat in a while, why don't you stay there while I stick you. *I pull out my knife and lick it to intimidate and creep out the goblin.* Or you can move aside.",
            "outside the tavern",
        );
        assert_ne!(t.action, "buy_food", "meat must not match eat substring");
        assert_eq!(
            t.action, "challenge_dewr",
            "intimidate + move aside should be a check, not combat"
        );
    }

    #[test]
    fn daily_stabbing_taunt_is_challenge_not_pay() {
        let t = heuristic_interpret_action(
            "Oh hi little goblin, have you gotten your daily stabbing yet? *Draws knife to intimidate*",
            "outside the tavern",
        );
        assert_eq!(t.action, "challenge_dewr");
    }

    #[test]
    fn blood_scram_threat_is_challenge() {
        let t = heuristic_interpret_action(
            "What's this? Charity? I don't give filthy gobbos charity. Why don't you be a good flith bag and scram before I paint the floor with your blood?",
            "outside the tavern",
        );
        assert_eq!(t.action, "challenge_dewr");
    }

    #[test]
    fn check_kind_fallback_prefers_challenge_over_talk() {
        let plan = plan_from_kind(
            TurnKind::Check,
            "What's this? Charity? Scram before I paint the floor with your blood?",
            "outside the tavern",
        );
        assert_eq!(plan.steps[0].action, "challenge_dewr");
    }

    #[test]
    fn meat_does_not_match_eat_word() {
        assert!(!has_word("goblin meat in a while", "eat"));
        assert!(has_word("I want to eat stew", "eat"));
    }

    #[test]
    fn refuse_scam_outside() {
        let t = heuristic_interpret_action(
            "I'm not paying your fake door tax, goblin",
            "outside the tavern",
        );
        assert_eq!(t.action, "call_out_scam");
    }

    #[test]
    fn sweet_talk_groll() {
        let t = heuristic_interpret_action(
            "I try to convince the cloaked newt to share the storage password",
            "main hall",
        );
        assert_eq!(t.action, "persuade_groll");
    }

    #[test]
    fn tip_then_charm_plan() {
        let p = heuristic_interpret_plan(
            "I tip Sally some gold then try to charm her",
            "main hall",
        );
        assert_eq!(p.steps.len(), 2);
        assert_eq!(p.steps[0].action, "tip_sally");
        assert_eq!(p.steps[1].action, "charm_sally");
    }

    #[test]
    fn guard_drops_invented_mechanics() {
        let plan = guard_plan(ActionPlan {
            steps: vec![
                tag("summon_dragon", None, None, "bad"),
                tag("persuade_groll", Some("groll"), None, "ok"),
            ],
            summary: None,
        });
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].action, "persuade_groll");
    }

    #[test]
    fn oh_look_step_aside_is_challenge_not_look() {
        let t = heuristic_interpret_action(
            "Oh look what we have here? A Pincoushin. How about you step aside pint size?",
            "outside the tavern",
        );
        assert_ne!(t.action, "look");
        assert_eq!(t.action, "challenge_dewr");
    }

    #[test]
    fn approach_door_is_go() {
        let t = heuristic_interpret_action("Approach the door.", "outside the tavern");
        assert_eq!(t.action, "go");
    }

    #[test]
    fn sneak_past_goblin_is_go_not_talk() {
        let t = heuristic_interpret_action("Sneek past the Goblin.", "outside the tavern");
        assert_eq!(t.action, "go", "got {:?}", t);
        assert_eq!(t.location.as_deref(), Some("hall"));
        let t2 = heuristic_interpret_action("I sneak past Dewr into the tavern", "outside");
        assert_eq!(t2.action, "go");
    }

    #[test]
    fn classify_threat_intent() {
        let i = classify_speech_intent(
            "I draw my knife to intimidate the goblin",
            "outside",
        );
        assert_eq!(i, SpeechIntent::Threaten);
    }

    #[tokio::test]
    async fn llm_reasoned_plan_json() {
        let llm = FakeLlm::new([
            r#"{"reason":"paying the door fee","kind":"action","action":"pay_scam","npc":"dewr","confidence":4}"#,
        ]);
        let p = interpret_player_plan(&llm, "fine take your gold", "outside", "scam active").await;
        assert_eq!(p.steps[0].action, "pay_scam");
    }

    #[tokio::test]
    async fn llm_reasoned_threat_is_check() {
        let llm = FakeLlm::new([
            r#"{"reason":"intimidate goblin to scram","kind":"check","action":"challenge_dewr","npc":"dewr","confidence":5}"#,
        ]);
        let p = interpret_player_plan(
            &llm,
            "Scram before I paint the floor with your blood",
            "outside the tavern",
            "visible_npcs=Dewr",
        )
        .await;
        assert_eq!(p.steps[0].action, "challenge_dewr");
    }

    #[test]
    fn reasoned_plan_rejects_kind_as_action() {
        let catalog: Vec<String> = catalog_for_location("outside")
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert!(accept_reasoned_plan(
            r#"{"kind":"action|check","action":"..."}"#,
            "outside",
            &catalog,
        )
        .is_none());
        assert!(accept_reasoned_plan(
            r#"{"kind":"check","action":"check"}"#,
            "outside",
            &catalog,
        )
        .is_none());
        // Junk kind is fine if the action id is real.
        let (kind, plan) = accept_reasoned_plan(
            r#"{"kind":"action|check","action":"challenge_dewr","npc":"dewr"}"#,
            "outside",
            &catalog,
        )
        .expect("action wins over junk kind");
        assert_eq!(kind, TurnKind::Check);
        assert_eq!(plan.steps[0].action, "challenge_dewr");
    }

    #[test]
    fn reasoned_plan_accepts_challenge() {
        let catalog: Vec<String> = catalog_for_location("outside the tavern")
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let (kind, plan) = accept_reasoned_plan(
            r#"{"reason":"threat","kind":"check","action":"challenge_dewr","npc":"dewr"}"#,
            "outside the tavern",
            &catalog,
        )
        .expect("accepted");
        assert_eq!(kind, TurnKind::Check);
        assert_eq!(plan.steps[0].action, "challenge_dewr");
    }

    #[tokio::test]
    async fn sneak_past_shortcut_skips_llm_talk() {
        // Reasoner would wrongly return talk; empty fake proves we never called it.
        let llm = FakeLlm::new(Vec::<String>::new());
        let p = interpret_player_plan(
            &llm,
            "Sneek past the Goblin.",
            "outside the tavern",
            "visible_npcs=Dewr",
        )
        .await;
        assert_eq!(p.steps[0].action, "go");
    }

    #[tokio::test]
    async fn let_me_in_is_not_leave_ambush() {
        let llm = FakeLlm::new([
            r#"{"reason":"player wants to leave","kind":"action","action":"leave","confidence":5}"#,
        ]);
        let p = interpret_player_plan(
            &llm,
            "Groll? Why are you refusing to let me in?",
            "outside the tavern",
            "visible_npcs=Dewr",
        )
        .await;
        assert_ne!(p.steps[0].action, "leave", "got {:?}", p.steps[0]);
        assert_eq!(p.steps[0].action, "go");
    }

    #[test]
    fn wants_leave_is_strict() {
        assert!(wants_leave_command("leave"));
        assert!(wants_leave_command("I'm leaving the tavern"));
        assert!(!wants_leave_command(
            "Groll? Why are you refusing to let me in?"
        ));
        assert!(!wants_leave_command("leave me alone"));
    }

    #[test]
    fn turn_kind_heuristic_splits() {
        assert_eq!(
            classify_turn_kind_heuristic("I intimidate the goblin", "outside"),
            TurnKind::Check
        );
        assert_eq!(
            classify_turn_kind_heuristic("hello there little goblin", "outside"),
            TurnKind::Chat
        );
        assert_eq!(
            classify_turn_kind_heuristic("enter the tavern", "outside"),
            TurnKind::Action
        );
    }

    #[test]
    fn normalize_look_around_alias() {
        assert_eq!(normalize_alias("Look around."), "look around");
        assert_eq!(normalize_alias("look about!"), "look about");
    }

    #[tokio::test]
    async fn look_around_shortcut_skips_llm() {
        // Empty fake would fail if the reasoner ran.
        let llm = FakeLlm::new(Vec::<String>::new());
        let p = interpret_player_plan(&llm, "Look around.", "outside", "").await;
        assert_eq!(p.steps[0].action, "look");
    }
}
