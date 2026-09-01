use hexbee_provider::LlmClient;
use hexbee_store::{Campaign, StorePatch};

use crate::gate::CapabilityGate;

/// Context passed to a hex module each turn.
pub struct HexContext<'a> {
    pub campaign: &'a mut Campaign,
    pub gate: &'a CapabilityGate,
    pub llm: Option<&'a dyn LlmClient>,
    pub actor_id: String,
}

/// Output from a hex turn.
#[derive(Debug, Clone, Default)]
pub struct HexOutput {
    pub lines: Vec<String>,
    pub patches: Vec<StorePatch>,
    pub narration: Option<String>,
    pub organ_trace: Vec<String>,
    pub ended: bool,
}

pub type HexResult = Result<HexOutput, String>;

/// A pluggable hex module (design doc §16).
pub trait HexModule: Send {
    fn id(&self) -> &str;
    fn handle_intent(
        &mut self,
        ctx: &mut HexContext<'_>,
        intent: &crate::intent::Intent,
    ) -> HexResult;
    fn scene_header(&self, campaign: &Campaign) -> Vec<String>;
    fn sticky_header(&self, campaign: &Campaign, _include_intro: bool) -> Vec<String> {
        self.scene_header(campaign)
    }
    fn status_footer(&self, _campaign: &Campaign) -> String {
        String::new()
    }
    fn scene_fingerprint(&self) -> u64 {
        0
    }
    /// When to repaint transition + arrival (location / major room shift — not ambient seed).
    fn scene_paint_fingerprint(&self) -> u64 {
        self.scene_fingerprint()
    }
    /// Transition + arrival lines when location or cast changes between turns.
    fn scene_change_lines(
        &mut self,
        _llm: Option<&dyn LlmClient>,
        _player_line: Option<&str>,
    ) -> Vec<String> {
        Vec::new()
    }
    fn scene_arrival_lines(
        &mut self,
        _llm: Option<&dyn LlmClient>,
        _player_line: Option<&str>,
    ) -> Vec<String> {
        Vec::new()
    }
    fn is_resumed_session(&self, _campaign: &Campaign) -> bool {
        false
    }
    fn refresh_living(
        &mut self,
        llm: Option<&dyn LlmClient>,
        player_line: Option<&str>,
    ) -> String {
        self.scene_change_lines(llm, player_line).join("\n")
    }
    /// Seed ambient / presence without returning display prose.
    fn prepare_scene(&mut self, _llm: Option<&dyn LlmClient>) {}
    fn is_over(&self) -> bool;
    /// Rebuild in-memory play state from the (already reset) campaign.
    fn reset(&mut self, campaign: &Campaign) {
        let _ = campaign;
    }
    /// Set the main PC display name (new game / rename).
    fn set_player_name(&mut self, _name: &str) {}
    fn player_name(&self) -> String {
        "You".into()
    }
    /// Apply a full character build (abilities + skills) after chargen.
    fn apply_character_build(&mut self, _build_json: &serde_json::Value) -> Result<(), String> {
        Ok(())
    }
    /// True when the hex wants the CLI to run character creation.
    fn needs_chargen(&self, _campaign: &Campaign) -> bool {
        false
    }
    fn operator_dict(&self) -> Option<String> {
        None
    }
}
