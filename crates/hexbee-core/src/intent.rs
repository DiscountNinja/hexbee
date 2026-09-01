use serde::{Deserialize, Serialize};

/// Top-level user intent routed by the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    /// Free-form conversation (stub in v1).
    Chat,
    /// System / shell command request (stub in v1).
    Command,
    /// Modify settings (stub in v1).
    Settings,
    /// Start or continue the tavern RPG.
    PlayRpg,
}
