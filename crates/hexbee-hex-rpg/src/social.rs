//! Rules-first social skill detection on the Talk path.
//!
//! The Purpose LLM classifies speech vs action; this module decides whether a line
//! is pure banter (no roll) or a contested social beat (d20 + skill), from text
//! and who is being addressed — not from LLM `social_act`.

use crate::checks::CheckKind;
use crate::scene::TalkFocus;
use crate::state::WorldState;

/// True when the line is color, small talk, or self-description — no skill roll.
pub fn is_pure_banter(input: &str) -> bool {
    let lower = crate::slang::normalize(input);
    if wants_social_outcome(&lower) {
        return false;
    }
    if is_self_description(&lower) {
        return true;
    }
    if is_greeting_only(&lower) {
        return true;
    }
    if is_casual_question(&lower) {
        return true;
    }
    false
}

/// Legacy helper — prefer [`crate::secrets::SecretsCatalog::bind_talk`].
pub fn detect_talk_check(
    input: &str,
    focus: &TalkFocus,
    world: &WorldState,
) -> Option<CheckKind> {
    let cat = crate::secrets::SecretsCatalog::bundled();
    match cat.bind_talk(input, focus, world) {
        Ok(crate::secrets::TalkBind::Tagged(id)) => cat
            .secret(&id)
            .and_then(|s| crate::secrets::SecretsCatalog::check_to_kind(&s.check)),
        _ => None,
    }
}

/// True when the line is trying to change something socially (may bind or go off-book).
pub fn wants_social_outcome(input: &str) -> bool {
    let lower = crate::slang::normalize(input);
    lower.contains("persuade")
        || lower.contains("convince")
        || lower.contains("talk into")
        || lower.contains("intimidate")
        || lower.contains("threaten")
        || lower.contains("charm")
        || lower.contains("flirt")
        || lower.contains("impress")
        || lower.contains("creep out")
        || lower.contains("password")
        || lower.contains("secret")
        || lower.contains("back room")
        || lower.contains("hidden door")
        || lower.contains("let me in")
        || lower.contains("let me pass")
        || lower.contains("step aside")
        || lower.contains("move aside")
        || lower.contains("or else")
        || lower.contains("won't pay")
        || lower.contains("wont pay")
        || lower.contains("not pay")
        || lower.contains("don't pay")
        || lower.contains("refuse to pay")
        || lower.contains("call out")
        || lower.contains("scam")
        || lower.contains("bluff")
        || lower.contains("help me get")
        || lower.contains("give me the word")
        || lower.contains("tell me the word")
        || lower.contains("share the word")
        || lower.contains("need the word")
        || lower.contains("any chance you'd")
        || lower.contains("please let")
        || (lower.contains("draw") && lower.contains("knife"))
        || (lower.contains("ask") && (lower.contains("password") || lower.contains("word") || lower.contains("secret")))
        || (lower.contains("tell me") && (lower.contains("password") || lower.contains("word") || lower.contains("secret")))
}

/// Social push with no authored secret — generic move bag (compliments, insults, etc.).
pub fn is_off_book_social(lower: &str) -> bool {
    lower.contains("compliment")
        || lower.contains("your hat")
        || lower.contains("awful hat")
        || lower.contains("nice hat")
        || lower.contains("ugly hat")
        || lower.contains("insult")
        || lower.contains("ridiculous")
        || lower.contains("admire")
        || lower.contains("mock")
}

fn is_self_description(lower: &str) -> bool {
    lower.contains("my pants")
        || lower.contains("my shirt")
        || lower.contains("my cloak")
        || lower.contains("my boots")
        || lower.contains("my hair")
        || lower.contains("my name is")
        || lower.contains("i am wearing")
        || lower.contains("i'm wearing")
        || lower.contains("i wear")
        || lower.starts_with("i am ")
        || lower.starts_with("i'm ")
}

fn is_greeting_only(lower: &str) -> bool {
    matches!(
        lower.trim(),
        "hello" | "hi" | "hey" | "howdy" | "greetings" | "good evening" | "good day"
    ) || (lower.starts_with("hello ")
        || lower.starts_with("hi ")
        || lower.starts_with("hey "))
        && !lower.contains('?')
        && !wants_social_outcome(lower)
}

fn is_casual_question(lower: &str) -> bool {
    (lower.starts_with("how are you")
        || lower.starts_with("what do you think")
        || lower.starts_with("nice weather")
        || lower.starts_with("cold night")
        || lower.starts_with("busy night"))
        && !groll_password_attempt(lower)
        && !charm_attempt(lower)
        && !intimidation_attempt(lower)
}

fn groll_password_attempt(lower: &str) -> bool {
    lower.contains("password")
        || lower.contains("secret word")
        || lower.contains("the word")
        || lower.contains("back room")
        || lower.contains("hidden door")
        || lower.contains("wall seam")
        || (lower.contains("groll") && lower.contains("word"))
        || (lower.contains("cloak") && lower.contains("word"))
        || lower.contains("persuade")
        || lower.contains("convince")
        || lower.contains("talk into")
        || (lower.contains("tell me") && lower.contains("word"))
}

fn charm_attempt(lower: &str) -> bool {
    lower.contains("charm")
        || lower.contains("flirt")
        || lower.contains("beautiful")
        || lower.contains("pretty")
        || lower.contains("handsome")
        || lower.contains("lovely")
        || lower.contains("date")
        || lower.contains("kiss")
        || lower.contains("your eyes")
}

fn intimidation_attempt(lower: &str) -> bool {
    lower.contains("intimidate")
        || lower.contains("threaten")
        || lower.contains("step aside")
        || lower.contains("move aside")
        || lower.contains("unless you move")
        || lower.contains("or else")
        || lower.contains("creep out")
        || lower.contains("back down")
        || (lower.contains("draw") && lower.contains("knife"))
        || (lower.contains("knife") && crate::slang::mentions_npc(lower, "goblin"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::TalkFocus;
    use crate::state::NpcId;

    #[test]
    fn blue_pants_is_banter() {
        assert!(is_pure_banter("say to groll: my pants are blue"));
        assert!(!wants_social_outcome("my pants are blue"));
    }

    #[test]
    fn natural_password_ask_is_check() {
        let world = WorldState::default();
        let focus = TalkFocus::Quest(NpcId::Groll);
        let line = "say to groll: surely you know a word for the hidden door?";
        assert!(!is_pure_banter(line));
        assert_eq!(
            detect_talk_check(line, &focus, &world),
            Some(CheckKind::PersuadeGrollPassword)
        );
    }

    #[test]
    fn hello_is_banter_not_check() {
        let world = WorldState::default();
        let focus = TalkFocus::Quest(NpcId::Groll);
        assert!(is_pure_banter("hello groll"));
        assert_eq!(detect_talk_check("hello groll", &focus, &world), None);
    }

    #[test]
    fn intimidate_goblin_is_check() {
        let world = WorldState::default();
        let focus = TalkFocus::Quest(NpcId::Dewr);
        let line = "I draw my knife and tell the goblin to step aside";
        assert_eq!(
            detect_talk_check(line, &focus, &world),
            Some(CheckKind::ChallengeDewr)
        );
    }
}
