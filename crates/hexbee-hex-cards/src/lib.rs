//! Card games hex module (21, Hearts, five-card draw).

mod ai;
mod card;
mod deck;
mod game;
mod module;
mod moves;
mod player;
mod render;
mod rules;
mod table;

pub use card::{standard_deck, Card, Rank, Suit};
pub use deck::{SeatId, Shoe};
pub use game::{CardsGame, GameEvent};
pub use module::{bundled_campaign_dir, CardsHex};
pub use moves::{IllegalMove, Move};
pub use render::{render_card, render_hand_cards};
pub use rules::{blackjack_total, dealer_should_hit, hand_rank, TableGame};
pub use table::{GameKind, TableState};
