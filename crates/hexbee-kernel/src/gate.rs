use hexbee_store::{path_matches_pattern, CapabilityKind, HexConfig, StorePatch};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockRequest {
    pub id: String,
    pub capability: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct LockQueue {
    pub pending: Vec<LockRequest>,
    next_id: u32,
}

impl LockQueue {
    pub fn propose(&mut self, capability: impl Into<String>, payload: serde_json::Value) -> String {
        self.next_id += 1;
        let id = format!("lock-{:04}", self.next_id);
        self.pending.push(LockRequest {
            id: id.clone(),
            capability: capability.into(),
            payload,
        });
        id
    }

    pub fn approve(&mut self, id: &str) -> Option<LockRequest> {
        if let Some(pos) = self.pending.iter().position(|r| r.id == id) {
            Some(self.pending.remove(pos))
        } else {
            None
        }
    }

    pub fn deny(&mut self, id: &str) -> bool {
        if let Some(pos) = self.pending.iter().position(|r| r.id == id) {
            self.pending.remove(pos);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Allow,
    Deny(String),
    Lock(LockRequest),
}

#[derive(Debug, Clone)]
pub struct CapabilityGate {
    pub config: HexConfig,
    pub locks: LockQueue,
}

impl CapabilityGate {
    pub fn new(config: HexConfig) -> Self {
        Self {
            config,
            locks: LockQueue::default(),
        }
    }

    pub fn check_patches(&self, patches: &[StorePatch]) -> GateDecision {
        let writable = self.writable_paths();
        for patch in patches {
            if !writable.iter().any(|p| path_matches_pattern(&patch.path, p)) {
                warn!(path = %patch.path, "capability gate denied patch");
                return GateDecision::Deny(format!("path not granted: {}", patch.path));
            }
        }
        GateDecision::Allow
    }

    pub fn check_capability(&mut self, capability: &str, payload: serde_json::Value) -> GateDecision {
        let Some(entry) = self.config.grants.iter().find(|g| g.capability == capability) else {
            warn!(%capability, "capability denied (not in grant table)");
            return GateDecision::Deny(format!("capability not granted: {capability}"));
        };
        match entry.kind {
            CapabilityKind::Grant | CapabilityKind::Program => GateDecision::Allow,
            CapabilityKind::Deny => {
                warn!(%capability, "capability denied");
                GateDecision::Deny(format!("capability denied: {capability}"))
            }
            CapabilityKind::Lock => {
                let id = self.locks.propose(capability, payload);
                info!(%id, %capability, "lock request queued");
                GateDecision::Lock(LockRequest {
                    id,
                    capability: capability.into(),
                    payload: serde_json::Value::Null,
                })
            }
        }
    }

    pub fn writable_paths(&self) -> Vec<String> {
        self.config
            .grants
            .iter()
            .find(|g| g.capability == "store.patch")
            .map(|g| g.writable_paths.clone())
            .unwrap_or_default()
    }

    pub fn grants_display(&self) -> String {
        let mut lines = vec!["Active grants:".into()];
        for g in &self.config.grants {
            lines.push(format!(
                "  {} — {:?}{}",
                g.capability,
                g.kind,
                if g.writable_paths.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", g.writable_paths.join(", "))
                }
            ));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hexbee_store::HexConfig;

    #[test]
    fn deny_forbidden_patch() {
        let gate = CapabilityGate::new(HexConfig::default());
        let decision = gate.check_patches(&[StorePatch::set_i64("actors.pc.stats.might", 99)]);
        assert!(matches!(decision, GateDecision::Deny(_)));
    }

    #[test]
    fn allow_state_patch() {
        let gate = CapabilityGate::new(HexConfig::default());
        let decision = gate.check_patches(&[StorePatch::set_str("state.current_scene", "hall")]);
        assert_eq!(decision, GateDecision::Allow);
    }

    #[test]
    fn allow_whole_runtime_blob() {
        let gate = CapabilityGate::new(HexConfig::default());
        let decision = gate.check_patches(&[StorePatch::new(
            "runtime",
            serde_json::json!({"location": "Outside"}),
        )]);
        assert_eq!(decision, GateDecision::Allow);
    }

    #[test]
    fn mail_send_denied() {
        let mut gate = CapabilityGate::new(HexConfig::default());
        let decision = gate.check_capability("mail.send", serde_json::json!({}));
        assert!(matches!(decision, GateDecision::Deny(_)));
    }
}
