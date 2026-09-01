use hexbee_core::Config;
use hexbee_store::{Campaign, StorePatch, TranscriptBeat};

use crate::gate::{CapabilityGate, GateDecision};
use crate::intent::Intent;
// use crate::intent::IntentKind;

#[derive(Debug, Clone)]
pub enum CommandResult {
    Message(String),
    Save,
    Quit,
    Reset,
    NoOp,
}

#[derive(Debug, Clone)]
pub enum KernelCommand {
    Help,
    Options,
    Save,
    Load(String),
    Quit,
    Reset,
    Actor(String),
    Next,
    Undo,
    Reprompt,
    Approve(String),
    Deny(String),
    Grants,
    Locks,
    Goal,
    Metrics,
    Dict,
    Unknown(String),
}

impl KernelCommand {
    pub fn parse(input: &str) -> Self {
        let lower = input.trim().to_lowercase();
        let parts: Vec<&str> = lower.split_whitespace().collect();
        match parts.first().map(|s| *s) {
            Some("help") | Some("?") => Self::Help,
            Some("options") => Self::Options,
            Some("save") => Self::Save,
            Some("load") => Self::Load(input[4..].trim().to_string()),
            Some("quit") | Some("exit") | Some("q") => Self::Quit,
            Some("reset") | Some("restart") => Self::Reset,
            Some("new") if parts.get(1) == Some(&"game") => Self::Reset,
            Some("actor") => Self::Actor(
                parts.get(1).map(|s| s.to_string()).unwrap_or_default(),
            ),
            Some("next") => Self::Next,
            Some("undo") => Self::Undo,
            Some("reprompt") => Self::Reprompt,
            Some("approve") => Self::Approve(
                parts.get(1).map(|s| s.to_string()).unwrap_or_default(),
            ),
            Some("deny") => Self::Deny(parts.get(1).map(|s| s.to_string()).unwrap_or_default()),
            Some("grants") => Self::Grants,
            Some("locks") => Self::Locks,
            Some("goal") => Self::Goal,
            Some("metrics") | Some("stats") => Self::Metrics,
            Some("dict") => Self::Dict,
            _ => Self::Unknown(input.to_string()),
        }
    }
}

/// Operator help: exact kernel commands + free-form play prompt.
pub fn operator_help_text() -> String {
    "\
📖 HexBee help

Deterministic commands (exact words — always handled by the kernel):
  help / ?       this screen
  options        provider, model, actor, continue budget
  save           write campaign to disk
  load <path>    open a campaign folder (from the shell)
  actor <id>     hot-seat: speak as that sheet
  next           round-robin to the next controllable actor
  goal           print GM goals from the store
  grants / locks capability table / pending lock requests
  metrics        counters
  dict           parser vocabulary (during play)
  reset          wipe progress (or: restart / new game)
  undo           last-snapshot stub
  reprompt       resubmit last organ call
  quit / exit    save-if-dirty and leave

Anything goes (in-world play):
  Say what you do in plain language. An interpreter maps your line into a
  fixed rules catalog — look around, talk, pay, fight, persuade, leave, and
  so on. Wording can be inventive; inventing new mechanics cannot change the
  dice, inventory, or endings. Rust decides outcomes; the model only narrates.

  Examples:
    I glare at the goblin and call out his door tax scam
    tip Sally then ask about the cloaked patron
    go to storage and give the password
"
    .into()
}

pub fn handle_kernel_command(
    cmd: KernelCommand,
    campaign: &mut Campaign,
    gate: &CapabilityGate,
    config: &Config,
) -> CommandResult {
    match cmd {
        KernelCommand::Help => CommandResult::Message(operator_help_text()),
        KernelCommand::Options => CommandResult::Message(format!(
            "⚙️ Options\n  Provider: {} @ {}\n  Model: {}\n  Current actor: {}\n  Scene: {}\n  Continue budget: {}\n  Parser: type `dict` during play for aliases the hex hears.\n  Runtime: alias add <word> <kind>.<id> (in play, existing ids only)",
            config.ollama_base_url,
            "openai-compatible",
            config.ollama_model,
            campaign.state.current_actor,
            campaign.state.current_scene,
            campaign.state.continue_budget,
        )),
        KernelCommand::Save => {
            if let Err(e) = campaign.save() {
                CommandResult::Message(format!("Save failed: {e}"))
            } else {
                CommandResult::Message("💾 Campaign saved.".into())
            }
        }
        KernelCommand::Load(path) => {
            if path.is_empty() {
                CommandResult::Message("Usage: load <campaign-path>".into())
            } else {
                CommandResult::Message(format!("Use shell `load {path}` to open a campaign folder."))
            }
        }
        KernelCommand::Quit => CommandResult::Quit,
        KernelCommand::Reset => {
            campaign.reset_progress();
            if let Err(e) = campaign.save() {
                CommandResult::Message(format!("Reset failed to save: {e}"))
            } else {
                CommandResult::Reset
            }
        }
        KernelCommand::Actor(id) => {
            if id.is_empty() {
                let list: Vec<_> = campaign
                    .controllable_actors()
                    .iter()
                    .map(|a| a.id.as_str())
                    .collect();
                CommandResult::Message(format!(
                    "Controllable actors: {}",
                    list.join(", ")
                ))
            } else if campaign.actors.contains_key(&id) {
                campaign.state.current_actor = id.clone();
                campaign.dirty = true;
                CommandResult::Message(format!("🎭 Now playing as `{id}`."))
            } else {
                CommandResult::Message(format!("Unknown actor `{id}`."))
            }
        }
        KernelCommand::Next => {
            if let Some(next) = campaign.next_actor_id() {
                campaign.state.current_actor = next.clone();
                campaign.dirty = true;
                CommandResult::Message(format!("🎭 Now playing as `{next}`."))
            } else {
                CommandResult::Message("No controllable actors.".into())
            }
        }
        KernelCommand::Undo => {
            let snap = campaign.root.join("snapshots/last.json");
            if snap.exists() {
                CommandResult::Message(
                    "↩️ Undo restores last snapshot on next load (full undo in Stage 1.5)."
                        .into(),
                )
            } else {
                CommandResult::Message("No snapshot available.".into())
            }
        }
        KernelCommand::Reprompt => {
            CommandResult::Message("🔁 Reprompt: resubmit last organ call (bounded retries).".into())
        }
        KernelCommand::Approve(id) => CommandResult::Message(format!(
            "Approve `{id}` — lock release stub."
        )),
        KernelCommand::Deny(id) => {
            CommandResult::Message(format!("Denied lock `{id}`."))
        }
        KernelCommand::Grants => CommandResult::Message(gate.grants_display()),
        KernelCommand::Locks => {
            if gate.locks.pending.is_empty() {
                CommandResult::Message("No pending lock requests.".into())
            } else {
                let lines: Vec<_> = gate
                    .locks
                    .pending
                    .iter()
                    .map(|l| format!("  {} — {} {:?}", l.id, l.capability, l.payload))
                    .collect();
                CommandResult::Message(format!("Pending locks:\n{}", lines.join("\n")))
            }
        }
        KernelCommand::Goal => {
            let mut lines = vec!["📜 Goals:".to_string()];
            for g in &campaign.goals.goals {
                lines.push(format!("  • {} — {}", g.id, g.description));
            }
            CommandResult::Message(lines.join("\n"))
        }
        KernelCommand::Metrics => CommandResult::Message(
            hexbee_core::metrics_snapshot().render_text(),
        ),
        KernelCommand::Dict => CommandResult::Message(
            "📚 No active hex lexicon. Type `dict` during play.".into(),
        ),
        KernelCommand::Unknown(s) => CommandResult::Message(format!("Unknown command: {s}")),
    }
}

pub fn commit_beat(
    campaign: &mut Campaign,
    gate: &CapabilityGate,
    intent: &Intent,
    patches: &[StorePatch],
    narration: Option<String>,
    organ_trace: Vec<String>,
) -> Result<(), String> {
    match gate.check_patches(patches) {
        GateDecision::Allow => {}
        GateDecision::Deny(reason) => return Err(reason),
        GateDecision::Lock(_) => return Err("effect requires lock approval".into()),
    }
    let writable = gate.writable_paths();
    campaign
        .apply_patches(patches, &writable)
        .map_err(|e| e.to_string())?;
    campaign.append_transcript(TranscriptBeat {
        actor_id: intent.actor_id.clone(),
        intent_kind: intent.kind.as_str().into(),
        raw: intent.raw.clone(),
        narration,
        organ_trace,
    });
    Ok(())
}
