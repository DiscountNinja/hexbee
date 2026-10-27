//! AI decision tree: dealer policy, LLM play, validate + fallback.

use hexbee_provider::LlmClient;
use tracing::warn;

use crate::card::{Card, Rank};
use crate::deck::SeatId;
use crate::moves::Move;
use crate::player::SeatKind;
use crate::rules::{dealer_should_hit, TableGame};
use crate::table::{GameKind, TableState};

/// Choose a move for the seat to act. Always returns a move from `legal` when non-empty.
pub async fn choose_move(
    game: &dyn TableGame,
    table: &TableState,
    seat: SeatId,
    legal: &[Move],
    llm: Option<&dyn LlmClient>,
) -> Option<Move> {
    if legal.is_empty() {
        return None;
    }
    let kind = table.player(seat).kind;
    match kind {
        SeatKind::Human => None,
        SeatKind::Dealer => Some(dealer_move(table, seat, legal)),
        SeatKind::Ai => {
            if let Some(client) = llm {
                if let Some(mv) = llm_pick(game, table, seat, legal, client).await {
                    if legal_contains(legal, &mv) {
                        return Some(mv);
                    }
                    warn!("ai chose illegal move, falling back");
                }
            }
            Some(fallback(legal))
        }
    }
}

fn dealer_move(table: &TableState, seat: SeatId, legal: &[Move]) -> Move {
    // 21 dealer: hit or stand from totals
    let hand = table.shoe.hand(seat);
    if dealer_should_hit(&hand) {
        if legal.iter().any(|m| matches!(m, Move::Hit)) {
            return Move::Hit;
        }
    } else if legal.iter().any(|m| matches!(m, Move::Stand)) {
        return Move::Stand;
    }
    fallback(legal)
}

fn fallback(legal: &[Move]) -> Move {
    // Prefer stand/check/call/pat over fold when present
    for pref in [
        Move::Stand,
        Move::Check,
        Move::Call,
        Move::Discard(vec![]),
    ] {
        if let Some(m) = legal.iter().find(|m| matches_pref(m, &pref)) {
            return m.clone();
        }
    }
    // Hearts/poker: first legal play
    legal[0].clone()
}

fn matches_pref(m: &Move, pref: &Move) -> bool {
    match (m, pref) {
        (Move::Stand, Move::Stand)
        | (Move::Check, Move::Check)
        | (Move::Call, Move::Call) => true,
        (Move::Discard(a), Move::Discard(b)) if a.is_empty() && b.is_empty() => true,
        _ => false,
    }
}

fn legal_contains(legal: &[Move], mv: &Move) -> bool {
    legal.iter().any(|m| move_compatible(m, mv))
}

fn move_compatible(legal: &Move, chosen: &Move) -> bool {
    match (legal, chosen) {
        (Move::Hit, Move::Hit) | (Move::Stand, Move::Stand) | (Move::Call, Move::Call)
        | (Move::Fold, Move::Fold) | (Move::Check, Move::Check) | (Move::Ready, Move::Ready) => {
            true
        }
        (Move::PlayCard(a), Move::PlayCard(b)) => a == b,
        (Move::PassCards(_), Move::PassCards(b)) => b.len() == 3,
        (Move::Discard(_), Move::Discard(b)) => b.len() <= 3,
        (Move::Bet(a), Move::Bet(b)) => a == b,
        (Move::Raise(a), Move::Raise(b)) => *b >= *a || a == b,
        (Move::Bet(_), Move::Raise(_)) | (Move::Raise(_), Move::Bet(_)) => false,
        _ => legal == chosen,
    }
}

async fn llm_pick(
    game: &dyn TableGame,
    table: &TableState,
    seat: SeatId,
    legal: &[Move],
    llm: &dyn LlmClient,
) -> Option<Move> {
    let name = &table.player(seat).name;
    let hand = table
        .shoe
        .hand(seat)
        .iter()
        .map(|c| c.short())
        .collect::<Vec<_>>()
        .join(", ");
    let legal_desc: Vec<String> = legal
        .iter()
        .enumerate()
        .map(|(i, m)| format!("{i}: {}", m.label()))
        .collect();
    let game_name = table.game.map(|g| g.display()).unwrap_or(game.name());
    let system = format!(
        "You are {name} playing {game_name}. Reply with JSON only: {{\"index\":N}} or {{\"move\":\"label\"}} choosing one legal option. No prose."
    );
    let user = format!(
        "Hand: [{hand}]\nScores: {}\nLegal moves:\n{}\nPick one.",
        table.scores_line(),
        legal_desc.join("\n")
    );
    let raw = llm.complete_json(&system, &user).await.ok()?;
    parse_llm_move(&raw, legal).or_else(|| {
        // one soft retry context unused — caller falls back
        None
    })
}

pub fn parse_llm_move(raw: &str, legal: &[Move]) -> Option<Move> {
    let v: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;
    if let Some(i) = v.get("index").and_then(|x| x.as_u64()) {
        return legal.get(i as usize).cloned();
    }
    if let Some(s) = v.get("move").and_then(|x| x.as_str()) {
        let lower = s.to_lowercase();
        // index as string
        if let Ok(i) = lower.parse::<usize>() {
            return legal.get(i).cloned();
        }
        if let Some(m) = legal.iter().find(|m| m.label().eq_ignore_ascii_case(s)) {
            return Some(m.clone());
        }
        // kind match
        if let Some(m) = legal.iter().find(|m| lower.contains(m.kind_key())) {
            return Some(m.clone());
        }
        // play card
        if let Some(card) = crate::card::Card::parse(s) {
            if let Some(m) = legal.iter().find(|m| matches!(m, Move::PlayCard(c) if *c == card)) {
                return Some(m.clone());
            }
        }
    }
    None
}

/// Heuristic AI for hearts pass / discard when legal is a sentinel.
pub fn heuristic_pass(table: &TableState, seat: SeatId) -> Move {
    let mut hand = table.shoe.hand(seat);
    // Dump high spades / hearts
    hand.sort_by_key(|c| {
        let danger = if c.suit == crate::card::Suit::Spades && c.rank == crate::card::Rank::Queen {
            100
        } else if c.suit == crate::card::Suit::Hearts {
            50 + c.rank as u8
        } else if c.suit == crate::card::Suit::Spades && c.rank as u8 >= 12 {
            40
        } else {
            c.rank as u8
        };
        std::cmp::Reverse(danger)
    });
    let pass: Vec<_> = hand.into_iter().take(3).collect();
    Move::PassCards(pass)
}

pub fn heuristic_discard(table: &TableState, seat: SeatId) -> Move {
    let hand = table.shoe.hand(seat);
    // Keep pairs; discard low kickers up to 3
    use std::collections::HashMap;
    let mut counts: HashMap<Rank, Vec<Card>> = HashMap::new();
    for c in &hand {
        counts.entry(c.rank).or_default().push(*c);
    }
    let mut singles: Vec<Card> = counts
        .values()
        .filter(|v| v.len() == 1)
        .map(|v| v[0])
        .collect();
    singles.sort_by_key(|c| c.rank);
    let take = singles.len().min(3);
    Move::Discard(singles.into_iter().take(take).collect())
}

/// Expand sentinel legal moves for AI before choose_move.
pub fn expand_legal_for_ai(
    game_kind: Option<GameKind>,
    table: &TableState,
    seat: SeatId,
    legal: Vec<Move>,
) -> Vec<Move> {
    if legal.len() == 1 {
        match &legal[0] {
            Move::PassCards(v) if v.is_empty() => {
                return vec![heuristic_pass(table, seat)];
            }
            Move::Discard(v) if v.is_empty() => {
                if game_kind == Some(GameKind::DrawPoker) {
                    return vec![
                        heuristic_discard(table, seat),
                        Move::Discard(vec![]),
                    ];
                }
            }
            _ => {}
        }
    }
    legal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_index() {
        let legal = vec![Move::Hit, Move::Stand];
        assert_eq!(parse_llm_move(r#"{"index":1}"#, &legal), Some(Move::Stand));
    }
}
