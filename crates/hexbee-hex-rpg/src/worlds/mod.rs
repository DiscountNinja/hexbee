//! Three worlds — keep them separate or the module will write itself.
//!
//! | Layer | What | May expand at runtime? | Who writes |
//! |---|---|---|---|
//! | **Canon** | Sheets, secrets, inventory, goals, quest truth | Only via authored apply / store patches | Program after check/act |
//! | **Presentation** | Atmosphere, NPC voice, stammer after a fail | Every turn (prose only) | Actor / Narrator organs |
//! | **Situation** | Attitude, heat, time, declared runtime flags | Every turn, declared knobs only | Program via [`situation::SituationKnob`] |
//!
//! Models never expand canon. Models never mint secrets or DCs. Situation never
//! invents new flag names — see [`situation::SituationFlag`].

pub mod canon;
pub mod presentation;
pub mod situation;

pub use canon::{apply_canon, apply_check_success, note_authored_check_fail, CanonPatch};
pub use situation::{Situation, SituationFlag, SituationKnob};
