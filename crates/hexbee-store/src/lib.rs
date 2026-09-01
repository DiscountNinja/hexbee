//! Campaign file store — source of truth on disk.

mod actor;
mod campaign;
mod config;
mod goals;
mod patch;
mod scene;
mod transcript;

pub use actor::ActorSheet;
pub use campaign::{Campaign, CampaignState, Result as StoreResult, StoreError};
pub use config::{CapabilityKind, GrantEntry, HexConfig};
pub use goals::{GoalEntry, GoalsDoc};
pub use patch::{path_matches_pattern, StorePatch};
pub use scene::SceneDef;
pub use transcript::{TranscriptBeat, TranscriptLog};
