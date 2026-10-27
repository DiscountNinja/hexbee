//! Classic Hearts for 4 players.

use crate::card::{Card, Rank, Suit};
use crate::deck::SeatId;
use crate::moves::{ApplyEffect, IllegalMove, Move, ScoreDelta};
use crate::render::{render_hand_cards, render_seat_hand};
use crate::rules::TableGame;
use crate::table::TableState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Pass,
    Play,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PassDir {
    Left,
    Right,
    Across,
    Hold,
}

pub struct HeartsGame {
    phase: Phase,
    pass_dir: PassDir,
    hand_number: u32,
    passed: [bool; 4],
    pass_buffer: [Vec<Card>; 4],
    hearts_broken: bool,
    trick: Vec<(SeatId, Card)>,
    trick_leader: SeatId,
    hand_points: [i32; 4],
    tricks_played: u8,
}

impl Default for HeartsGame {
    fn default() -> Self {
        Self::new()
    }
}

impl HeartsGame {
    pub fn new() -> Self {
        Self {
            phase: Phase::Done,
            pass_dir: PassDir::Left,
            hand_number: 0,
            passed: [false; 4],
            pass_buffer: [vec![], vec![], vec![], vec![]],
            hearts_broken: false,
            trick: Vec::new(),
            trick_leader: SeatId(0),
            hand_points: [0; 4],
            tricks_played: 0,
        }
    }

    fn card_points(c: Card) -> i32 {
        if c.suit == Suit::Hearts {
            1
        } else if c.suit == Suit::Spades && c.rank == Rank::Queen {
            13
        } else {
            0
        }
    }

    fn pass_target(&self, from: SeatId) -> SeatId {
        match self.pass_dir {
            PassDir::Left => SeatId((from.0 + 1) % 4),
            PassDir::Right => SeatId((from.0 + 3) % 4),
            PassDir::Across => SeatId((from.0 + 2) % 4),
            PassDir::Hold => from,
        }
    }

    fn advance_pass_dir(&mut self) {
        self.pass_dir = match self.pass_dir {
            PassDir::Left => PassDir::Right,
            PassDir::Right => PassDir::Across,
            PassDir::Across => PassDir::Hold,
            PassDir::Hold => PassDir::Left,
        };
    }

    fn legal_plays(&self, table: &TableState, seat: SeatId) -> Vec<Card> {
        let hand = table.shoe.hand(seat);
        if self.trick.is_empty() {
            // Leading
            if self.tricks_played == 0 {
                // First trick: must lead 2♣ if held
                if let Some(two) = hand
                    .iter()
                    .find(|c| c.suit == Suit::Clubs && c.rank == Rank::Two)
                {
                    return vec![*two];
                }
            }
            if !self.hearts_broken {
                let non_hearts: Vec<_> = hand.iter().copied().filter(|c| c.suit != Suit::Hearts).collect();
                if !non_hearts.is_empty() {
                    return non_hearts;
                }
            }
            return hand;
        }
        let lead = self.trick[0].1.suit;
        let follow: Vec<_> = hand.iter().copied().filter(|c| c.suit == lead).collect();
        if !follow.is_empty() {
            return follow;
        }
        // Can dump; on first trick no points traditionally — keep simple: allow any
        if self.tricks_played == 0 {
            let safe: Vec<_> = hand
                .iter()
                .copied()
                .filter(|c| Self::card_points(*c) == 0)
                .collect();
            if !safe.is_empty() {
                return safe;
            }
        }
        hand
    }

    fn resolve_trick(&mut self, table: &mut TableState) -> Vec<String> {
        let lead_suit = self.trick[0].1.suit;
        let mut winner = self.trick[0].0;
        let mut best = self.trick[0].1;
        for &(seat, card) in &self.trick[1..] {
            if card.suit == Suit::Hearts {
                self.hearts_broken = true;
            }
            if card.suit == lead_suit && card.rank > best.rank {
                winner = seat;
                best = card;
            }
        }
        let pts: i32 = self.trick.iter().map(|(_, c)| Self::card_points(*c)).sum();
        self.hand_points[winner.0 as usize] += pts;
        for &(_, c) in &self.trick {
            table.shoe.discard_card(c);
        }
        self.trick.clear();
        self.tricks_played += 1;
        self.trick_leader = winner;
        table.to_act = winner;
        let mut msgs = vec![format!(
            "{} takes the trick (+{pts})",
            table.player(winner).name
        )];
        if self.tricks_played >= 13 {
            self.phase = Phase::Done;
            if let Some((i, _)) = self
                .hand_points
                .iter()
                .enumerate()
                .find(|(_, p)| **p == 26)
            {
                for s in 0..4 {
                    if s == i {
                        self.hand_points[s] = 0;
                    } else {
                        self.hand_points[s] = 26;
                    }
                }
                msgs.push(format!("{} shoots the moon!", table.players[i].name));
            }
            msgs.push("Hand over.".into());
        }
        msgs
    }
}

impl TableGame for HeartsGame {
    fn name(&self) -> &str {
        "Hearts"
    }

    fn start_hand(&mut self, table: &mut TableState) -> Vec<String> {
        table.shoe = crate::deck::Shoe::fresh(None);
        self.hand_number += 1;
        self.passed = [false; 4];
        self.pass_buffer = [vec![], vec![], vec![], vec![]];
        self.hearts_broken = false;
        self.trick.clear();
        self.hand_points = [0; 4];
        self.tricks_played = 0;
        for s in SeatId::all() {
            table.shoe.deal_to(s, 13, true);
        }
        if self.pass_dir == PassDir::Hold {
            self.phase = Phase::Play;
            self.trick_leader = find_two_clubs(table).unwrap_or(SeatId(0));
            table.to_act = self.trick_leader;
        } else {
            self.phase = Phase::Pass;
            table.to_act = SeatId(0);
        }
        let dir = match self.pass_dir {
            PassDir::Left => "left",
            PassDir::Right => "right",
            PassDir::Across => "across",
            PassDir::Hold => "hold (no pass)",
        };
        let mut lines = vec![format!("♥️ Hearts hand {}. Pass {dir}.", self.hand_number)];
        lines.extend(self.render_public(table));
        lines
    }

    fn legal_moves(&self, table: &TableState, seat: SeatId) -> Vec<Move> {
        if table.to_act != seat {
            return vec![];
        }
        match self.phase {
            Phase::Pass => {
                if self.passed[seat.0 as usize] || self.pass_dir == PassDir::Hold {
                    return vec![];
                }
                // Enumerate combinations of 3 is huge — expose PassCards as template via Ready? 
                // For AI/human we accept PassCards(3) validated in apply; list a sentinel Ready won't work.
                // Provide all C(13,3) is too many. Return empty list marker — human parses "pass X Y Z".
                // For decision tree, AI builds from hand. legal_moves returns placeholder PassCards empty meaning "must pass 3".
                vec![Move::PassCards(vec![])] // sentinel: must supply 3 cards
            }
            Phase::Play => self
                .legal_plays(table, seat)
                .into_iter()
                .map(Move::PlayCard)
                .collect(),
            Phase::Done => vec![],
        }
    }

    fn apply(
        &mut self,
        table: &mut TableState,
        seat: SeatId,
        mv: Move,
    ) -> Result<ApplyEffect, IllegalMove> {
        match self.phase {
            Phase::Pass => {
                let Move::PassCards(cards) = mv else {
                    return Err(IllegalMove::new(
                        "pass three cards (e.g. pass AS KH QD)",
                        vec![Move::PassCards(vec![])],
                    ));
                };
                if cards.len() != 3 {
                    return Err(IllegalMove::new(
                        "must pass exactly 3 cards",
                        vec![Move::PassCards(vec![])],
                    ));
                }
                let hand = table.shoe.hand(seat);
                for c in &cards {
                    if !hand.contains(c) {
                        return Err(IllegalMove::new(
                            format!("you don't hold {c}"),
                            vec![Move::PassCards(vec![])],
                        ));
                    }
                }
                // unique
                let mut uniq = cards.clone();
                uniq.sort_by_key(|c| (c.suit as u8, c.rank as u8));
                uniq.dedup();
                if uniq.len() != 3 {
                    return Err(IllegalMove::new(
                        "cards must be distinct",
                        vec![Move::PassCards(vec![])],
                    ));
                }
                for c in &cards {
                    table.shoe.take_from_hand(seat, *c);
                }
                self.pass_buffer[seat.0 as usize] = cards;
                self.passed[seat.0 as usize] = true;
                let mut msgs = vec![format!("{} passes 3 cards", table.player(seat).name)];
                // Next seater who hasn't passed
                if let Some(next) = (0..4).map(SeatId).find(|s| !self.passed[s.0 as usize]) {
                    table.to_act = next;
                } else {
                    // Deliver passes
                    for s in 0..4u8 {
                        let from = SeatId(s);
                        let to = self.pass_target(from);
                        let bundle = std::mem::take(&mut self.pass_buffer[s as usize]);
                        for c in bundle {
                            table.shoe.add_to_hand(to, c, true);
                        }
                    }
                    self.phase = Phase::Play;
                    self.trick_leader = find_two_clubs(table).unwrap_or(SeatId(0));
                    table.to_act = self.trick_leader;
                    msgs.push(format!(
                        "Pass complete. {} leads.",
                        table.player(self.trick_leader).name
                    ));
                }
                Ok(ApplyEffect {
                    messages: msgs,
                    hand_over: false,
                })
            }
            Phase::Play => {
                let Move::PlayCard(card) = mv else {
                    let legal = self.legal_moves(table, seat);
                    return Err(IllegalMove::new("play a card", legal));
                };
                let legal_cards = self.legal_plays(table, seat);
                if !legal_cards.contains(&card) {
                    return Err(IllegalMove::new(
                        format!("{card} is not legal"),
                        legal_cards.into_iter().map(Move::PlayCard).collect(),
                    ));
                }
                if table.to_act != seat {
                    return Err(IllegalMove::new(
                        "not your turn",
                        self.legal_moves(table, seat),
                    ));
                }
                table.shoe.take_from_hand(seat, card);
                self.trick.push((seat, card));
                let mut msgs = vec![format!("{} plays {card}", table.player(seat).name)];
                if self.trick.len() == 4 {
                    msgs.extend(self.resolve_trick(table));
                } else {
                    table.to_act = SeatId((seat.0 + 1) % 4);
                }
                Ok(ApplyEffect {
                    messages: msgs,
                    hand_over: self.phase == Phase::Done,
                })
            }
            Phase::Done => Err(IllegalMove::new("hand is over", vec![])),
        }
    }

    fn is_hand_over(&self, _table: &TableState) -> bool {
        self.phase == Phase::Done
    }

    fn settle(&mut self, table: &mut TableState) -> Vec<ScoreDelta> {
        let mut deltas = Vec::new();
        for s in 0..4u8 {
            let seat = SeatId(s);
            let d = self.hand_points[s as usize];
            table.add_score(seat, d);
            deltas.push(ScoreDelta { seat, delta: d });
        }
        self.advance_pass_dir();
        deltas
    }

    fn prompt_hint(&self, table: &TableState, seat: SeatId) -> String {
        match self.phase {
            Phase::Pass => "Pass 3 cards: pass AS KH 2D".into(),
            Phase::Play => {
                let legal = self.legal_moves(table, seat);
                format!(
                    "Play: {}",
                    legal
                        .iter()
                        .take(8)
                        .map(|m| m.label())
                        .collect::<Vec<_>>()
                        .join(" · ")
                )
            }
            Phase::Done => "Hand over. Type deal for next.".into(),
        }
    }

    fn render_public(&self, table: &TableState) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.trick.is_empty() {
            let cards: Vec<_> = self.trick.iter().map(|(_, c)| (*c, true)).collect();
            lines.push("Trick:".into());
            lines.extend(render_hand_cards(&cards));
        }
        for s in SeatId::all() {
            let n = table.shoe.hand(s).len();
            lines.push(format!(
                "{} ({} cards, hand pts {}):",
                table.player(s).name,
                n,
                self.hand_points[s.0 as usize]
            ));
            if s.0 == 0 {
                lines.extend(render_seat_hand(&table.shoe, s, true));
            }
        }
        lines
    }
}

fn find_two_clubs(table: &TableState) -> Option<SeatId> {
    for s in SeatId::all() {
        if table
            .shoe
            .hand(s)
            .iter()
            .any(|c| c.suit == Suit::Clubs && c.rank == Rank::Two)
        {
            return Some(s);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn must_follow_suit() {
        let mut g = HeartsGame::new();
        let mut table = TableState::new(Some(42));
        g.start_hand(&mut table);
        // Force play phase
        g.phase = Phase::Play;
        g.pass_dir = PassDir::Hold;
        // Put known cards
        table.shoe.clear_all_to_discard();
        table.shoe.add_to_hand(SeatId(0), Card::new(Suit::Hearts, Rank::Ace), true);
        table.shoe.add_to_hand(SeatId(0), Card::new(Suit::Clubs, Rank::Three), true);
        g.trick = vec![(SeatId(1), Card::new(Suit::Clubs, Rank::King))];
        table.to_act = SeatId(0);
        let legal = g.legal_plays(&table, SeatId(0));
        assert_eq!(legal, vec![Card::new(Suit::Clubs, Rank::Three)]);
        let err = g
            .apply(
                &mut table,
                SeatId(0),
                Move::PlayCard(Card::new(Suit::Hearts, Rank::Ace)),
            )
            .unwrap_err();
        assert!(err.reason.contains("not legal"));
    }
}
