//! Compact ASCII card rendering.

use crate::card::Card;
use crate::deck::{CardLoc, SeatId, Shoe};

/// Four lines: top, suit row, rank row, bottom.
pub fn render_card(card: Card, face_up: bool) -> [String; 4] {
    if !face_up {
        return [
            "┌──┐".into(),
            "│? │".into(),
            "│ ?│".into(),
            "└──┘".into(),
        ];
    }
    let suit = card.suit.glyph();
    let rank = card.rank.label();
    let top_inner = if suit.chars().count() == 1 {
        format!("{suit} ")
    } else {
        suit.to_string()
    };
    let bot_inner = if rank.len() == 1 {
        format!(" {rank}")
    } else {
        rank.to_string()
    };
    [
        "┌──┐".into(),
        format!("│{top_inner}│"),
        format!("│{bot_inner}│"),
        "└──┘".into(),
    ]
}

pub fn render_hand_cards(cards: &[(Card, bool)]) -> Vec<String> {
    if cards.is_empty() {
        return vec!["(empty)".into()];
    }
    let rendered: Vec<[String; 4]> = cards
        .iter()
        .map(|(c, up)| render_card(*c, *up))
        .collect();
    let mut lines = vec![String::new(); 4];
    for card_lines in &rendered {
        for (i, line) in card_lines.iter().enumerate() {
            lines[i].push_str(line);
        }
    }
    lines
}

#[allow(dead_code)]
pub fn render_hand_from_locs(locs: &[&CardLoc]) -> Vec<String> {
    let pairs: Vec<_> = locs.iter().map(|l| (l.card, l.face_up)).collect();
    render_hand_cards(&pairs)
}

pub fn render_seat_hand(shoe: &Shoe, seat: SeatId, viewer_is_seat: bool) -> Vec<String> {
    let locs = shoe.hand_locs(seat);
    let pairs: Vec<_> = locs
        .iter()
        .map(|l| {
            let up = l.face_up || viewer_is_seat;
            (l.card, up)
        })
        .collect();
    render_hand_cards(&pairs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Rank, Suit};

    #[test]
    fn compact_frame() {
        let lines = render_card(Card::new(Suit::Spades, Rank::Ace), true);
        assert_eq!(lines[0], "┌──┐");
        assert!(lines[1].contains('♠'));
        assert!(lines[2].contains('A'));
        assert_eq!(lines[3], "└──┘");
    }

    #[test]
    fn ten_fits() {
        let lines = render_card(Card::new(Suit::Hearts, Rank::Ten), true);
        assert_eq!(lines[2], "│10│");
    }

    #[test]
    fn face_down() {
        let lines = render_card(Card::new(Suit::Clubs, Rank::King), false);
        assert!(lines[1].contains('?'));
    }
}
