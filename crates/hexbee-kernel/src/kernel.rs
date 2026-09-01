use hexbee_core::Config;
use hexbee_provider::LlmClient;
use hexbee_store::Campaign;
use tracing::{info, warn};

use crate::commands::{commit_beat, handle_kernel_command, CommandResult, KernelCommand};
use crate::gate::CapabilityGate;
use crate::hex::{HexModule, HexOutput};
use crate::intent::{Intent, IntentKind};
use crate::interpreter::{classify_line, shell_route, ShellRoute};
use crate::{KernelError, Result};

pub struct TurnOutput {
    pub lines: Vec<String>,
    pub quit: bool,
    pub ended: bool,
    /// Campaign progress was wiped; rebuild UI / hex in-memory state.
    pub reset: bool,
}

pub struct Kernel {
    pub config: Config,
    pub campaign: Option<Campaign>,
    pub gate: Option<CapabilityGate>,
    pub hex: Option<Box<dyn HexModule>>,
}

impl Kernel {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            campaign: None,
            gate: None,
            hex: None,
        }
    }

    pub fn has_active_hex(&self) -> bool {
        self.hex.is_some()
    }

    pub fn hex_id(&self) -> Option<&str> {
        self.hex.as_ref().map(|h| h.id())
    }

    pub fn start_hex(
        &mut self,
        hex: Box<dyn HexModule>,
        campaign: Campaign,
    ) -> Result<()> {
        let gate = CapabilityGate::new(campaign.config.clone());
        self.campaign = Some(campaign);
        self.gate = Some(gate);
        self.hex = Some(hex);
        info!(hex = self.hex_id(), "hex session started");
        Ok(())
    }

    pub fn load_campaign(&mut self, path: &str, hex: Box<dyn HexModule>) -> Result<()> {
        let campaign = Campaign::load(path)?;
        self.start_hex(hex, campaign)
    }

    pub fn run_turn(&mut self, input: &str, llm: Option<&dyn LlmClient>) -> Result<TurnOutput> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(TurnOutput {
                lines: vec![],
                quit: false,
                ended: false,
                reset: false,
            });
        }

        // Shell-level routing when no hex active.
        if self.hex.is_none() {
            return self.run_shell_turn(trimmed, llm);
        }

        let actor = self
            .campaign
            .as_ref()
            .map(|c| c.state.current_actor.clone())
            .unwrap_or_else(|| "pc".into());

        let intent = classify_line(trimmed, &actor, true);

        if intent.kind == IntentKind::Command {
            return self.run_command(&intent);
        }

        self.run_hex_turn(intent, llm)
    }

    fn run_shell_turn(&mut self, input: &str, _llm: Option<&dyn LlmClient>) -> Result<TurnOutput> {
        let lower = input.to_lowercase();
        if let Some(route) = shell_route(input, &lower) {
            match route {
                ShellRoute::PlayHex(_) => {
                    return Ok(TurnOutput {
                        lines: vec!["🎮 Starting tavern hex — use `play tavern` handler in CLI.".into()],
                        quit: false,
                        ended: false,
                        reset: false,
                    });
                }
                ShellRoute::LoadCampaign(path) => {
                    return Ok(TurnOutput {
                        lines: vec![format!("Load campaign at `{path}` via CLI bootstrap.")],
                        quit: false,
                        ended: false,
                        reset: false,
                    });
                }
                ShellRoute::KernelCommand(intent) => return self.run_command(&intent),
                ShellRoute::Chat => {
                    return Ok(TurnOutput {
                        lines: vec![
                            "🎮 Available: Tavern RPG — say Play Tavern.".into(),
                        ],
                        quit: false,
                        ended: false,
                        reset: false,
                    });
                }
            }
        }

        Ok(TurnOutput {
            lines: vec!["Say Play Tavern to begin, or `load <path>`. Type `reset` to wipe saves.".into()],
            quit: false,
            ended: false,
            reset: false,
        })
    }

    fn run_command(&mut self, intent: &Intent) -> Result<TurnOutput> {
        let cmd = KernelCommand::parse(&intent.raw);
        let Some(campaign) = self.campaign.as_mut() else {
            if matches!(cmd, KernelCommand::Quit) {
                return Ok(TurnOutput {
                    lines: vec!["👋 Bye.".into()],
                    quit: true,
                    ended: false,
                    reset: false,
                });
            }
            if matches!(cmd, KernelCommand::Help) {
                return Ok(TurnOutput {
                    lines: vec![crate::commands::operator_help_text()],
                    quit: false,
                    ended: false,
                    reset: false,
                });
            }
            if matches!(cmd, KernelCommand::Reset) {
                // No loaded session — CLI should wipe the bundled campaign.
                return Ok(TurnOutput {
                    lines: vec!["🔄 No active session — reset the bundled campaign from the shell.".into()],
                    quit: false,
                    ended: false,
                    reset: true,
                });
            }
            return Ok(TurnOutput {
                lines: vec!["No active campaign.".into()],
                quit: false,
                ended: false,
                reset: false,
            });
        };

        let gate = self
            .gate
            .as_ref()
            .cloned()
            .unwrap_or_else(|| CapabilityGate::new(campaign.config.clone()));

        if matches!(cmd, KernelCommand::Dict) {
            if let Some(hex) = self.hex.as_ref() {
                if let Some(dump) = hex.operator_dict() {
                    return Ok(TurnOutput {
                        lines: vec![dump],
                        quit: false,
                        ended: false,
                        reset: false,
                    });
                }
            }
        }

        let result = handle_kernel_command(cmd, campaign, &gate, &self.config);
        match result {
            CommandResult::Message(msg) => Ok(TurnOutput {
                lines: vec![msg],
                quit: false,
                ended: false,
                reset: false,
            }),
            CommandResult::Save => Ok(TurnOutput {
                lines: vec!["💾 Campaign saved.".into()],
                quit: false,
                ended: false,
                reset: false,
            }),
            CommandResult::Quit => {
                if campaign.dirty {
                    if let Err(e) = campaign.save() {
                        warn!(%e, "save on quit failed");
                    }
                }
                Ok(TurnOutput {
                    lines: vec!["👋 Saved and quit.".into()],
                    quit: true,
                    ended: false,
                    reset: false,
                })
            }
            CommandResult::Reset => {
                if let Some(campaign) = self.campaign.as_ref() {
                    if let Some(hex) = self.hex.as_mut() {
                        hex.reset(campaign);
                    }
                }
                Ok(TurnOutput {
                    lines: vec![
                        "🔄 Campaign reset — fresh scene, full HP, empty transcript.".into(),
                    ],
                    quit: false,
                    ended: false,
                    reset: true,
                })
            }
            CommandResult::NoOp => Ok(TurnOutput {
                lines: vec![],
                quit: false,
                ended: false,
                reset: false,
            }),
        }
    }

    fn run_hex_turn(
        &mut self,
        intent: Intent,
        llm: Option<&dyn LlmClient>,
    ) -> Result<TurnOutput> {
        let hex = self
            .hex
            .as_mut()
            .ok_or(KernelError::NoActiveHex)?;
        let campaign = self
            .campaign
            .as_mut()
            .ok_or(KernelError::NoActiveHex)?;
        let gate = self
            .gate
            .as_ref()
            .ok_or(KernelError::NoActiveHex)?
            .clone();

        let actor_id = campaign.state.current_actor.clone();
        let mut ctx = crate::hex::HexContext {
            campaign,
            gate: &gate,
            llm,
            actor_id,
        };

        let output: HexOutput = hex
            .handle_intent(&mut ctx, &intent)
            .map_err(KernelError::Hex)?;

        if !output.patches.is_empty() {
            commit_beat(
                ctx.campaign,
                ctx.gate,
                &intent,
                &output.patches,
                output.narration.clone(),
                output.organ_trace.clone(),
            )
            .map_err(KernelError::Validation)?;
        } else if output.narration.is_some() || !intent.raw.is_empty() {
            ctx.campaign.append_transcript(hexbee_store::TranscriptBeat {
                actor_id: intent.actor_id.clone(),
                intent_kind: intent.kind.as_str().into(),
                raw: intent.raw.clone(),
                narration: output.narration.clone(),
                organ_trace: output.organ_trace.clone(),
            });
        }

        if ctx.campaign.dirty {
            if let Err(e) = ctx.campaign.save() {
                warn!(%e, "auto-save failed");
            }
        }

        let ended = hex.is_over();
        Ok(TurnOutput {
            lines: output.lines,
            quit: false,
            ended,
            reset: false,
        })
    }

    pub fn scene_header(&self) -> Vec<String> {
        match (&self.hex, &self.campaign) {
            (Some(hex), Some(campaign)) => hex.scene_header(campaign),
            _ => vec![],
        }
    }

    pub fn sticky_header(&self, include_intro: bool) -> Vec<String> {
        match (&self.hex, &self.campaign) {
            (Some(hex), Some(campaign)) => hex.sticky_header(campaign, include_intro),
            _ => vec![],
        }
    }

    pub fn status_footer(&self) -> String {
        match (&self.hex, &self.campaign) {
            (Some(hex), Some(campaign)) => hex.status_footer(campaign),
            _ => String::new(),
        }
    }

    pub fn player_display_name(&self) -> String {
        self.hex
            .as_ref()
            .map(|h| h.player_name())
            .unwrap_or_default()
    }

    pub fn scene_fingerprint(&self) -> u64 {
        self.hex.as_ref().map(|h| h.scene_fingerprint()).unwrap_or(0)
    }

    pub fn scene_paint_fingerprint(&self) -> u64 {
        self.hex
            .as_ref()
            .map(|h| h.scene_paint_fingerprint())
            .unwrap_or(0)
    }

    pub fn scene_change_lines(
        &mut self,
        llm: Option<&dyn LlmClient>,
        player_line: Option<&str>,
    ) -> Vec<String> {
        self.hex
            .as_mut()
            .map(|h| h.scene_change_lines(llm, player_line))
            .unwrap_or_default()
    }

    pub fn scene_arrival_lines(
        &mut self,
        llm: Option<&dyn LlmClient>,
        player_line: Option<&str>,
    ) -> Vec<String> {
        self.hex
            .as_mut()
            .map(|h| h.scene_arrival_lines(llm, player_line))
            .unwrap_or_default()
    }

    pub fn is_resumed_session(&self) -> bool {
        match (&self.hex, &self.campaign) {
            (Some(hex), Some(campaign)) => hex.is_resumed_session(campaign),
            _ => false,
        }
    }

    pub fn refresh_living(
        &mut self,
        llm: Option<&dyn LlmClient>,
        player_line: Option<&str>,
    ) -> String {
        self.scene_change_lines(llm, player_line).join("\n")
    }

    /// Seed scene cast without living prose.
    pub fn prepare_scene(&mut self, llm: Option<&dyn LlmClient>) {
        if let Some(hex) = self.hex.as_mut() {
            hex.prepare_scene(llm);
        }
    }

    pub fn hex_is_over(&self) -> bool {
        self.hex.as_ref().map(|h| h.is_over()).unwrap_or(false)
    }

    pub fn end_session(&mut self) {
        self.campaign = None;
        self.hex = None;
        self.gate = None;
    }

    pub fn save_if_dirty(&mut self) {
        if let Some(c) = self.campaign.as_mut() {
            if c.dirty {
                let _ = c.save();
            }
        }
    }

    /// True when the PC still needs a chosen display name / chargen.
    pub fn needs_player_name(&self) -> bool {
        self.needs_chargen()
    }

    /// True when character creator should run.
    pub fn needs_chargen(&self) -> bool {
        let Some(campaign) = self.campaign.as_ref() else {
            return false;
        };
        if let Some(hex) = self.hex.as_ref() {
            return hex.needs_chargen(campaign);
        }
        if let Some(pc) = campaign.actors.get("pc") {
            return is_default_display_name(&pc.name);
        }
        true
    }

    /// Apply a PC display name to sheet + in-memory hex and mark dirty.
    pub fn apply_player_name(&mut self, name: &str) -> Result<()> {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.chars().all(|c| !c.is_alphanumeric()) {
            return Err(KernelError::Validation(
                "name must include a letter or number".into(),
            ));
        }
        if let Some(campaign) = self.campaign.as_mut() {
            if let Some(pc) = campaign.actors.get_mut("pc") {
                pc.name = trimmed.to_string();
            }
            campaign.dirty = true;
        }
        if let Some(hex) = self.hex.as_mut() {
            hex.set_player_name(trimmed);
        }
        self.save_if_dirty();
        Ok(())
    }

    /// Apply a finished character build (abilities + skills) to sheet + hex.
    pub fn apply_character_build(&mut self, build_json: serde_json::Value) -> Result<()> {
        if let Some(campaign) = self.campaign.as_mut() {
            if let Some(pc) = campaign.actors.get_mut("pc") {
                // Preserve hp/gold/weapon in stats while merging build fields.
                if let Some(obj) = pc.stats.as_object_mut() {
                    if let Some(incoming) = build_json.as_object() {
                        for (k, v) in incoming {
                            obj.insert(k.clone(), v.clone());
                        }
                    }
                } else {
                    pc.stats = build_json.clone();
                }
            }
            campaign.dirty = true;
        }
        if let Some(hex) = self.hex.as_mut() {
            hex.apply_character_build(&build_json)
                .map_err(KernelError::Validation)?;
        }
        self.save_if_dirty();
        Ok(())
    }
}

fn is_default_display_name(name: &str) -> bool {
    let n = name.trim();
    n.is_empty() || n.eq_ignore_ascii_case("you") || n.eq_ignore_ascii_case("traveler")
}
