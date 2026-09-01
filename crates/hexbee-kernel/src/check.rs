//! Kernel Check slot — hex modules supply the dice math.
//!
//! The kernel owns the *shape* of a contested resolution (request → outcome).
//! A tavern hex may implement d20 + attribute vs DC; another hex may use
//! opposed scores or a Judge organ. Organs never write store patches from a Check.

use serde::{Deserialize, Serialize};

/// What a hex asks the Check slot to resolve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckRequest {
    /// Stable id / label for logs (e.g. "challenge_dewr", "persuasion").
    pub id: String,
    /// Difficulty class (or opposed target number).
    pub dc: i32,
    /// Modifier added to the roll (attribute bonus, etc.).
    pub modifier: i32,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Result of a Check — report string is for the feed; success drives Apply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckOutcome {
    pub success: bool,
    pub roll: i32,
    pub modifier: i32,
    pub total: i32,
    pub dc: i32,
    /// Human-readable line for the transcript / feed.
    pub report: String,
}

impl CheckOutcome {
    pub fn from_d20(roll: i32, modifier: i32, dc: i32, label: &str) -> Self {
        let total = roll + modifier;
        let success = total >= dc;
        let report = format!(
            "🎲 {label} — d20 {roll} {mod_sign}{modifier} = {total} vs DC {dc} → {outcome}",
            mod_sign = if modifier >= 0 { "+" } else { "" },
            outcome = if success { "success" } else { "fail" },
        );
        Self {
            success,
            roll,
            modifier,
            total,
            dc,
            report,
        }
    }
}
