//! Per-game table engines.

mod draw_poker;
mod hearts;
mod twenty_one;

pub use draw_poker::{hand_rank, DrawPokerGame};
pub use hearts::HeartsGame;
pub use twenty_one::{blackjack_total, dealer_should_hit, TwentyOneGame};

use crate::deck::SeatId;
use crate::moves::{ApplyEffect, IllegalMove, Move, ScoreDelta};
use crate::table::TableState;

pub trait TableGame: Send {
    fn name(&self) -> &str;
    fn start_hand(&mut self, table: &mut TableState) -> Vec<String>;
    fn legal_moves(&self, table: &TableState, seat: SeatId) -> Vec<Move>;
    fn apply(
        &mut self,
        table: &mut TableState,
        seat: SeatId,
        mv: Move,
    ) -> Result<ApplyEffect, IllegalMove>;
    fn is_hand_over(&self, table: &TableState) -> bool;
    fn settle(&mut self, table: &mut TableState) -> Vec<ScoreDelta>;
    fn prompt_hint(&self, table: &TableState, seat: SeatId) -> String;
    fn render_public(&self, table: &TableState) -> Vec<String>;
}
