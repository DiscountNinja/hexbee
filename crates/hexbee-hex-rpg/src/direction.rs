//! What a line is asking the program to do.
//!
//! A reasoner (or the offline heuristic) returns a verb, a kind, and a subject.
//! An action also names an ability. The program binds that subject inside one
//! pile and does not borrow a match from another.

use hexbee_provider::{complete_json_with_retry, LlmClient};
use serde::Deserialize;

use crate::bind::{interactables_for_location, look_scene_line};
use crate::cards::card_for;
use crate::checks::{format_ability_report, resolve_ability_check};
use crate::game::{GameEvent, TavernGame};
use crate::organs::format_narrator_line;
use crate::scene::SceneCoordinator;
use crate::skills::{Ability, Skill};
use crate::state::NpcId;
use crate::world_doc::{is_room_survey_line, SoftFlag, SoftKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Ask,
    Look,
    Talk,
    Act,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Character,
    Creature,
    Object,
    Place,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Direction {
    pub verb: Verb,
    pub kind: Kind,
    pub subject: String,
    pub ability: Option<Ability>,
}

/// Handle a line when it belongs to one pile. Room-wide looks stay on the old path.
pub async fn try_specific(
    game: &mut TavernGame,
    input: &str,
    llm: Option<&dyn LlmClient>,
) -> Option<Vec<GameEvent>> {
    let normalized = crate::slang::normalize(input);
    if is_room_survey_line(&normalized) && !names_specific_subject(&normalized) {
        return None;
    }
    if is_self_or_price(&normalized) {
        return None;
    }
    if authored_path(&normalized) {
        return None;
    }
    let direction = classify(input, &normalized, llm).await?;
    match direction.verb {
        Verb::Ask | Verb::Look => Some(resolve_perceive(game, &direction).await),
        Verb::Talk if direction.kind != Kind::Character => {
            Some(resolve_talk(game, &direction, input, llm).await)
        }
        Verb::Act => Some(resolve_act(game, &direction, &normalized).await),
        Verb::Talk => None,
    }
}

async fn classify(
    raw: &str,
    normalized: &str,
    llm: Option<&dyn LlmClient>,
) -> Option<Direction> {
    if let Some(llm) = llm {
        if let Some(dir) = ask_model(llm, raw).await {
            return Some(dir);
        }
    }
    heuristic(normalized)
}

async fn ask_model(llm: &dyn LlmClient, raw: &str) -> Option<Direction> {
    let system = "Classify one TTRPG player line. Return ONLY JSON:\n\
{\"direction\":\"ask|look|talk|act\",\"kind\":\"character|creature|object|place\",\
\"subject\":\"short noun\",\"ability\":\"str|dex|con|int|wis|cha\"}\n\
direction: ask=a question, look=inspect, talk=address something, act=try to change it.\n\
kind is what the subject is. A cat is a creature. A person is a character. A stool is an object. A room is a place.\n\
Normalize plurals (kitties → cat). ability is required only for act.\n\
Do not decide success, DC, loot, or plot.";
    let user = format!("Player: {raw}");
    complete_json_with_retry(llm, system, &user, 2, parse_direction).await
}

#[derive(Deserialize)]
struct DirectionJson {
    direction: String,
    kind: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    ability: String,
}

fn parse_direction(raw: &str) -> Option<Direction> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    let parsed: DirectionJson = serde_json::from_str(&raw[start..=end]).ok()?;
    let verb = match parsed.direction.trim().to_lowercase().as_str() {
        "ask" => Verb::Ask,
        "look" => Verb::Look,
        "talk" => Verb::Talk,
        "act" | "action" => Verb::Act,
        _ => return None,
    };
    let kind = match parsed.kind.trim().to_lowercase().as_str() {
        "character" | "person" | "npc" => Kind::Character,
        "creature" | "animal" | "monster" => Kind::Creature,
        "object" | "item" | "thing" => Kind::Object,
        "place" | "room" | "location" => Kind::Place,
        _ => return None,
    };
    let subject = canonical_subject(&parsed.subject);
    let ability = if verb == Verb::Act {
        Ability::parse(&parsed.ability).or_else(|| ability_from_line(&subject))
    } else {
        None
    };
    Some(Direction {
        verb,
        kind,
        subject,
        ability,
    })
}

fn heuristic(lower: &str) -> Option<Direction> {
    let kind = kind_of(lower);
    let subject = subject_of(lower, kind);
    let verb = if ability_from_line(lower).is_some()
        && !lower.starts_with("are there")
        && !lower.starts_with("is there")
        && !lower.contains("talk to")
        && !lower.contains("speak to")
    {
        Verb::Act
    } else if lower.contains("talk to")
        || lower.contains("speak to")
        || lower.contains("speak with")
        || lower.starts_with("say to")
    {
        Verb::Talk
    } else if lower.starts_with("look")
        || lower.starts_with("examine")
        || lower.starts_with("inspect")
        || lower.starts_with("watch")
    {
        Verb::Look
    } else if lower.starts_with("are there")
        || lower.starts_with("is there")
        || lower.starts_with("any ")
        || lower.starts_with("who ")
        || lower.contains("anyone")
        || lower.contains("anybody")
        || lower.contains('?')
    {
        Verb::Ask
    } else {
        return None;
    };
    if verb == Verb::Look && is_room_survey_line(lower) && !names_specific_subject(lower) {
        return None;
    }
    let ability = if verb == Verb::Act {
        ability_from_line(lower)
    } else {
        None
    };
    if verb == Verb::Act && ability.is_none() {
        return None;
    }
    Some(Direction {
        verb,
        kind,
        subject,
        ability,
    })
}

fn kind_of(lower: &str) -> Kind {
    if creature_word(lower) {
        return Kind::Creature;
    }
    if !crate::rules::extract_npc_targets(lower).is_empty()
        || lower.contains("anyone")
        || lower.contains("anybody")
        || lower.contains("who ")
    {
        return Kind::Character;
    }
    if object_word(lower) {
        return Kind::Object;
    }
    if lower.contains("room")
        || lower.contains("hall")
        || lower.contains("place")
        || lower.contains("tavern")
        || is_room_survey_line(lower)
    {
        return Kind::Place;
    }
    if lower.contains("talk to") || lower.contains("speak to") || lower.contains("say to") {
        return Kind::Character;
    }
    Kind::Object
}

fn object_word(lower: &str) -> bool {
    [
        "stool", "door", "mug", "hearth", "fire", "table", "bar", "amulet", "sword", "coin",
        "gold", "key", "chest", "barrel", "sign",
    ]
    .iter()
    .any(|w| {
        lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|t| t == *w)
    })
}

/// Authored commands keep their own skill, talk, and movement paths.
fn authored_path(normalized: &str) -> bool {
    use crate::rules::{match_rules_normalized, TurnMode};
    let Some(hit) = match_rules_normalized(normalized) else {
        return false;
    };
    if hit.mode == TurnMode::Query {
        return false;
    }
    if hit.action_hint.as_deref() == Some("look") && names_specific_subject(normalized) {
        return false;
    }
    if hit.mode == TurnMode::Talk {
        return kind_of(normalized) == Kind::Character;
    }
    hit.action_hint.is_some()
}

fn subject_of(lower: &str, kind: Kind) -> String {
    if let Some(name) = creature_subject(lower) {
        return name;
    }
    if kind == Kind::Character {
        if let Some(id) = NpcId::parse(lower) {
            return id.slug().into();
        }
        if lower.contains("anyone") || lower.contains("anybody") || lower.contains("who ") {
            return "anyone".into();
        }
    }
    let skip = [
        "are", "there", "any", "the", "this", "that", "around", "here", "what", "who", "look",
        "examine", "inspect", "talk", "speak", "with", "about", "please", "some", "does", "have",
        "shove", "push", "force", "break", "lift", "climb", "grab", "open", "pry", "smash",
    ];
    lower
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| w.len() >= 3 && !skip.contains(w))
        .unwrap_or(if kind == Kind::Place { "room" } else { "" })
        .to_string()
}

fn canonical_subject(raw: &str) -> String {
    let lower = raw.trim().to_lowercase();
    if let Some(name) = creature_subject(&lower) {
        return name;
    }
    lower
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| w.len() >= 3)
        .unwrap_or("")
        .to_string()
}

fn names_specific_subject(lower: &str) -> bool {
    creature_word(lower) || NpcId::parse(lower).is_some()
}

fn is_self_or_price(lower: &str) -> bool {
    crate::prices::match_price_query(lower).is_some()
        || lower.contains("inventory")
        || lower == "sheet"
        || lower == "status"
        || lower == "skills"
        || lower.contains("do i have")
}

fn creature_word(lower: &str) -> bool {
    tokens(lower).into_iter().any(|w| {
        is_cat(&w) || is_dog(&w) || is_generic_creature(&w)
    })
}

fn creature_subject(lower: &str) -> Option<String> {
    let words = tokens(lower);
    if words.iter().any(|w| is_cat(w)) {
        return Some("cat".into());
    }
    if words.iter().any(|w| is_dog(w)) {
        return Some("dog".into());
    }
    if words.iter().any(|w| is_generic_creature(w)) {
        return Some("animal".into());
    }
    None
}

fn tokens(lower: &str) -> Vec<String> {
    lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

fn is_cat(w: &str) -> bool {
    w == "cat" || w == "cats" || w.starts_with("kitt") || w == "feline"
}

fn is_dog(w: &str) -> bool {
    w == "dog" || w == "dogs" || w == "puppy" || w == "puppies" || w == "hound"
}

fn is_generic_creature(w: &str) -> bool {
    matches!(
        w,
        "animal" | "animals" | "pet" | "pets" | "monster" | "monsters" | "beast" | "beasts" | "creature" | "creatures"
    )
}

fn ability_from_line(lower: &str) -> Option<Ability> {
    if contains_any(lower, &["shove", "push", "force", "break", "lift", "smash", "pry", "wrestle", "strength"])
    {
        return Some(Ability::Str);
    }
    if contains_any(lower, &["sneak", "climb", "dodge", "slip", "grab", "palm", "dexterity"]) {
        return Some(Ability::Dex);
    }
    if contains_any(lower, &["endure", "chug", "hold on", "resist", "constitution"]) {
        return Some(Ability::Con);
    }
    if contains_any(lower, &["recall", "decipher", "study", "figure", "intelligence"]) {
        return Some(Ability::Int);
    }
    if contains_any(lower, &["notice", "spot", "soothe", "calm", "pet", "sense", "wisdom"]) {
        return Some(Ability::Wis);
    }
    if contains_any(lower, &["intimidate", "persuade", "charm", "flirt", "deceive", "lie", "charisma"])
    {
        return Some(Ability::Cha);
    }
    None
}

fn contains_any(lower: &str, words: &[&str]) -> bool {
    words.iter().any(|w| lower.contains(w))
}

fn harmless(lower: &str) -> bool {
    contains_any(lower, &["pet", "touch", "pat", "stroke", "scratch", "nuzzle"])
        && ability_from_line(lower) == Some(Ability::Wis)
        && !contains_any(lower, &["shove", "force", "break", "smash", "pry"])
}

async fn resolve_perceive(game: &mut TavernGame, direction: &Direction) -> Vec<GameEvent> {
    let text = match direction.kind {
        Kind::Character => character_answer(game, direction),
        Kind::Creature => creature_answer(game, direction),
        Kind::Object => object_answer(game, direction),
        Kind::Place => look_scene_line(&game.world, &game.scene),
    };
    vec![GameEvent::Narration(text)]
}

fn character_answer(game: &TavernGame, direction: &Direction) -> String {
    let visible = SceneCoordinator::visible_npcs(&game.world);
    if direction.subject.is_empty() || direction.subject == "anyone" {
        if visible.is_empty() {
            return "No one notable here.".into();
        }
        let names = visible
            .iter()
            .map(|id| id.display_name())
            .collect::<Vec<_>>()
            .join(", ");
        return format!("Yes — {names}.");
    }
    let Some(id) = NpcId::parse(&direction.subject) else {
        return format!("No one called {} is here.", direction.subject);
    };
    if !visible.contains(&id) {
        return format!("{} isn't here.", id.display_name());
    }
    if direction.verb == Verb::Look {
        if let Some(card) = card_for(&game.cards, id) {
            return card.description_tag();
        }
    }
    format!("{} is here.", id.display_name())
}

fn creature_answer(game: &mut TavernGame, direction: &Direction) -> String {
    let found = find_flag(game, Kind::Creature, &direction.subject)
        .map(|flag| (flag.id.clone(), flag.text.clone()));
    if let Some((id, text)) = found {
        game.world_doc.discover(&mut game.world, &id);
        if direction.verb == Verb::Look {
            return format!("You look closer. {text}");
        }
        return format!("Yes — {}.", soften(&text));
    }
    let label = if direction.subject.is_empty() {
        "animals".into()
    } else {
        direction.subject.clone()
    };
    format!("No {label} here.")
}

fn object_answer(game: &mut TavernGame, direction: &Direction) -> String {
    let found = find_flag(game, Kind::Object, &direction.subject)
        .map(|flag| (flag.id.clone(), flag.text.clone()));
    if let Some((id, text)) = found {
        game.world_doc.discover(&mut game.world, &id);
        if direction.verb == Verb::Look {
            return format!("You look closer. {text}");
        }
        return format!("Yes — {}.", soften(&text));
    }
    if let Some(label) = interactables_for_location(&game.world)
        .into_iter()
        .find(|label| label.to_lowercase().contains(&direction.subject) && !direction.subject.is_empty())
    {
        return format!("Yes — {label}.");
    }
    if direction.subject.is_empty() {
        return "Nothing by that name is here.".into();
    }
    format!("No {} here.", direction.subject)
}

fn find_flag<'a>(game: &'a TavernGame, kind: Kind, subject: &str) -> Option<&'a SoftFlag> {
    game.world_doc
        .at_location(game.world.location)
        .filter(|flag| flag_matches(flag, kind, subject))
        .max_by_key(|flag| flag.text.len())
}

fn flag_matches(flag: &SoftFlag, kind: Kind, subject: &str) -> bool {
    let text = flag.text.to_lowercase();
    let name = flag.name.clone().unwrap_or_default().to_lowercase();
    let blob = format!("{text} {name}");
    match kind {
        Kind::Creature => {
            let animal = flag.kind == Some(SoftKind::Animal) || creature_word(&blob);
            if !animal {
                return false;
            }
            subject.is_empty()
                || subject == "animal"
                || subject == "creature"
                || creature_subject(&blob).as_deref() == Some(subject)
        }
        Kind::Object => {
            let objectish = flag.kind == Some(SoftKind::Object) || flag.kind.is_none();
            objectish && !subject.is_empty() && blob.contains(subject) && !creature_word(&blob)
        }
        Kind::Character => {
            flag.kind == Some(SoftKind::Person)
                && !subject.is_empty()
                && blob.contains(subject)
        }
        Kind::Place => false,
    }
}

fn soften(text: &str) -> String {
    let lower = text.trim().to_lowercase();
    if let Some(rest) = lower.strip_prefix("there's a ") {
        return rest.to_string();
    }
    if let Some(rest) = lower.strip_prefix("there is a ") {
        return rest.to_string();
    }
    text.trim().to_string()
}

async fn resolve_talk(
    game: &mut TavernGame,
    direction: &Direction,
    input: &str,
    llm: Option<&dyn LlmClient>,
) -> Vec<GameEvent> {
    let _ = (input, llm);
    match direction.kind {
        Kind::Place => vec![GameEvent::Narration(
            "The room doesn't answer.".into(),
        )],
        Kind::Object => talk_unvoiced(game, direction, "It stays silent."),
        Kind::Creature => talk_unvoiced(
            game,
            direction,
            "It stirs, but it doesn't speak.",
        ),
        Kind::Character => vec![GameEvent::Narration("There's no one by that name to talk to.".into())],
    }
}

fn talk_unvoiced(game: &mut TavernGame, direction: &Direction, silent: &str) -> Vec<GameEvent> {
    let found = find_flag(game, direction.kind, &direction.subject).map(|flag| {
        (
            flag.id.clone(),
            flag.text.clone(),
            flag.voice.clone().filter(|v| !v.trim().is_empty()),
            flag.name
                .clone()
                .unwrap_or_else(|| direction.subject.clone()),
        )
    });
    if let Some((id, text, voice, name)) = found {
        game.world_doc.discover(&mut game.world, &id);
        if let Some(voice) = voice {
            return vec![GameEvent::Narration(format!(
                "💬 {name}: *{voice}* \"I hear you.\""
            ))];
        }
        return vec![GameEvent::Narration(format_narrator_line(&format!(
            "{text} {silent}"
        )))];
    }
    let label = if direction.subject.is_empty() {
        "That".into()
    } else {
        direction.subject.clone()
    };
    vec![GameEvent::Narration(format!("{label} isn't here to answer."))]
}

async fn resolve_act(game: &mut TavernGame, direction: &Direction, lower: &str) -> Vec<GameEvent> {
    let Some(ability) = direction.ability.or_else(|| ability_from_line(lower)) else {
        return vec![GameEvent::Narration(
            "That action needs a strength, dexterity, constitution, intelligence, wisdom, or charisma attempt.".into(),
        )];
    };
    let subject = if direction.subject.is_empty() {
        "it".into()
    } else {
        direction.subject.clone()
    };
    if harmless(lower) {
        let found = find_flag(game, direction.kind, &direction.subject)
            .map(|flag| (flag.id.clone(), flag.text.clone()));
        if let Some((id, text)) = found {
            game.world_doc.discover(&mut game.world, &id);
            return vec![GameEvent::Narration(format_narrator_line(&format!(
                "You manage it without a struggle. {text}"
            )))];
        }
        return vec![GameEvent::Narration(format_narrator_line(&format!(
            "You manage the {subject} without a struggle."
        )))];
    }
    let dc = find_flag(game, direction.kind, &direction.subject)
        .and_then(|f| f.stake.as_ref())
        .and_then(|s| s.dc)
        .unwrap_or(12)
        .to_owned();
    let skill = skill_for(ability, lower);
    let check = resolve_ability_check(ability, skill, &game.world.player.build, dc, &mut game.rng);
    let report = format_ability_report(&check);
    let beat = if check.success {
        format!("The {subject} gives way.")
    } else {
        format!("The {subject} holds.")
    };
    if check.success {
        if let Some(id) =
            find_flag(game, direction.kind, &direction.subject).map(|flag| flag.id.clone())
        {
            game.world_doc.discover(&mut game.world, &id);
        }
    }
    vec![
        GameEvent::Narration(report),
        GameEvent::Narration(format_narrator_line(&beat)),
    ]
}

fn skill_for(ability: Ability, lower: &str) -> Option<Skill> {
    match ability {
        Ability::Str => Some(Skill::Athletics),
        Ability::Dex => {
            if lower.contains("sneak") || lower.contains("slip") || lower.contains("hide") {
                Some(Skill::Stealth)
            } else if lower.contains("grab") || lower.contains("palm") || lower.contains("pick") {
                Some(Skill::SleightOfHand)
            } else {
                Some(Skill::Acrobatics)
            }
        }
        Ability::Con => None,
        Ability::Int => {
            if lower.contains("recall") || lower.contains("history") {
                Some(Skill::History)
            } else {
                Some(Skill::Investigation)
            }
        }
        Ability::Wis => {
            if lower.contains("pet") || lower.contains("calm") || lower.contains("soothe") {
                Some(Skill::AnimalHandling)
            } else if lower.contains("notice") || lower.contains("spot") || lower.contains("search") {
                Some(Skill::Perception)
            } else {
                Some(Skill::Insight)
            }
        }
        Ability::Cha => {
            if lower.contains("intimidate") || lower.contains("threaten") {
                Some(Skill::Intimidation)
            } else if lower.contains("lie") || lower.contains("deceive") {
                Some(Skill::Deception)
            } else {
                Some(Skill::Persuasion)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitties_are_an_ask_about_a_cat() {
        let dir = heuristic("are there any kitties around").unwrap();
        assert_eq!(dir.verb, Verb::Ask);
        assert_eq!(dir.kind, Kind::Creature);
        assert_eq!(dir.subject, "cat");
    }

    #[test]
    fn shove_is_strength() {
        let dir = heuristic("i shove the stool").unwrap();
        assert_eq!(dir.verb, Verb::Act);
        assert_eq!(dir.kind, Kind::Object);
        assert_eq!(dir.ability, Some(Ability::Str));
    }

    #[test]
    fn constitution_has_no_skill() {
        assert!(skill_for(Ability::Con, "i endure the cold").is_none());
    }

    #[tokio::test]
    async fn kitties_answer_from_the_note_not_the_room() {
        use crate::game::TavernGame;
        use crate::state::Location;

        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let events = game
            .handle_line("Are there any kitties around?", None)
            .await;
        let text = events
            .iter()
            .filter_map(|e| match e {
                GameEvent::Narration(s) => Some(s.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.to_lowercase().contains("kitty"),
            "expected the fireplace note, got {text}"
        );
        assert!(!text.contains("Sally"), "characters stay out of a creature answer, got {text}");
        assert!(!text.contains("Ways out"), "room survey stays out, got {text}");
    }

    #[tokio::test]
    async fn shove_rolls_strength() {
        use crate::game::TavernGame;
        use crate::state::Location;

        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let events = game.handle_line("I shove the stool.", None).await;
        let text = events
            .iter()
            .filter_map(|e| match e {
                GameEvent::Narration(s) => Some(s.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("STR"), "expected a strength check, got {text}");
        assert!(text.contains("🎲"), "expected a roll, got {text}");
    }
}
