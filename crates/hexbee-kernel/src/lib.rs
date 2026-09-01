//! HexBee kernel — interpreter, router, capability gate, commands.

mod check;
mod commands;
mod error;
mod gate;
mod hex;
mod interpreter;
mod intent;
mod kernel;
mod terminal;
mod validator;

pub use check::{CheckOutcome, CheckRequest};
pub use commands::{operator_help_text, CommandResult, KernelCommand};
pub use error::{KernelError, Result};
pub use gate::{CapabilityGate, GateDecision, LockQueue, LockRequest};
pub use hex::{HexContext, HexModule, HexOutput, HexResult};
pub use interpreter::{classify_line, interpret_with_llm, looks_like_games_inquiry, shell_route, ShellRoute};
pub use intent::{Intent, IntentKind};
pub use kernel::Kernel;
pub use terminal::{term_height, term_width, FrameLayout};
pub use validator::Validator;

pub use hexbee_core::{Config, MetricsSnapshot, metrics_snapshot};
