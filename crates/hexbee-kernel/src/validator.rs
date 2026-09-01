use crate::intent::IntentKind;
use hexbee_store::StorePatch;

pub struct Validator;

impl Validator {
    pub fn validate_judge_score(score: i32) -> Result<(), String> {
        if !(0..=10).contains(&score) {
            return Err(format!("judge score out of range: {score}"));
        }
        Ok(())
    }

    pub fn validate_intent_kind(s: &str) -> Result<IntentKind, String> {
        match s {
            "command" => Ok(IntentKind::Command),
            "query" => Ok(IntentKind::Query),
            "speech" | "talk" => Ok(IntentKind::Speech),
            "action" | "act" => Ok(IntentKind::Action),
            "attack" => Ok(IntentKind::Attack),
            "ambiguous" => Ok(IntentKind::Ambiguous),
            other => Err(format!("unknown intent kind: {other}")),
        }
    }

    /// Validate Purpose organ mode strings (talk|act|query|ambiguous).
    pub fn validate_purpose_mode(s: &str) -> Result<(), String> {
        match s.trim().to_lowercase().as_str() {
            "talk" | "act" | "query" | "ambiguous" => Ok(()),
            other => Err(format!("unknown purpose mode: {other}")),
        }
    }

    pub fn validate_patches(patches: &[StorePatch], forbidden: &[&str]) -> Result<(), String> {
        for patch in patches {
            for f in forbidden {
                if patch.path.contains(f) {
                    return Err(format!("forbidden patch path: {}", patch.path));
                }
            }
        }
        Ok(())
    }
}
