//! Playing cards: suit, rank, card.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Suit {
    Spades,
    Hearts,
    Diamonds,
    Clubs,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs];

    pub fn glyph(self) -> &'static str {
        match self {
            Suit::Spades => "♠",
            Suit::Hearts => "♥",
            Suit::Diamonds => "♦",
            Suit::Clubs => "♣",
        }
    }

    pub fn is_red(self) -> bool {
        matches!(self, Suit::Hearts | Suit::Diamonds)
    }
}

impl fmt::Display for Suit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.glyph())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Rank {
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    Jack = 11,
    Queen = 12,
    King = 13,
    Ace = 14,
}

impl Rank {
    pub const ALL: [Rank; 13] = [
        Rank::Ace,
        Rank::Two,
        Rank::Three,
        Rank::Four,
        Rank::Five,
        Rank::Six,
        Rank::Seven,
        Rank::Eight,
        Rank::Nine,
        Rank::Ten,
        Rank::Jack,
        Rank::Queen,
        Rank::King,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Rank::Ace => "A",
            Rank::Two => "2",
            Rank::Three => "3",
            Rank::Four => "4",
            Rank::Five => "5",
            Rank::Six => "6",
            Rank::Seven => "7",
            Rank::Eight => "8",
            Rank::Nine => "9",
            Rank::Ten => "10",
            Rank::Jack => "J",
            Rank::Queen => "Q",
            Rank::King => "K",
        }
    }

    /// Blackjack pip value (Ace counted separately as soft/hard).
    pub fn blackjack_pip(self) -> u8 {
        match self {
            Rank::Ace => 11,
            Rank::Jack | Rank::Queen | Rank::King => 10,
            r => r as u8,
        }
    }

    pub fn from_label(s: &str) -> Option<Rank> {
        match s.trim().to_uppercase().as_str() {
            "A" | "ACE" => Some(Rank::Ace),
            "2" => Some(Rank::Two),
            "3" => Some(Rank::Three),
            "4" => Some(Rank::Four),
            "5" => Some(Rank::Five),
            "6" => Some(Rank::Six),
            "7" => Some(Rank::Seven),
            "8" => Some(Rank::Eight),
            "9" => Some(Rank::Nine),
            "10" | "T" => Some(Rank::Ten),
            "J" | "JACK" => Some(Rank::Jack),
            "Q" | "QUEEN" => Some(Rank::Queen),
            "K" | "KING" => Some(Rank::King),
            _ => None,
        }
    }
}

impl fmt::Display for Rank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Card {
    pub suit: Suit,
    pub rank: Rank,
}

impl Card {
    pub fn new(suit: Suit, rank: Rank) -> Self {
        Self { suit, rank }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        // Forms: "A♠", "AS", "A of spades", "10H", "QH"
        let upper = s.to_uppercase();
        if let Some(card) = parse_compact(&upper) {
            return Some(card);
        }
        let parts: Vec<_> = upper.split_whitespace().collect();
        if parts.len() >= 3 && parts[1] == "OF" {
            let rank = Rank::from_label(parts[0])?;
            let suit = match parts[2].as_ref() {
                "SPADES" | "SPADE" | "S" => Suit::Spades,
                "HEARTS" | "HEART" | "H" => Suit::Hearts,
                "DIAMONDS" | "DIAMOND" | "D" => Suit::Diamonds,
                "CLUBS" | "CLUB" | "C" => Suit::Clubs,
                _ => return None,
            };
            return Some(Card::new(suit, rank));
        }
        None
    }

    pub fn short(self) -> String {
        format!("{}{}", self.rank.label(), self.suit.glyph())
    }
}

fn parse_compact(s: &str) -> Option<Card> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    // Unicode suits
    for (glyph, suit) in [
        ('♠', Suit::Spades),
        ('♥', Suit::Hearts),
        ('♦', Suit::Diamonds),
        ('♣', Suit::Clubs),
    ] {
        if let Some(rest) = s.strip_suffix(glyph) {
            let rank = Rank::from_label(rest.trim())?;
            return Some(Card::new(suit, rank));
        }
    }
    // Letter suits
    let (rank_s, suit) = match bytes[bytes.len() - 1] as char {
        'S' => (&s[..s.len() - 1], Suit::Spades),
        'H' => (&s[..s.len() - 1], Suit::Hearts),
        'D' => (&s[..s.len() - 1], Suit::Diamonds),
        'C' => (&s[..s.len() - 1], Suit::Clubs),
        _ => return None,
    };
    let rank = Rank::from_label(rank_s.trim())?;
    Some(Card::new(suit, rank))
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short())
    }
}

/// Standard 52-card deck order.
pub fn standard_deck() -> Vec<Card> {
    let mut out = Vec::with_capacity(52);
    for &suit in &Suit::ALL {
        for &rank in &Rank::ALL {
            out.push(Card::new(suit, rank));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ace_spades() {
        assert_eq!(
            Card::parse("AS"),
            Some(Card::new(Suit::Spades, Rank::Ace))
        );
        assert_eq!(
            Card::parse("A♠"),
            Some(Card::new(Suit::Spades, Rank::Ace))
        );
    }
}
