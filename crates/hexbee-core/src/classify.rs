use crate::metrics;
use crate::Intent;
use tracing::{debug, info};

/// Deterministic phrase-based intent classifier for v1.
pub fn classify_intent(input: &str) -> Intent {
    let lower = input.trim().to_lowercase();
    let compact: String = lower.chars().filter(|c| !c.is_whitespace()).collect();

    let intent = if is_play_rpg(&lower, &compact) {
        Intent::PlayRpg
    } else if looks_like_settings(&lower) {
        Intent::Settings
    } else if looks_like_command(&lower) {
        Intent::Command
    } else {
        Intent::Chat
    };

    match intent {
        Intent::PlayRpg => metrics::inc_intent_play_rpg(),
        Intent::Command => metrics::inc_intent_command(),
        Intent::Settings => metrics::inc_intent_settings(),
        Intent::Chat => metrics::inc_intent_chat(),
    }
    info!(?intent, input = %input.trim(), "intent classified");
    debug!(%compact, "intent compact form");
    intent
}

fn is_play_rpg(lower: &str, compact: &str) -> bool {
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
        "start the rpg",
        "start tavern",
        "start game",
    ];
    // Typos / short forms seen in logs.
    if matches!(lower, "play" | "play gmae" | "play gaem" | "playgame") {
        return true;
    }
    PHRASES.iter().any(|p| lower.contains(p))
        || compact.contains("playtavern")
        || compact.contains("playanrpg")
        || compact.contains("letsplayanrpg")
        || compact.contains("playgame")
}

pub fn looks_like_games_inquiry(input: &str) -> bool {
    let lower = input.trim().to_lowercase();
    (lower.contains("what game") || lower.contains("which game") || lower.contains("games do"))
        || (lower.contains("what can you play") || lower.contains("games can you"))
        || lower == "games"
}

fn looks_like_settings(lower: &str) -> bool {
    lower.starts_with("set ")
        || lower.starts_with("settings")
        || lower.contains("change setting")
        || lower.contains("modify setting")
        || lower.starts_with("config ")
}

fn looks_like_command(lower: &str) -> bool {
    lower.starts_with("run ")
        || lower.starts_with("exec ")
        || lower.starts_with("bash ")
        || lower.starts_with("shell ")
        || lower.starts_with("sudo ")
        || lower.starts_with('$')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_tavern_phrases() {
        assert_eq!(classify_intent("Play Tavern"), Intent::PlayRpg);
        assert_eq!(classify_intent("Let's play an RPG"), Intent::PlayRpg);
        assert_eq!(classify_intent("play an rpg please"), Intent::PlayRpg);
        assert_eq!(classify_intent("Play game"), Intent::PlayRpg);
        assert_eq!(classify_intent("Play gmae"), Intent::PlayRpg);
        assert_eq!(classify_intent("Play"), Intent::PlayRpg);
    }

    #[test]
    fn games_inquiry() {
        assert!(looks_like_games_inquiry("What games do you have?"));
        assert!(looks_like_games_inquiry("What games can you play?"));
        assert!(!looks_like_games_inquiry("Play Tavern"));
    }

    #[test]
    fn command_and_settings() {
        assert_eq!(classify_intent("run ls -la"), Intent::Command);
        assert_eq!(classify_intent("set model foo"), Intent::Settings);
        assert_eq!(classify_intent("hello there"), Intent::Chat);
    }
}
