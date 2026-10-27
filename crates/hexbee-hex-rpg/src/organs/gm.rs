use hexbee_store::GoalsDoc;
// use hexbee_store::{Campaign, GoalsDoc};

/// GM organ — tracks module goals from store, no narration.
pub struct GmOrgan;

impl GmOrgan {
    /// Next incomplete goal, for the narrator to steer toward without completing.
    pub fn next_open(goals: &GoalsDoc, runtime: &serde_json::Value) -> String {
        for g in &goals.goals {
            if !check_complete(&g.complete_when, runtime) {
                return g.description.clone();
            }
        }
        "The story's goals are complete.".into()
    }

    pub fn goal_status(goals: &GoalsDoc, runtime: &serde_json::Value) -> Vec<String> {
        let mut lines = vec!["📜 GM goal status:".into()];
        for g in &goals.goals {
            let done = check_complete(&g.complete_when, runtime);
            let mark = if done { "✓" } else { "○" };
            lines.push(format!("  {mark} {} — {}", g.id, g.description));
        }
        lines
    }

    // pub fn pressure(_campaign: &Campaign) -> Option<String> {
    //     None
    // }
}

fn check_complete(when: &str, runtime: &serde_json::Value) -> bool {
    if when.starts_with("runtime.") {
        let key = &when[8..];
        if key.contains("==") {
            let parts: Vec<&str> = key.split("==").map(|s| s.trim()).collect();
            if parts.len() == 2 {
                return runtime.get(parts[0]).and_then(|v| v.as_str()) == Some(parts[1]);
            }
        }
        return runtime
            .get(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    }
    false
}
