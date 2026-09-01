use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalEntry {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub complete_when: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalsDoc {
    pub goals: Vec<GoalEntry>,
}
