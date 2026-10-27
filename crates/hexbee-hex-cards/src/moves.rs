//! Shared move vocabulary and illegal-move failures.

use crate::card::Card;
use crate::deck::SeatId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Move {
    Hit,
    Stand,
    PlayCard(Card),
    PassCards(Vec<Card>),
    Discard(Vec<Card>),
    Bet(u32),
    Call,
    Fold,
    Check,
    Raise(u32),
    Ready,
}

impl Move {
    pub fn label(&self) -> String {
        match self {
            Move::Hit => "hit".into(),
            Move::Stand => "stand".into(),
            Move::PlayCard(c) => format!("play {c}"),
            Move::PassCards(cs) => format!(
                "pass {}",
                cs.iter()
                    .map(|c| c.short())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Move::Discard(cs) => {
                if cs.is_empty() {
                    "stand pat".into()
                } else {
                    format!(
                        "discard {}",
                        cs.iter()
                            .map(|c| c.short())
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                }
            }
            Move::Bet(n) => format!("bet {n}"),
            Move::Call => "call".into(),
            Move::Fold => "fold".into(),
            Move::Check => "check".into(),
            Move::Raise(n) => format!("raise {n}"),
            Move::Ready => "ready".into(),
        }
    }

    pub fn kind_key(&self) -> &'static str {
        match self {
            Move::Hit => "hit",
            Move::Stand => "stand",
            Move::PlayCard(_) => "play",
            Move::PassCards(_) => "pass",
            Move::Discard(_) => "discard",
            Move::Bet(_) => "bet",
            Move::Call => "call",
            Move::Fold => "fold",
            Move::Check => "check",
            Move::Raise(_) => "raise",
            Move::Ready => "ready",
        }
    }
}

#[derive(Debug, Clone)]
pub struct IllegalMove {
    pub reason: String,
    pub legal: Vec<Move>,
}

impl IllegalMove {
    pub fn new(reason: impl Into<String>, legal: Vec<Move>) -> Self {
        Self {
            reason: reason.into(),
            legal,
        }
    }

    pub fn display(&self) -> String {
        let opts: Vec<_> = self.legal.iter().map(|m| m.label()).collect();
        format!(
            "Illegal: {}. Legal: {}",
            self.reason,
            if opts.is_empty() {
                "(none)".into()
            } else {
                opts.join(" · ")
            }
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct ApplyEffect {
    pub messages: Vec<String>,
    pub hand_over: bool,
}

#[derive(Debug, Clone)]
pub struct ScoreDelta {
    pub seat: SeatId,
    pub delta: i32,
}
