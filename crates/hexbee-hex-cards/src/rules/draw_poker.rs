//! Five-card draw poker.

use crate::card::Card;
use crate::deck::SeatId;
use crate::moves::{ApplyEffect, IllegalMove, Move, ScoreDelta};
use crate::render::render_seat_hand;
use crate::rules::TableGame;
use crate::table::TableState;

const ANTE: i32 = 5;
const MAX_DISCARD: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Bet1,
    Discard,
    Bet2,
    Showdown,
    Done,
}

pub struct DrawPokerGame {
    phase: Phase,
    pot: i32,
    folded: [bool; 4],
    /// Contribution this betting round.
    round_bet: [u32; 4],
    to_call: u32,
    acted: [bool; 4],
    discarded: [bool; 4],
}

impl Default for DrawPokerGame {
    fn default() -> Self {
        Self::new()
    }
}

impl DrawPokerGame {
    pub fn new() -> Self {
        Self {
            phase: Phase::Done,
            pot: 0,
            folded: [false; 4],
            round_bet: [0; 4],
            to_call: 0,
            acted: [false; 4],
            discarded: [false; 4],
        }
    }

    fn active_seats(&self) -> Vec<SeatId> {
        (0..4)
            .filter(|&i| !self.folded[i])
            .map(|i| SeatId(i as u8))
            .collect()
    }

    fn reset_betting_round(&mut self) {
        self.round_bet = [0; 4];
        self.to_call = 0;
        self.acted = [false; 4];
        for i in 0..4 {
            if self.folded[i] {
                self.acted[i] = true;
            }
        }
    }

    fn betting_complete(&self) -> bool {
        self.active_seats().iter().all(|s| {
            self.acted[s.0 as usize] && self.round_bet[s.0 as usize] == self.to_call
        })
    }

    fn next_actor(&self, from: SeatId) -> Option<SeatId> {
        for i in 1..=4 {
            let s = SeatId((from.0 + i) % 4);
            if !self.folded[s.0 as usize] {
                return Some(s);
            }
        }
        None
    }

    fn first_active(&self) -> SeatId {
        self.active_seats().first().copied().unwrap_or(SeatId(0))
    }
}

impl TableGame for DrawPokerGame {
    fn name(&self) -> &str {
        "Five-card draw"
    }

    fn start_hand(&mut self, table: &mut TableState) -> Vec<String> {
        table.shoe.clear_all_to_discard();
        if table.shoe.draw_pile.len() < 20 {
            table.shoe.ensure_cards(52);
            if table.shoe.draw_pile.len() < 20 {
                table.shoe = crate::deck::Shoe::fresh(None);
            }
        }
        self.pot = 0;
        self.folded = [false; 4];
        self.discarded = [false; 4];
        self.reset_betting_round();
        for s in SeatId::all() {
            if table.score_of(s) >= ANTE {
                table.add_score(s, -ANTE);
                self.pot += ANTE;
            } else {
                self.folded[s.0 as usize] = true;
            }
        }
        for s in SeatId::all() {
            if !self.folded[s.0 as usize] {
                table.shoe.deal_to(s, 5, true);
            }
        }
        self.phase = Phase::Bet1;
        table.to_act = self.first_active();
        let mut lines = vec![format!("🃏 Five-card draw. Ante {ANTE}. Pot {}.", self.pot)];
        lines.extend(self.render_public(table));
        lines
    }

    fn legal_moves(&self, table: &TableState, seat: SeatId) -> Vec<Move> {
        if table.to_act != seat || self.folded[seat.0 as usize] {
            return vec![];
        }
        match self.phase {
            Phase::Bet1 | Phase::Bet2 => {
                let mut v = vec![Move::Fold];
                let mine = self.round_bet[seat.0 as usize];
                if mine == self.to_call {
                    v.push(Move::Check);
                    v.push(Move::Bet(10));
                    v.push(Move::Raise(10));
                } else {
                    v.push(Move::Call);
                    v.push(Move::Raise(self.to_call + 10));
                }
                v
            }
            Phase::Discard => {
                if self.discarded[seat.0 as usize] {
                    return vec![];
                }
                // Sentinel: discard 0–3 validated in apply
                vec![Move::Discard(vec![])]
            }
            _ => vec![],
        }
    }

    fn apply(
        &mut self,
        table: &mut TableState,
        seat: SeatId,
        mv: Move,
    ) -> Result<ApplyEffect, IllegalMove> {
        if self.folded[seat.0 as usize] && !matches!(mv, Move::Discard(_)) {
            return Err(IllegalMove::new("already folded", vec![]));
        }
        if table.to_act != seat {
            return Err(IllegalMove::new(
                "not your turn",
                self.legal_moves(table, seat),
            ));
        }
        let mut msgs = Vec::new();
        match self.phase {
            Phase::Bet1 | Phase::Bet2 => {
                match mv {
                    Move::Fold => {
                        self.folded[seat.0 as usize] = true;
                        self.acted[seat.0 as usize] = true;
                        msgs.push(format!("{} folds", table.player(seat).name));
                    }
                    Move::Check => {
                        if self.round_bet[seat.0 as usize] != self.to_call {
                            return Err(IllegalMove::new(
                                "cannot check — must call or raise",
                                self.legal_moves(table, seat),
                            ));
                        }
                        self.acted[seat.0 as usize] = true;
                        msgs.push(format!("{} checks", table.player(seat).name));
                    }
                    Move::Call => {
                        let need = self.to_call - self.round_bet[seat.0 as usize];
                        if need == 0 {
                            return Err(IllegalMove::new(
                                "nothing to call — check or bet",
                                self.legal_moves(table, seat),
                            ));
                        }
                        let pay = need as i32;
                        if table.score_of(seat) < pay {
                            return Err(IllegalMove::new("not enough chips", self.legal_moves(table, seat)));
                        }
                        table.add_score(seat, -pay);
                        self.pot += pay;
                        self.round_bet[seat.0 as usize] = self.to_call;
                        self.acted[seat.0 as usize] = true;
                        msgs.push(format!("{} calls ({need})", table.player(seat).name));
                    }
                    Move::Bet(n) | Move::Raise(n) => {
                        if n < 10 {
                            return Err(IllegalMove::new(
                                "minimum bet/raise is 10",
                                self.legal_moves(table, seat),
                            ));
                        }
                        let target = if matches!(mv, Move::Bet(_)) && self.to_call == 0 {
                            n
                        } else {
                            self.to_call.max(n)
                        };
                        let need = target.saturating_sub(self.round_bet[seat.0 as usize]);
                        if table.score_of(seat) < need as i32 {
                            return Err(IllegalMove::new("not enough chips", self.legal_moves(table, seat)));
                        }
                        table.add_score(seat, -(need as i32));
                        self.pot += need as i32;
                        self.round_bet[seat.0 as usize] = target;
                        self.to_call = target;
                        // Others must act again
                        for i in 0..4 {
                            self.acted[i] = self.folded[i] || i == seat.0 as usize;
                        }
                        msgs.push(format!(
                            "{} {} to {target}",
                            table.player(seat).name,
                            mv.kind_key()
                        ));
                    }
                    _ => {
                        return Err(IllegalMove::new(
                            "betting action required",
                            self.legal_moves(table, seat),
                        ));
                    }
                }
                let active = self.active_seats();
                if active.len() == 1 {
                    self.phase = Phase::Done;
                    msgs.push(format!("{} wins pot {} (others folded)", table.player(active[0]).name, self.pot));
                    return Ok(ApplyEffect {
                        messages: msgs,
                        hand_over: true,
                    });
                }
                if self.betting_complete() {
                    if self.phase == Phase::Bet1 {
                        self.phase = Phase::Discard;
                        self.discarded = [false; 4];
                        for i in 0..4 {
                            if self.folded[i] {
                                self.discarded[i] = true;
                            }
                        }
                        table.to_act = self.first_active();
                        msgs.push("Draw phase — discard 0–3 cards (discard AS KH) or `pat`.".into());
                    } else {
                        self.phase = Phase::Showdown;
                        msgs.push("Showdown!".into());
                        return Ok(ApplyEffect {
                            messages: msgs,
                            hand_over: true,
                        });
                    }
                } else if let Some(n) = self.next_actor(seat) {
                    table.to_act = n;
                }
            }
            Phase::Discard => {
                let cards = match mv {
                    Move::Discard(cs) => cs,
                    Move::Stand => vec![], // alias stand pat
                    _ => {
                        return Err(IllegalMove::new(
                            "discard up to 3 cards or `pat`",
                            vec![Move::Discard(vec![])],
                        ));
                    }
                };
                if cards.len() > MAX_DISCARD {
                    return Err(IllegalMove::new(
                        format!("may discard at most {MAX_DISCARD}"),
                        vec![Move::Discard(vec![])],
                    ));
                }
                let hand = table.shoe.hand(seat);
                for c in &cards {
                    if !hand.contains(c) {
                        return Err(IllegalMove::new(
                            format!("you don't hold {c}"),
                            vec![Move::Discard(vec![])],
                        ));
                    }
                }
                for c in &cards {
                    table.shoe.discard_from_hand(seat, *c);
                }
                let n = cards.len();
                if n > 0 {
                    table.shoe.deal_to(seat, n, true);
                }
                self.discarded[seat.0 as usize] = true;
                msgs.push(format!(
                    "{} discards {n}",
                    table.player(seat).name
                ));
                if self.discarded.iter().all(|&d| d) {
                    self.phase = Phase::Bet2;
                    self.reset_betting_round();
                    table.to_act = self.first_active();
                    msgs.push("Second betting round.".into());
                } else if let Some(n) = (0..4)
                    .map(|i| SeatId(i as u8))
                    .find(|s| !self.discarded[s.0 as usize])
                {
                    table.to_act = n;
                }
            }
            Phase::Showdown | Phase::Done => {
                return Err(IllegalMove::new("hand is over", vec![]));
            }
        }
        Ok(ApplyEffect {
            messages: msgs,
            hand_over: self.phase == Phase::Done || self.phase == Phase::Showdown,
        })
    }

    fn is_hand_over(&self, _table: &TableState) -> bool {
        matches!(self.phase, Phase::Done | Phase::Showdown)
    }

    fn settle(&mut self, table: &mut TableState) -> Vec<ScoreDelta> {
        let active = self.active_seats();
        if active.is_empty() {
            return vec![];
        }
        if active.len() == 1 || self.phase == Phase::Done {
            let w = active[0];
            table.add_score(w, self.pot);
            let d = ScoreDelta {
                seat: w,
                delta: self.pot,
            };
            self.pot = 0;
            self.phase = Phase::Done;
            return vec![d];
        }
        // Showdown
        let mut best_seat = active[0];
        let mut best_rank = hand_rank(&table.shoe.hand(best_seat));
        for &s in &active[1..] {
            let r = hand_rank(&table.shoe.hand(s));
            if r > best_rank {
                best_rank = r;
                best_seat = s;
            }
        }
        table.add_score(best_seat, self.pot);
        let pot = self.pot;
        self.pot = 0;
        self.phase = Phase::Done;
        vec![ScoreDelta {
            seat: best_seat,
            delta: pot,
        }]
    }

    fn prompt_hint(&self, table: &TableState, seat: SeatId) -> String {
        match self.phase {
            Phase::Bet1 | Phase::Bet2 => {
                let legal = self.legal_moves(table, seat);
                format!(
                    "Bet: {}",
                    legal.iter().map(|m| m.label()).collect::<Vec<_>>().join(" / ")
                )
            }
            Phase::Discard => "discard C1 C2 … (max 3) or `pat`".into(),
            _ => "Hand over.".into(),
        }
    }

    fn render_public(&self, table: &TableState) -> Vec<String> {
        let mut lines = vec![format!("Pot: {}", self.pot)];
        for s in SeatId::all() {
            let status = if self.folded[s.0 as usize] {
                " (folded)"
            } else {
                ""
            };
            lines.push(format!("{}{status}:", table.player(s).name));
            if s.0 == 0 && !self.folded[0] {
                lines.extend(render_seat_hand(&table.shoe, s, true));
            } else if !self.folded[s.0 as usize] {
                lines.push(format!("  [{} cards]", table.shoe.hand(s).len()));
            }
        }
        lines
    }
}

/// Rank category: higher is better. (category, tiebreakers high to low)
pub fn hand_rank(cards: &[Card]) -> (u8, Vec<u8>) {
    let mut ranks: Vec<u8> = cards.iter().map(|c| c.rank as u8).collect();
    ranks.sort_unstable_by(|a, b| b.cmp(a));
    let mut counts: Vec<(u8, u8)> = Vec::new(); // (rank, count)
    for r in &ranks {
        if let Some(last) = counts.last_mut() {
            if last.0 == *r {
                last.1 += 1;
                continue;
            }
        }
        counts.push((*r, 1));
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));

    let flush = cards.windows(2).all(|w| w[0].suit == cards[0].suit) && cards.len() == 5
        || {
            let s0 = cards[0].suit;
            cards.iter().all(|c| c.suit == s0) && cards.len() == 5
        };
    let mut sorted = ranks.clone();
    sorted.sort_unstable();
    let straight = is_straight(&sorted);

    if flush && straight {
        return (8, vec![ranks[0]]);
    }
    if counts[0].1 == 4 {
        return (7, vec![counts[0].0, counts.get(1).map(|c| c.0).unwrap_or(0)]);
    }
    if counts[0].1 == 3 && counts.get(1).map(|c| c.1) == Some(2) {
        return (6, vec![counts[0].0, counts[1].0]);
    }
    if flush {
        return (5, ranks);
    }
    if straight {
        return (4, vec![straight_high(&sorted)]);
    }
    if counts[0].1 == 3 {
        let mut tb = vec![counts[0].0];
        for c in &counts[1..] {
            tb.push(c.0);
        }
        return (3, tb);
    }
    if counts[0].1 == 2 && counts.get(1).map(|c| c.1) == Some(2) {
        let high_pair = counts[0].0.max(counts[1].0);
        let low_pair = counts[0].0.min(counts[1].0);
        let kicker = counts.get(2).map(|c| c.0).unwrap_or(0);
        return (2, vec![high_pair, low_pair, kicker]);
    }
    if counts[0].1 == 2 {
        let mut tb = vec![counts[0].0];
        for c in &counts[1..] {
            tb.push(c.0);
        }
        return (1, tb);
    }
    (0, ranks)
}

fn is_straight(sorted_asc: &[u8]) -> bool {
    if sorted_asc.len() != 5 {
        return false;
    }
    // A-2-3-4-5
    if sorted_asc == [2, 3, 4, 5, 14] {
        return true;
    }
    for w in sorted_asc.windows(2) {
        if w[1] != w[0] + 1 {
            return false;
        }
    }
    true
}

fn straight_high(sorted_asc: &[u8]) -> u8 {
    if sorted_asc == [2, 3, 4, 5, 14] {
        5
    } else {
        *sorted_asc.last().unwrap_or(&0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, Rank, Suit};

    #[test]
    fn pair_beats_high() {
        let pair = vec![
            Card::new(Suit::Spades, Rank::Ace),
            Card::new(Suit::Hearts, Rank::Ace),
            Card::new(Suit::Clubs, Rank::Two),
            Card::new(Suit::Diamonds, Rank::Three),
            Card::new(Suit::Spades, Rank::Four),
        ];
        let high = vec![
            Card::new(Suit::Spades, Rank::King),
            Card::new(Suit::Hearts, Rank::Queen),
            Card::new(Suit::Clubs, Rank::Two),
            Card::new(Suit::Diamonds, Rank::Three),
            Card::new(Suit::Spades, Rank::Four),
        ];
        assert!(hand_rank(&pair) > hand_rank(&high));
    }

    #[test]
    fn discard_four_illegal() {
        let mut g = DrawPokerGame::new();
        let mut table = TableState::new(Some(1));
        g.start_hand(&mut table);
        g.phase = Phase::Discard;
        g.discarded = [false, true, true, true];
        table.to_act = SeatId(0);
        let cards: Vec<_> = table.shoe.hand(SeatId(0)).into_iter().take(4).collect();
        let err = g
            .apply(&mut table, SeatId(0), Move::Discard(cards))
            .unwrap_err();
        assert!(err.reason.contains("at most"));
    }
}
