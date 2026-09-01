//! Lexicon — normalize + match before the Purpose organ.
//!
//! Kernel entries (commands) load first; hex entries merge from `campaign/lexicon.json`.
//! Runtime `alias add` may only point at existing canonical ids.

mod load;
mod match_route;
mod normalize;
mod types;

#[cfg(test)]
mod tests;

pub use match_route::{LexiconRoute, RouteContext};
pub use types::{Lexicon, LexiconKind};
