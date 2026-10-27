//! Table rounds: a spending action marks a seat. When every seat has gone,
//! one present NPC reacts, then the table starts again.

use hexbee_provider::complete_json_with_retry;
use hexbee_provider::LlmClient;
use serde::Deserialize;

use crate::cards::card_for;
use crate::game::TavernGame;
use crate::organs::{format_narrator_line, format_npc_display, parse_staged_line};
use crate::prices::match_price_query;
use crate::rules::{match_rules_normalized, TurnMode};
use crate::scene::{SceneCoordinator, TalkFocus};
use crate::state::NpcId;
use crate::worlds::SituationKnob;

#[derive(Debug, Clone)]
pub struct SeatFinish {
    /// The current actor is not a seat, so the table did not move.
    pub skipped: bool,
    pub next_actor: String,
    pub closed: bool,
    pub round_lines: Vec<String>,
}

#[derive(Debug, Clone)]
struct NpcBeat {
    line: String,
    attitude: i32,
    heat: i32,
}

struct RoundNpc {
    name: String,
    voice: String,
    slug: String,
    template: String,
}

/// Look, inventory, sheet, price, dict, and help do not spend a turn.
/// A pending clarify question does not either.
pub fn spends_turn(normalized: &str, pending_clarify: bool) -> bool {
    if pending_clarify {
        return false;
    }
    let lower = normalized.trim();
    if lower.is_empty() {
        return false;
    }
    const FREE: &[&str] = &[
        "look",
        "l",
        "inventory",
        "inv",
        "i",
        "sheet",
        "status",
        "skills",
        "help",
        "dict",
        "price",
    ];
    if FREE.contains(&lower) {
        return false;
    }
    if let Some(hit) = match_rules_normalized(lower) {
        if hit.mode == TurnMode::Query {
            return false;
        }
        if matches!(
            hit.action_hint.as_deref(),
            Some("look" | "inventory" | "dict" | "help" | "price" | "sheet" | "skills")
        ) {
            return false;
        }
    }
    match_price_query(lower).is_none()
}

pub fn seat_ids(campaign: &hexbee_store::Campaign) -> Vec<String> {
    let mut ids: Vec<_> = campaign
        .controllable_actors()
        .into_iter()
        .map(|a| a.id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Record that `actor_id` has gone. Close the round when every seat has.
pub fn finish_spent_seat(
    game: &mut TavernGame,
    actor_id: &str,
    summary: &str,
    seats: &[String],
    llm: Option<&dyn LlmClient>,
) -> SeatFinish {
    if seats.is_empty() || !seats.iter().any(|id| id == actor_id) {
        return SeatFinish {
            skipped: true,
            next_actor: actor_id.to_string(),
            closed: false,
            round_lines: Vec::new(),
        };
    }

    if !game.world.round_gone.iter().any(|id| id == actor_id) {
        game.world.round_gone.push(actor_id.to_string());
    }
    let summary: String = summary.chars().take(180).collect();
    if !summary.is_empty() {
        game.world.round_log.push(summary);
        if game.world.round_log.len() > 8 {
            let drop_n = game.world.round_log.len() - 8;
            game.world.round_log.drain(0..drop_n);
        }
    }

    if !all_gone(seats, &game.world.round_gone) {
        let next = next_pending(seats, actor_id, &game.world.round_gone)
            .unwrap_or_else(|| seats[0].clone());
        return SeatFinish {
            skipped: false,
            next_actor: next,
            closed: false,
            round_lines: Vec::new(),
        };
    }

    let log = game.world.round_log.clone();
    let round_lines = paint_npc_phase(game, seats, &log, llm);
    game.world.round_gone.clear();
    game.world.round_log.clear();
    game.world.situation.apply(SituationKnob::ClockBeat);
    SeatFinish {
        skipped: false,
        next_actor: seats[0].clone(),
        closed: true,
        round_lines,
    }
}

fn all_gone(seats: &[String], gone: &[String]) -> bool {
    seats.iter().all(|id| gone.iter().any(|g| g == id))
}

fn next_pending(seats: &[String], current: &str, gone: &[String]) -> Option<String> {
    let start = seats.iter().position(|id| id == current).unwrap_or(0);
    for step in 1..=seats.len() {
        let id = &seats[(start + step) % seats.len()];
        if !gone.iter().any(|g| g == id) {
            return Some(id.clone());
        }
    }
    None
}

fn paint_npc_phase(
    game: &mut TavernGame,
    seats: &[String],
    log: &[String],
    llm: Option<&dyn LlmClient>,
) -> Vec<String> {
    let mut lines = vec!["— round —".to_string()];
    let Some(npc) = pick_round_npc(game, seats) else {
        lines.push(format_narrator_line("The room holds for a breath."));
        return lines;
    };

    let beat = llm.and_then(|llm| poll_npc(llm, &npc, game.world.location.display_name(), log));
    let (spoken, attitude, heat) = match beat {
        Some(beat) => (beat.line, beat.attitude, beat.heat),
        None => (npc.template.clone(), 0, 0),
    };
    if attitude != 0 {
        game.world.situation.apply(SituationKnob::Attitude {
            npc: npc.slug.clone(),
            delta: attitude,
        });
    }
    if heat != 0 {
        game.world.situation.apply(SituationKnob::SceneHeatDelta(heat));
    }
    let staged = parse_staged_line(&spoken);
    lines.push(format_npc_display(&npc.name, &staged));
    lines
}

fn pick_round_npc(game: &TavernGame, seats: &[String]) -> Option<RoundNpc> {
    let visible = SceneCoordinator::visible_npcs(&game.world);
    if let Some(focus) = game.scene.last_focus() {
        if let Some(npc) = focus_to_npc(game, &focus, seats, &visible) {
            return Some(npc);
        }
    }
    if let Some(id) = SceneCoordinator::spotlight_npc(&game.world) {
        if visible.contains(&id) && !is_seated(id, seats) {
            return Some(quest_npc(game, id));
        }
    }
    for id in visible {
        if !is_seated(id, seats) {
            return Some(quest_npc(game, id));
        }
    }
    game.scene
        .ambient_here(game.world.location)
        .into_iter()
        .find(|a| !seats.iter().any(|s| s == &a.id))
        .map(|a| RoundNpc {
            name: a.card.name.clone(),
            voice: a.card.voice_tag(),
            slug: a.id.clone(),
            template: "*glances over* \"Evening.\"".into(),
        })
}

fn focus_to_npc(
    game: &TavernGame,
    focus: &TalkFocus,
    seats: &[String],
    visible: &[NpcId],
) -> Option<RoundNpc> {
    match focus {
        TalkFocus::Quest(id) => {
            if visible.contains(id) && !is_seated(*id, seats) {
                Some(quest_npc(game, *id))
            } else {
                None
            }
        }
        TalkFocus::Ambient(raw) => game
            .scene
            .ambient_here(game.world.location)
            .into_iter()
            .find(|a| a.id == *raw && !seats.iter().any(|s| s == &a.id))
            .map(|a| RoundNpc {
                name: a.card.name.clone(),
                voice: a.card.voice_tag(),
                slug: a.id.clone(),
                template: "*glances over* \"Evening.\"".into(),
            }),
    }
}

fn quest_npc(game: &TavernGame, id: NpcId) -> RoundNpc {
    let voice = card_for(&game.cards, id)
        .map(|c| c.voice_tag())
        .unwrap_or_else(|| id.display_name().into());
    RoundNpc {
        name: id.display_name().into(),
        voice,
        slug: id.slug().into(),
        template: template_for(id).into(),
    }
}

fn is_seated(id: NpcId, seats: &[String]) -> bool {
    seats.iter().any(|s| s.eq_ignore_ascii_case(id.slug()))
}

fn template_for(id: NpcId) -> &'static str {
    match id {
        NpcId::Dewr => "*leans in the doorway* \"Don't get comfortable.\"",
        NpcId::Sally => "*wipes the bar* \"Yell if you need something.\"",
        NpcId::Groll => "*the cloak stays still* \"Mm.\"",
        NpcId::Dremis => "*smoke shifts* \"I am still here.\"",
        NpcId::Patron => "*lifts a mug* \"Quiet, for a moment.\"",
        NpcId::Goblin => "*snickers* \"Heh.\"",
    }
}

fn poll_npc(llm: &dyn LlmClient, npc: &RoundNpc, place: &str, log: &[String]) -> Option<NpcBeat> {
    let handle = tokio::runtime::Handle::try_current().ok()?;
    let name = npc.name.clone();
    let voice = npc.voice.clone();
    let place = place.to_string();
    let log = log.to_vec();
    tokio::task::block_in_place(|| {
        handle.block_on(async move { ask_npc(llm, &name, &voice, &place, &log).await })
    })
}

async fn ask_npc(
    llm: &dyn LlmClient,
    name: &str,
    voice: &str,
    place: &str,
    log: &[String],
) -> Option<NpcBeat> {
    let system = format!(
        "You are {name} ({voice}) at {place}.\n\
The round just closed. React to what already happened, or take one small action of your own.\n\
You may only speak, and optionally shift attitude or heat.\n\
attitude and heat are integers from -1 to 1. Use 0 when you only speak.\n\
Do not give items, gold, a password, or an amulet. Do not move anyone. Do not finish a quest.\n\
Do not quote the log, the narrator, or a player id. Do not retell what they already did.\n\
A *gesture* and one or two spoken sentences is enough.\n\
Return ONLY JSON with keys line, attitude, heat."
    );
    let happened = if log.is_empty() {
        "(nothing committed)".to_string()
    } else {
        log.join("\n")
    };
    let user = format!("Committed this round:\n{happened}");
    let log = log.to_vec();
    complete_json_with_retry(llm, &system, &user, 2, move |text| {
        let beat = parse_beat(text)?;
        if repeats_committed(&beat.line, &log) {
            return None;
        }
        Some(beat)
    })
    .await
}

#[derive(Deserialize)]
struct BeatJson {
    line: String,
    #[serde(default)]
    attitude: i32,
    #[serde(default)]
    heat: i32,
}

fn parse_beat(raw: &str) -> Option<NpcBeat> {
    let json = json_object(raw)?;
    let beat: BeatJson = serde_json::from_str(json).ok()?;
    let line = beat.line.trim();
    if line.is_empty() || leaks_loot(line) {
        return None;
    }
    Some(NpcBeat {
        line: line.to_string(),
        attitude: beat.attitude.clamp(-1, 1),
        heat: beat.heat.clamp(-1, 1),
    })
}

fn json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(&raw[start..=end])
}

/// What the round may remember: the outcome, without feed icons or a raw seat id.
pub fn plain_round_fact(actor: &str, raw: &str, lines: &[String]) -> String {
    let beat = lines.iter().rev().find(|line| {
        let t = line.trim_start();
        t.starts_with('🪶')
            || t.starts_with('🎲')
            || t.starts_with('✅')
            || t.starts_with('❌')
    });
    let who = if actor.eq_ignore_ascii_case("pc") {
        "the player"
    } else {
        actor
    };
    let text = match beat {
        Some(line) => format!("{who}: {}", strip_feed_chrome(line)),
        None => format!("{who} tried: {}", strip_feed_chrome(raw)),
    };
    text.chars().take(180).collect()
}

fn strip_feed_chrome(line: &str) -> String {
    let mut text = line.trim().to_string();
    for mark in ["🪶", "💬", "🎬", "🎲", "✅", "❌", "💀", "🎭"] {
        text = text.replace(mark, "");
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn repeats_committed(line: &str, log: &[String]) -> bool {
    let spoken = line.to_lowercase();
    if spoken.contains("pc:") || spoken.contains('🪶') || spoken.contains("the player:") {
        return true;
    }
    for entry in log {
        let words: Vec<&str> = entry.split_whitespace().collect();
        if words.len() < 6 {
            continue;
        }
        let slice = words[words.len() / 3..].join(" ").to_lowercase();
        if slice.len() > 20 && spoken.contains(&slice) {
            return true;
        }
    }
    false
}

fn leaks_loot(line: &str) -> bool {
    let lower = line.to_lowercase();
    ["amulet", "password", "gold", "coin"]
        .iter()
        .any(|word| lower.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_and_price_do_not_spend() {
        assert!(!spends_turn("look around", false));
        assert!(!spends_turn("how much for a room", false));
        assert!(!spends_turn("inventory", false));
        assert!(spends_turn("sit down", false));
        assert!(!spends_turn("sit down", true));
    }

    #[test]
    fn round_fact_drops_the_feed_prefix() {
        let fact = plain_round_fact(
            "pc",
            "I pet the kitty",
            &["💬 🎭 Benny: \"I pet the kitty.\"".into(), "🪶 You approach the sleeping kitty by the fireplace and pet her.".into()],
        );
        assert!(fact.starts_with("the player:"));
        assert!(fact.contains("sleeping kitty"));
        assert!(!fact.contains('🪶'));
        assert!(!fact.contains("pc:"));
        assert!(!fact.contains("Benny"));
    }

    #[test]
    fn echoed_narration_is_not_a_spoken_line() {
        let log = vec![
            "the player: You approach the sleeping kitty by the fireplace and pet her. The cloaked figure sitting apart from the noise.".into(),
        ];
        let echo = "pc: 🪶 You approach the sleeping kitty by the fireplace and pet her. The cloaked figure sitting apart from the noise.";
        assert!(repeats_committed(echo, &log));
        assert!(!repeats_committed(
            "*the cloak stays still* \"Someone's made a friend.\"",
            &log
        ));
    }

    #[test]
    fn pending_seat_walks_the_table() {
        let seats = vec!["mira".into(), "pc".into()];
        let gone = vec!["pc".into()];
        assert_eq!(
            next_pending(&seats, "pc", &gone).as_deref(),
            Some("mira")
        );
        let gone = vec!["pc".into(), "mira".into()];
        assert!(all_gone(&seats, &gone));
    }
}
