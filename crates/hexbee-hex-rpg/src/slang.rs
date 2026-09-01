//! Player vocabulary shim — delegates to [`crate::lexicon::Lexicon`].

use crate::lexicon::Lexicon;

/// Canonical NPC keys scanned in player lines (after normalize).
pub const NPC_KEYS: &[&str] = &["dewr", "sally", "groll", "dremis", "goblin", "patron"];

/// Lowercase, strip punctuation, collapse spaces, expand aliases.
pub fn normalize(input: &str) -> String {
    Lexicon::bundled().normalize(input)
}

/// Resolve a single token to a canonical NPC key.
pub fn resolve_npc_alias(token: &str) -> Option<&'static str> {
    let t = token.trim().to_lowercase();
    let expanded = Lexicon::bundled().normalize(t.as_str());
    let bare = expanded.split_whitespace().next().unwrap_or(&expanded);
    match bare {
        "goblin" => Some("goblin"),
        "dewr" => Some("dewr"),
        "sally" => Some("sally"),
        "groll" => Some("groll"),
        "dremis" => Some("dremis"),
        "patron" => Some("patron"),
        _ => None,
    }
}

pub fn extract_npc_targets(input: &str) -> Vec<String> {
    let lower = normalize(input);
    let mut out = Vec::new();
    for name in NPC_KEYS {
        if lower.contains(name) && !out.iter().any(|n: &String| n == name) {
            out.push((*name).into());
        }
    }
    out
}

pub fn mentions_npc(input: &str, canonical: &str) -> bool {
    normalize(input).contains(canonical)
}
