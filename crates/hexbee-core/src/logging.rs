//! Tracing setup. Logs go to a file by default so the clear-screen UI stays clean.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Path of the active log file, if init ran successfully.
pub fn log_path() -> Option<&'static Path> {
    LOG_PATH.get().map(|p| p.as_path())
}

/// Initialize global tracing.
///
/// - Default filter: `hexbee=info,hexbee_rpg=debug,hexbee_llm=info,hexbee_ethics=info`
/// - Override with `RUST_LOG`
/// - File: `HEXBEE_LOG_FILE` or `logs/hexbee.log`
/// - Also mirror to stderr when `HEXBEE_LOG_STDERR=1`
pub fn init_logging() -> Result<PathBuf, String> {
    let path = std::env::var("HEXBEE_LOG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("logs/hexbee.log"));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create log dir: {e}"))?;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open log file {}: {e}", path.display()))?;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "hexbee=info,hexbee_cli=info,hexbee_core=info,hexbee_rpg=debug,hexbee_llm=info,hexbee_ethics=info",
        )
    });

    let file_layer = fmt::layer()
        .with_writer(file)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_level(true);

    let stderr = std::env::var("HEXBEE_LOG_STDERR")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    if stderr {
        let stderr_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .with_target(true);
        tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .with(stderr_layer)
            .try_init()
            .map_err(|e| format!("tracing init: {e}"))?;
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .try_init()
            .map_err(|e| format!("tracing init: {e}"))?;
    }

    let _ = LOG_PATH.set(path.clone());
    Ok(path)
}
