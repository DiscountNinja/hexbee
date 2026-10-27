//! Seat identities at the table.

use crate::deck::SeatId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatKind {
    Human,
    Ai,
    Dealer,
}

#[derive(Debug, Clone)]
pub struct Player {
    pub seat: SeatId,
    pub id: &'static str,
    pub name: String,
    pub kind: SeatKind,
}

impl Player {
    pub fn default_table() -> [Player; 4] {
        [
            Player {
                seat: SeatId(0),
                id: "you",
                name: "You".into(),
                kind: SeatKind::Human,
            },
            Player {
                seat: SeatId(1),
                id: "ai1",
                name: "Ace".into(),
                kind: SeatKind::Ai,
            },
            Player {
                seat: SeatId(2),
                id: "ai2",
                name: "Bluff".into(),
                kind: SeatKind::Ai,
            },
            Player {
                seat: SeatId(3),
                id: "ai3",
                name: "Dealer".into(),
                kind: SeatKind::Ai,
            },
        ]
    }

    pub fn for_twenty_one() -> [Player; 4] {
        let mut p = Self::default_table();
        p[3].kind = SeatKind::Dealer;
        p[3].name = "Dealer".into();
        p
    }
}
