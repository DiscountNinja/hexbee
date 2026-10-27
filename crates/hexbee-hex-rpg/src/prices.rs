//! Store-backed prices — engine truth, not LLM invention.
//!
//! Hexes declare what an item costs and which vendor role sells it.
//! Delivery is generic: if that vendor is present they may speak the quote;
//! otherwise the engine says there is no vendor — never invent a silent board.

use crate::state::NpcId;

/// One price the campaign store is allowed to quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceQuote {
    pub item: &'static str,
    pub gold: i32,
    /// Campaign vendor for this good (None = nowhere sells it).
    pub vendor: Option<NpcId>,
    pub note: &'static str,
}

/// Tavern hex catalog — other hexes can grow their own tables the same way.
pub const MEAL: PriceQuote = PriceQuote {
    item: "meal",
    gold: 5,
    vendor: Some(NpcId::Sally),
    note: "food or drink",
};

pub const ROOM: PriceQuote = PriceQuote {
    item: "room",
    gold: 25,
    vendor: Some(NpcId::Sally),
    note: "lodging for the night",
};

/// Match a price / wares question to store truth.
pub fn match_price_query(lower: &str) -> Option<PriceQuote> {
    let t = lower.trim();
    if !(t.contains("how much")
        || t.contains("price")
        || t.contains("cost")
        || t.contains("wares")
        || t.contains("menu")
        || t.contains("what's on")
        || t.contains("whats on"))
    {
        return None;
    }
    // Player's own gold is inventory, not a vendor quote.
    if t.contains("do i have") || t.contains("my gold") || t.contains("gold do i") {
        return None;
    }
    if t.contains("room")
        || t.contains("bed")
        || t.contains("sleep")
        || t.contains("lodging")
        || t.contains("rent")
        || t.contains("night")
    {
        return Some(ROOM);
    }
    if t.contains("food")
        || t.contains("meal")
        || t.contains("stew")
        || t.contains("bread")
        || t.contains("ale")
        || t.contains("drink")
        || t.contains("menu")
        || t.contains("wares")
        || t.contains("eat")
    {
        return Some(MEAL);
    }
    None
}

#[allow(dead_code)] // hex-facing helper for boards / logs
pub fn quote_line(q: &PriceQuote) -> String {
    format!("{}: {} gold ({})", q.item, q.gold, q.note)
}

pub fn locked_facts(q: &PriceQuote) -> Vec<String> {
    let mut facts = vec![
        format!("price_item={}", q.item),
        format!("price_gold={}", q.gold),
        format!("price_note={}", q.note),
        "quote_from_store=true".into(),
    ];
    if let Some(v) = q.vendor {
        facts.push(format!("vendor={}", v.display_name().to_lowercase()));
    }
    facts
}

/// Generic spoken fallback — store number only; no hex-specific stage business.
pub fn spoken_fallback(q: &PriceQuote, _vendor_name: &str) -> String {
    format!("*nods* \"{} is {} gold.\"", q.item, q.gold)
}

/// No seller in scene for this good.
pub fn no_vendor_line(q: &PriceQuote) -> String {
    match q.vendor {
        Some(v) => format!(
            "There's no vendor here for {} — {} isn't around.",
            q.item,
            v.display_name()
        ),
        None => format!("There's no vendor here for {}.", q.item),
    }
}

/// Named person is present but does not sell this good.
pub fn wrong_vendor_line(name: &str, q: &PriceQuote) -> String {
    format!("{} isn't selling {}.", name, q.item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_price_from_named_vendor_line() {
        let q = match_price_query("how much for a room, sally?").unwrap();
        assert_eq!(q.gold, 25);
        assert_eq!(q.item, "room");
        assert_eq!(q.vendor, Some(NpcId::Sally));
    }

    #[test]
    fn menu_is_meal() {
        let q = match_price_query("hey sally, what's on the menu?").unwrap();
        assert_eq!(q.item, "meal");
        assert_eq!(q.gold, 5);
    }

    #[test]
    fn own_gold_not_vendor() {
        assert!(match_price_query("how much gold do i have?").is_none());
    }

    #[test]
    fn no_vendor_message_is_generic() {
        let line = no_vendor_line(&ROOM);
        assert!(line.contains("no vendor"));
        assert!(line.contains("room"));
    }
}
