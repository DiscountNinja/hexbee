//! Shoe: draw pile, cards in play, discard, reshuffle.

use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;

use crate::card::{standard_deck, Card};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeatId(pub u8);

impl SeatId {
    pub const COUNT: usize = 4;
    pub fn all() -> [SeatId; 4] {
        [SeatId(0), SeatId(1), SeatId(2), SeatId(3)]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardLoc {
    pub card: Card,
    pub seat: Option<SeatId>,
    /// Face-up for observers (dealer hole may be false).
    pub face_up: bool,
}

#[derive(Debug, Clone)]
pub struct Shoe {
    pub draw_pile: Vec<Card>,
    pub in_play: Vec<CardLoc>,
    pub discard: Vec<Card>,
    rng: StdRng,
}

impl Shoe {
    pub fn fresh(seed: Option<u64>) -> Self {
        let mut rng = match seed {
            Some(s) => StdRng::seed_from_u64(s),
            None => StdRng::from_entropy(),
        };
        let mut draw_pile = standard_deck();
        draw_pile.shuffle(&mut rng);
        Self {
            draw_pile,
            in_play: Vec::new(),
            discard: Vec::new(),
            rng,
        }
    }

    pub fn ensure_cards(&mut self, n: usize) {
        if self.draw_pile.len() >= n {
            return;
        }
        if self.discard.is_empty() {
            return;
        }
        self.draw_pile.append(&mut self.discard);
        self.draw_pile.shuffle(&mut self.rng);
    }

    pub fn draw_one(&mut self) -> Option<Card> {
        self.ensure_cards(1);
        self.draw_pile.pop()
    }

    pub fn deal_to(&mut self, seat: SeatId, n: usize, face_up: bool) -> Vec<Card> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            self.ensure_cards(1);
            if let Some(card) = self.draw_pile.pop() {
                self.in_play.push(CardLoc {
                    card,
                    seat: Some(seat),
                    face_up,
                });
                out.push(card);
            }
        }
        out
    }

    pub fn hand(&self, seat: SeatId) -> Vec<Card> {
        self.in_play
            .iter()
            .filter(|c| c.seat == Some(seat))
            .map(|c| c.card)
            .collect()
    }

    pub fn hand_locs(&self, seat: SeatId) -> Vec<&CardLoc> {
        self.in_play
            .iter()
            .filter(|c| c.seat == Some(seat))
            .collect()
    }

    pub fn take_from_hand(&mut self, seat: SeatId, card: Card) -> Option<Card> {
        let idx = self
            .in_play
            .iter()
            .position(|c| c.seat == Some(seat) && c.card == card)?;
        let loc = self.in_play.remove(idx);
        Some(loc.card)
    }

    pub fn discard_card(&mut self, card: Card) {
        self.discard.push(card);
    }

    pub fn discard_from_hand(&mut self, seat: SeatId, card: Card) -> bool {
        if let Some(c) = self.take_from_hand(seat, card) {
            self.discard.push(c);
            true
        } else {
            false
        }
    }

    pub fn set_face_up(&mut self, seat: SeatId, face_up: bool) {
        for loc in &mut self.in_play {
            if loc.seat == Some(seat) {
                loc.face_up = face_up;
            }
        }
    }

    pub fn clear_hands_to_discard(&mut self) {
        let mut keep = Vec::new();
        for loc in self.in_play.drain(..) {
            if loc.seat.is_some() {
                self.discard.push(loc.card);
            } else {
                keep.push(loc);
            }
        }
        self.in_play = keep;
    }

    pub fn clear_all_to_discard(&mut self) {
        for loc in self.in_play.drain(..) {
            self.discard.push(loc.card);
        }
    }

    pub fn add_to_hand(&mut self, seat: SeatId, card: Card, face_up: bool) {
        self.in_play.push(CardLoc {
            card,
            seat: Some(seat),
            face_up,
        });
    }

    pub fn gen_range(&mut self, range: std::ops::RangeInclusive<usize>) -> usize {
        self.rng.gen_range(range)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reshuffle_when_draw_empty() {
        let mut shoe = Shoe::fresh(Some(1));
        let total = shoe.draw_pile.len();
        for _ in 0..total {
            let c = shoe.draw_one().unwrap();
            shoe.discard_card(c);
        }
        assert!(shoe.draw_pile.is_empty());
        shoe.ensure_cards(1);
        assert!(!shoe.draw_pile.is_empty());
        assert!(shoe.discard.is_empty());
    }
}
