//! Shared table state.

use std::collections::HashMap;

use crate::deck::{SeatId, Shoe};
use crate::player::{Player, SeatKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameKind {
    TwentyOne,
    Hearts,
    DrawPoker,
}

impl GameKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "21" | "blackjack" | "twenty_one" | "twenty-one" | "bj" => Some(GameKind::TwentyOne),
            "hearts" => Some(GameKind::Hearts),
            "poker" | "draw" | "five-card" | "5card" | "draw_poker" => Some(GameKind::DrawPoker),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            GameKind::TwentyOne => "twenty_one",
            GameKind::Hearts => "hearts",
            GameKind::DrawPoker => "draw_poker",
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            GameKind::TwentyOne => "21",
            GameKind::Hearts => "Hearts",
            GameKind::DrawPoker => "Five-card draw",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TableState {
    pub shoe: Shoe,
    pub players: [Player; 4],
    pub scores: HashMap<String, i32>,
    pub to_act: SeatId,
    pub game: Option<GameKind>,
    /// Per-game scratch (serde_json for flexibility inside engines).
    pub scratch: serde_json::Value,
}

impl TableState {
    pub fn new(seed: Option<u64>) -> Self {
        let mut scores = HashMap::new();
        scores.insert("you".into(), 100);
        scores.insert("ai1".into(), 100);
        scores.insert("ai2".into(), 100);
        scores.insert("ai3".into(), 100);
        Self {
            shoe: Shoe::fresh(seed),
            players: Player::default_table(),
            scores,
            to_act: SeatId(0),
            game: None,
            scratch: serde_json::json!({}),
        }
    }

    pub fn player(&self, seat: SeatId) -> &Player {
        &self.players[seat.0 as usize]
    }

    pub fn player_mut(&mut self, seat: SeatId) -> &mut Player {
        &mut self.players[seat.0 as usize]
    }

    pub fn score_of(&self, seat: SeatId) -> i32 {
        let id = self.player(seat).id;
        *self.scores.get(id).unwrap_or(&0)
    }

    pub fn add_score(&mut self, seat: SeatId, delta: i32) {
        let id = self.player(seat).id.to_string();
        let e = self.scores.entry(id).or_insert(0);
        *e += delta;
    }

    pub fn setup_players_for(&mut self, kind: GameKind) {
        self.players = match kind {
            GameKind::TwentyOne => Player::for_twenty_one(),
            _ => Player::default_table(),
        };
        // Rename AI3 when not dealer
        if kind != GameKind::TwentyOne {
            self.players[3].kind = SeatKind::Ai;
            self.players[3].name = "River".into();
        }
    }

    pub fn scores_line(&self) -> String {
        SeatId::all()
            .iter()
            .map(|s| {
                let p = self.player(*s);
                format!("{}:{}", p.name, self.score_of(*s))
            })
            .collect::<Vec<_>>()
            .join(" · ")
    }
}
