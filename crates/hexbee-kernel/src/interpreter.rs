use hexbee_core::metrics;
use tracing::info;

use crate::intent::{Intent, IntentKind};

/// Shell-level routing before hex session starts.
#[derive(Debug, Clone, PartialEq)]
pub enum ShellRoute {
    PlayHex(String),
    LoadCampaign(String),
    KernelCommand(Intent),
    Chat,
}

const KERNEL_COMMANDS: &[&str] = &[
    "help",
    "?",
    "options",
    "save",
    "load",
    "quit",
    "exit",
    "q",
    "reset",
    "restart",
    "actor",
    "next",
    "undo",
    "reprompt",
    "approve",
    "deny",
    "grants",
    "locks",
    "goal",
    "metrics",
    "stats",
    "dict",
];

/// Rule-first interpreter (design doc §9).
pub fn classify_line(input: &str, current_actor: &str, in_hex: bool) -> Intent {
    let trimmed = input.trim();
    let lower = trimmed.to_lowercase();

    if let Some(intent) = rule_kernel_command(trimmed, &lower, current_actor) {
        return intent;
    }

    if !in_hex {
        if let Some(route) = shell_route(trimmed, &lower) {
            return match route {
                ShellRoute::PlayHex(_) | ShellRoute::LoadCampaign(_) => {
                    Intent::command(trimmed, current_actor)
                }
                ShellRoute::KernelCommand(i) => i,
                ShellRoute::Chat => Intent {
                    kind: IntentKind::Ambiguous,
                    hex_target: None,
                    actor_id: current_actor.into(),
                    verb: None,
                    object_ids: vec![],
                    raw: trimmed.into(),
                    confidence: 0.5,
                    notes: Some("shell chat".into()),
                },
            };
        }
    }

    if looks_like_query(&lower) {
        return Intent {
            kind: IntentKind::Query,
            hex_target: None,
            actor_id: current_actor.into(),
            verb: Some("query".into()),
            object_ids: vec![],
            raw: trimmed.into(),
            confidence: 0.8,
            notes: None,
        };
    }

    if looks_like_attack(&lower) {
        return Intent {
            kind: IntentKind::Attack,
            hex_target: None,
            actor_id: current_actor.into(),
            verb: Some("attack".into()),
            object_ids: extract_targets(&lower),
            raw: trimmed.into(),
            confidence: 0.7,
            notes: None,
        };
    }

    if looks_like_speech(&lower) {
        return Intent {
            kind: IntentKind::Speech,
            hex_target: None,
            actor_id: current_actor.into(),
            verb: Some("say".into()),
            object_ids: extract_targets(&lower),
            raw: trimmed.into(),
            confidence: 0.6,
            notes: None,
        };
    }

    Intent {
        kind: IntentKind::Action,
        hex_target: None,
        actor_id: current_actor.into(),
        verb: None,
        object_ids: vec![],
        raw: trimmed.into(),
        confidence: 0.5,
        notes: None,
    }
}

pub fn shell_route(input: &str, lower: &str) -> Option<ShellRoute> {
    if is_play_hex(lower) {
        return Some(ShellRoute::PlayHex("tavern".into()));
    }
    if lower.starts_with("load ") {
        let path = input[5..].trim();
        if !path.is_empty() {
            return Some(ShellRoute::LoadCampaign(path.into()));
        }
    }
    if let Some(intent) = rule_kernel_command(input, lower, "operator") {
        return Some(ShellRoute::KernelCommand(intent));
    }
    if looks_like_games_inquiry(lower) {
        return Some(ShellRoute::Chat);
    }
    None
}

fn rule_kernel_command(input: &str, lower: &str, actor: &str) -> Option<Intent> {
    let first = lower.split_whitespace().next()?;
    if KERNEL_COMMANDS.contains(&first) {
        metrics::inc_intent_command();
        info!(cmd = first, "kernel command detected");
        return Some(Intent::command(input, actor));
    }
    if lower == "new game" || lower.starts_with("new game ") {
        metrics::inc_intent_command();
        return Some(Intent::command(input, actor));
    }
    if lower.starts_with("actor ") {
        metrics::inc_intent_command();
        return Some(Intent::command(input, actor));
    }
    if lower.starts_with("approve ") || lower.starts_with("deny ") {
        metrics::inc_intent_command();
        return Some(Intent::command(input, actor));
    }
    None
}

fn is_play_hex(lower: &str) -> bool {
    const PHRASES: &[&str] = &[
        "play tavern",
        "play the tavern",
        "let's play an rpg",
        "lets play an rpg",
        "play an rpg",
        "play rpg",
        "play game",
        "play a game",
        "play the game",
        "start rpg",
        "start tavern",
        "start game",
    ];
    if matches!(lower, "play" | "play gmae" | "play gaem" | "playgame") {
        return true;
    }
    PHRASES.iter().any(|p| lower.contains(p))
}

pub fn looks_like_games_inquiry(lower: &str) -> bool {
    (lower.contains("what game") || lower.contains("which game") || lower.contains("games do"))
        || (lower.contains("what can you play") || lower.contains("games can you"))
        || lower == "games"
}

fn looks_like_query(lower: &str) -> bool {
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
        || lower.contains("do i have")
        || lower.contains("inventory")
}

fn looks_like_attack(lower: &str) -> bool {
    lower.contains("fight")
        || lower.contains("attack")
        || lower.contains("strike")
        || lower.contains("kill")
        || lower.contains("hit ")
}

fn looks_like_speech(lower: &str) -> bool {
    lower.starts_with("say ")
        || lower.starts_with("tell ")
        || lower.starts_with("ask ")
        || lower.contains("talk to")
        || lower.contains("speak to")
        || matches!(lower, "hello" | "hi" | "hey" | "talk")
        || lower.starts_with("what do you")
}

fn extract_targets(lower: &str) -> Vec<String> {
    for name in ["dewr", "sally", "groll", "dremis", "goblin", "patron"] {
        if lower.contains(name) {
            return vec![name.into()];
        }
    }
    vec![]
}

/// LLM interpreter organ for ambiguous lines (optional upgrade).
pub async fn interpret_with_llm(
    llm: &dyn hexbee_provider::LlmClient,
    input: &str,
    actor_id: &str,
) -> Intent {
    let system = "Classify the player line as JSON: {\"kind\":\"action|speech|attack|query|ambiguous\",\"confidence\":0.0-1.0,\"verb\":\"optional\",\"object_ids\":[]}";
    match llm.complete_json(system, input).await {
        Ok(json) => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) {
                let kind = v
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .map(parse_kind)
                    .unwrap_or(IntentKind::Ambiguous);
                return Intent {
                    kind,
                    hex_target: None,
                    actor_id: actor_id.into(),
                    verb: v
                        .get("verb")
                        .and_then(|x| x.as_str())
                        .map(String::from),
                    object_ids: v
                        .get("object_ids")
                        .and_then(|x| x.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    raw: input.into(),
                    confidence: v
                        .get("confidence")
                        .and_then(|x| x.as_f64())
                        .unwrap_or(0.5) as f32,
                    notes: None,
                };
            }
        }
        Err(e) => {
            tracing::warn!(%e, "interpreter organ failed");
        }
    }
    classify_line(input, actor_id, true)
}

fn parse_kind(s: &str) -> IntentKind {
    match s {
        "command" => IntentKind::Command,
        "query" => IntentKind::Query,
        "speech" => IntentKind::Speech,
        "action" => IntentKind::Action,
        "attack" => IntentKind::Attack,
        _ => IntentKind::Ambiguous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_during_play_is_command() {
        let intent = classify_line("options", "pc", true);
        assert_eq!(intent.kind, IntentKind::Command);
    }

    #[test]
    fn save_is_command() {
        let intent = classify_line("save", "pc", true);
        assert_eq!(intent.kind, IntentKind::Command);
    }

    #[test]
    fn help_is_command() {
        let intent = classify_line("help", "pc", true);
        assert_eq!(intent.kind, IntentKind::Command);
        let intent = classify_line("?", "pc", true);
        assert_eq!(intent.kind, IntentKind::Command);
    }

    #[test]
    fn what_do_you_know_is_speech_not_query() {
        let intent = classify_line("what do you know", "pc", true);
        assert_eq!(intent.kind, IntentKind::Speech);
    }

    #[test]
    fn inventory_is_query() {
        let intent = classify_line("inventory", "pc", true);
        assert_eq!(intent.kind, IntentKind::Query);
    }
}
