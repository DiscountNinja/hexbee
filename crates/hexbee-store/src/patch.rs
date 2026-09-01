use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A validated mutation to a campaign store path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorePatch {
    pub path: String,
    pub value: Value,
}

impl StorePatch {
    pub fn new(path: impl Into<String>, value: Value) -> Self {
        Self {
            path: path.into(),
            value,
        }
    }

    pub fn set_bool(path: impl Into<String>, v: bool) -> Self {
        Self::new(path, Value::Bool(v))
    }

    pub fn set_i64(path: impl Into<String>, v: i64) -> Self {
        Self::new(path, serde_json::json!(v))
    }

    pub fn set_str(path: impl Into<String>, v: impl Into<String>) -> Self {
        Self::new(path, Value::String(v.into()))
    }
}

/// Check whether `path` matches a writable pattern like `actors.*.inventory`.
///
/// A pattern ending in `.*` also matches the bare prefix (`runtime.*` → `runtime`),
/// so whole-object replacements of a granted subtree are allowed.
pub fn path_matches_pattern(path: &str, pattern: &str) -> bool {
    let path_parts: Vec<&str> = path.split('.').collect();
    let pat_parts: Vec<&str> = pattern.split('.').collect();
    if path_parts.len() == pat_parts.len() {
        return path_parts
            .iter()
            .zip(pat_parts.iter())
            .all(|(p, q)| *q == "*" || p == q);
    }
    // `runtime.*` grants whole-object write to `runtime`.
    if pat_parts.len() == path_parts.len() + 1 && pat_parts.last() == Some(&"*") {
        return path_parts
            .iter()
            .zip(pat_parts.iter())
            .all(|(p, q)| *q == "*" || p == q);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_match() {
        assert!(path_matches_pattern(
            "actors.pc.inventory",
            "actors.*.inventory"
        ));
        assert!(!path_matches_pattern(
            "actors.pc.stats.might",
            "actors.*.inventory"
        ));
        assert!(path_matches_pattern("state.current_scene", "state.*"));
        assert!(path_matches_pattern("runtime", "runtime.*"));
        assert!(path_matches_pattern("state", "state.*"));
        assert!(!path_matches_pattern("runtime", "state.*"));
    }
}
