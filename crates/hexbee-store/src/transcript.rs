use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptBeat {
    pub actor_id: String,
    pub intent_kind: String,
    pub raw: String,
    #[serde(default)]
    pub narration: Option<String>,
    #[serde(default)]
    pub organ_trace: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranscriptLog {
    pub beats: Vec<TranscriptBeat>,
}

impl TranscriptLog {
    pub fn append(&mut self, beat: TranscriptBeat) {
        self.beats.push(beat);
    }
}
