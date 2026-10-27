//! Blackjack-style 21.

use serde_json::json;

use crate::card::{Card, Rank};
use crate::deck::SeatId;
use crate::moves::{ApplyEffect, IllegalMove, Move, ScoreDelta};
use crate::render::render_seat_hand;
use crate::rules::TableGame;
use crate::table::TableState;

const BET: i32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    PlayerTurns,
    DealerTurn,
    Done,
}

pub struct TwentyOneGame {
    phase: Phase,
    /// Seats that have finished (stood or bust).
    done: [bool; 4],
    bets: [i32; 4],
}

impl Default for TwentyOneGame {
    fn default() -> Self {
        Self::new()
    }
}

impl TwentyOneGame {
    pub fn new() -> Self {
        Self {
            phase: Phase::Done,
            done: [true, true, true, true],
            bets: [0; 4],
        }
    }
}

/// Soft/hard total for blackjack. Returns (total, soft).
pub fn blackjack_total(cards: &[Card]) -> (u8, bool) {
    let mut total = 0u8;
    let mut aces = 0u8;
    for c in cards {
        match c.rank {
            Rank::Ace => {
                aces += 1;
                total += 11;
            }
            r => total += r.blackjack_pip(),
        }
    }
    while total > 21 && aces > 0 {
        total -= 10;
        aces -= 1;
    }
    let soft = aces > 0 && total <= 21;
    (total, soft)
}

/// House: hit on 16 or less (including soft 17 stands — classic stand on all 17).
pub fn dealer_should_hit(cards: &[Card]) -> bool {
    let (total, _) = blackjack_total(cards);
    total <= 16
}

fn dealer_seat() -> SeatId {
    SeatId(3)
}

impl TableGame for TwentyOneGame {
    fn name(&self) -> &str {
        "21"
    }

    fn start_hand(&mut self, table: &mut TableState) -> Vec<String> {
        table.shoe.clear_all_to_discard();
        self.bets = [0; 4];
        self.done = [false, false, false, true]; // dealer acts later
        for s in 0..3u8 {
            let seat = SeatId(s);
            if table.score_of(seat) < BET {
                self.done[s as usize] = true;
                continue;
            }
            table.add_score(seat, -BET);
            self.bets[s as usize] = BET;
        }
        // Deal 2 each; dealer second card face down
        for _ in 0..2 {
            for s in 0..3u8 {
                if self.bets[s as usize] > 0 {
                    table.shoe.deal_to(SeatId(s), 1, true);
                }
            }
            // dealer
            let face_up = table.shoe.hand(dealer_seat()).is_empty();
            table.shoe.deal_to(dealer_seat(), 1, face_up);
        }
        self.phase = Phase::PlayerTurns;
        self.done[3] = true; // dealer not in player loop
        // Natural blackjacks auto-stand
        for s in 0..3u8 {
            if self.bets[s as usize] == 0 {
                continue;
            }
            let hand = table.shoe.hand(SeatId(s));
            let (t, _) = blackjack_total(&hand);
            if t == 21 {
                self.done[s as usize] = true;
            }
        }
        table.to_act = next_player_to_act(self, 0);
        table.scratch = json!({ "phase": "players" });
        let mut lines = vec!["🃏 New hand of 21. Bet 10.".into()];
        lines.extend(self.render_public(table));
        lines
    }

    fn legal_moves(&self, table: &TableState, seat: SeatId) -> Vec<Move> {
        if self.phase != Phase::PlayerTurns || seat.0 >= 3 {
            return vec![];
        }
        if table.to_act != seat || self.done[seat.0 as usize] {
            return vec![];
        }
        let hand = table.shoe.hand(seat);
        let (total, _) = blackjack_total(&hand);
        if total >= 21 {
            return vec![Move::Stand];
        }
        vec![Move::Hit, Move::Stand]
    }

    fn apply(
        &mut self,
        table: &mut TableState,
        seat: SeatId,
        mv: Move,
    ) -> Result<ApplyEffect, IllegalMove> {
        let legal = self.legal_moves(table, seat);
        if !legal.iter().any(|m| move_eq(m, &mv)) {
            return Err(IllegalMove::new(
                format!("cannot {} now", mv.label()),
                legal,
            ));
        }
        let mut msgs = Vec::new();
        match mv {
            Move::Hit => {
                table.shoe.deal_to(seat, 1, true);
                let hand = table.shoe.hand(seat);
                let (total, _) = blackjack_total(&hand);
                msgs.push(format!("{} hits → {total}", table.player(seat).name));
                if total > 21 {
                    msgs.push(format!("{} busts!", table.player(seat).name));
                    self.done[seat.0 as usize] = true;
                    table.to_act = next_player_to_act(self, seat.0 + 1);
                } else if total == 21 {
                    self.done[seat.0 as usize] = true;
                    table.to_act = next_player_to_act(self, seat.0 + 1);
                }
            }
            Move::Stand => {
                let hand = table.shoe.hand(seat);
                let (total, _) = blackjack_total(&hand);
                msgs.push(format!("{} stands at {total}", table.player(seat).name));
                self.done[seat.0 as usize] = true;
                table.to_act = next_player_to_act(self, seat.0 + 1);
            }
            _ => {
                return Err(IllegalMove::new("not a 21 action", legal));
            }
        }
        if players_all_done(self) {
            msgs.extend(self.run_dealer(table));
            self.phase = Phase::Done;
            return Ok(ApplyEffect {
                messages: msgs,
                hand_over: true,
            });
        }
        Ok(ApplyEffect {
            messages: msgs,
            hand_over: false,
        })
    }

    fn is_hand_over(&self, _table: &TableState) -> bool {
        self.phase == Phase::Done
    }

    fn settle(&mut self, table: &mut TableState) -> Vec<ScoreDelta> {
        let dealer_hand = table.shoe.hand(dealer_seat());
        let (d_total, _) = blackjack_total(&dealer_hand);
        let dealer_bust = d_total > 21;
        let mut deltas = Vec::new();
        for s in 0..3u8 {
            let seat = SeatId(s);
            let bet = self.bets[s as usize];
            if bet == 0 {
                continue;
            }
            let hand = table.shoe.hand(seat);
            let (p_total, _) = blackjack_total(&hand);
            let delta = if p_total > 21 {
                0 // already lost bet
            } else if dealer_bust || p_total > d_total {
                let win = bet * 2; // return stake + win
                table.add_score(seat, win);
                deltas.push(ScoreDelta { seat, delta: win });
                win
            } else if p_total == d_total {
                table.add_score(seat, bet); // push
                deltas.push(ScoreDelta { seat, delta: bet });
                bet
            } else {
                0
            };
            let _ = delta;
        }
        // Reveal dealer
        table.shoe.set_face_up(dealer_seat(), true);
        deltas
    }

    fn prompt_hint(&self, table: &TableState, seat: SeatId) -> String {
        let legal = self.legal_moves(table, seat);
        if legal.is_empty() {
            return "Wait…".into();
        }
        format!(
            "Your move: {}",
            legal
                .iter()
                .map(|m| m.label())
                .collect::<Vec<_>>()
                .join(" / ")
        )
    }

    fn render_public(&self, table: &TableState) -> Vec<String> {
        let mut lines = Vec::new();
        for s in SeatId::all() {
            let name = &table.player(s).name;
            let viewer = s.0 == 0;
            let hand_lines = render_seat_hand(&table.shoe, s, viewer || s == dealer_seat() && self.phase == Phase::Done);
            // Always show dealer up-cards as stored face_up; hole stays down until done
            let hand_lines = if s == dealer_seat() && self.phase != Phase::Done {
                render_seat_hand(&table.shoe, s, false)
            } else {
                hand_lines
            };
            let total = if s.0 == 0 || (s == dealer_seat() && self.phase == Phase::Done) {
                let h = table.shoe.hand(s);
                let (t, _) = blackjack_total(&h);
                format!(" ({t})")
            } else if s.0 < 3 {
                String::new()
            } else {
                String::new()
            };
            lines.push(format!("{name}{total}:"));
            lines.extend(hand_lines);
        }
        lines
    }
}

impl TwentyOneGame {
    fn run_dealer(&mut self, table: &mut TableState) -> Vec<String> {
        self.phase = Phase::DealerTurn;
        table.shoe.set_face_up(dealer_seat(), true);
        let mut msgs = vec!["Dealer reveals.".into()];
        while dealer_should_hit(&table.shoe.hand(dealer_seat())) {
            table.shoe.deal_to(dealer_seat(), 1, true);
            let (t, _) = blackjack_total(&table.shoe.hand(dealer_seat()));
            msgs.push(format!("Dealer hits → {t}"));
        }
        let (t, _) = blackjack_total(&table.shoe.hand(dealer_seat()));
        if t > 21 {
            msgs.push("Dealer busts!".into());
        } else {
            msgs.push(format!("Dealer stands at {t}"));
        }
        msgs
    }
}

fn players_all_done(g: &TwentyOneGame) -> bool {
    (0..3).all(|i| g.done[i] || g.bets[i] == 0)
}

fn next_player_to_act(g: &TwentyOneGame, from: u8) -> SeatId {
    for s in from..3 {
        if !g.done[s as usize] && g.bets[s as usize] > 0 {
            return SeatId(s);
        }
    }
    SeatId(0) // unused when all done
}

fn move_eq(a: &Move, b: &Move) -> bool {
    match (a, b) {
        (Move::Hit, Move::Hit) | (Move::Stand, Move::Stand) => true,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, Rank, Suit};

    #[test]
    fn dealer_stands_on_17() {
        let cards = vec![
            Card::new(Suit::Spades, Rank::Ten),
            Card::new(Suit::Hearts, Rank::Seven),
        ];
        assert!(!dealer_should_hit(&cards));
        let cards16 = vec![
            Card::new(Suit::Spades, Rank::Ten),
            Card::new(Suit::Hearts, Rank::Six),
        ];
        assert!(dealer_should_hit(&cards16));
    }

    #[test]
    fn soft_ace() {
        let cards = vec![
            Card::new(Suit::Spades, Rank::Ace),
            Card::new(Suit::Hearts, Rank::Six),
        ];
        assert_eq!(blackjack_total(&cards).0, 17);
    }

    #[test]
    fn hit_after_stand_illegal() {
        let mut g = TwentyOneGame::new();
        let mut table = TableState::new(Some(7));
        table.setup_players_for(crate::table::GameKind::TwentyOne);
        g.start_hand(&mut table);
        g.phase = Phase::PlayerTurns;
        g.done = [false, true, true, true];
        g.bets = [10, 0, 0, 0];
        table.to_act = SeatId(0);
        table.shoe.clear_all_to_discard();
        table.shoe.add_to_hand(SeatId(0), Card::new(Suit::Spades, Rank::Ten), true);
        table.shoe.add_to_hand(SeatId(0), Card::new(Suit::Hearts, Rank::Seven), true);
        g.apply(&mut table, SeatId(0), Move::Stand).unwrap();
        g.done[0] = true;
        table.to_act = SeatId(0);
        let err = g.apply(&mut table, SeatId(0), Move::Hit).unwrap_err();
        assert!(err.reason.contains("cannot") || err.legal.is_empty());
    }
}
