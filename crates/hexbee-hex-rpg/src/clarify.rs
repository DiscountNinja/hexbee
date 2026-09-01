//! Clarify gate — kernel questions for dangerous ambiguity only.
//!
//! Not Narrator/Actor prose. Not a chat loop with the model. One ask per line max.

use crate::lexicon::LexiconKind;
use crate::organs::PurposeResult;
use crate::rules::TurnMode;
use crate::scene::SceneCoordinator;
use crate::state::WorldState;

/// Why we need to ask before touching the store or rolling a check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClarifyNeed {
    KernelWordInSpeech { word: String },
    FlavorVsSocial {
        target_label: String,
        action_hint: String,
        targets: Vec<String>,
    },
    TiedTopics { labels: Vec<String> },
    SocialNoTopic { verb_label: String },
    LowConfidence {
        summary: String,
        action_hint: Option<String>,
        targets: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClarifyResolution {
    KernelHint { word: String },
    TalkSocial {
        action_hint: String,
        targets: Vec<String>,
    },
    TalkFlavor { targets: Vec<String> },
    TopicChoice { targets: Vec<String> },
    YieldFlavor,
}

#[derive(Debug, Clone)]
pub struct ClarifyOption {
    pub number: u8,
    pub label: String,
    pub resolution: ClarifyResolution,
}

#[derive(Debug, Clone)]
pub struct ClarifyPrompt {
    pub question: String,
    pub options: Vec<ClarifyOption>,
}

#[derive(Debug, Clone)]
pub struct PendingClarify {
    pub original_line: String,
    pub prompt: ClarifyPrompt,
}

const KERNEL_WORDS: &[&str] = &[
    "help", "options", "save", "load", "quit", "exit", "reset", "restart", "actor", "next",
    "undo", "reprompt", "approve", "deny", "grants", "locks", "goal", "metrics", "stats", "dict",
];

const CONFIDENCE_THRESHOLD: f32 = 0.6;

/// Multi-word line that embeds a kernel command word (not the command itself).
pub fn kernel_word_in_speech(normalized: &str) -> Option<String> {
    let trimmed = normalized.trim();
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let first = words[0];
    if KERNEL_WORDS.contains(&first) {
        return None;
    }
    for w in KERNEL_WORDS {
        if words.iter().any(|tok| *tok == *w) {
            return Some((*w).into());
        }
    }
    None
}

/// Unmatched chatter — safe as flavor; no clarify, no Purpose organ.
pub fn is_unmatched_chatter(normalized: &str, lexicon_hit_count: usize) -> bool {
    let t = normalized.trim();
    if t.is_empty() {
        return false;
    }
    if lexicon_hit_count > 0 {
        return false;
    }
    if crate::social::wants_social_outcome(t) {
        return false;
    }
    if looks_like_act_verb(t) {
        return false;
    }
    true
}

fn looks_like_act_verb(lower: &str) -> bool {
    lower.contains("attack")
        || lower.contains("fight")
        || lower.contains("pay")
        || lower.contains("enter")
        || lower.contains("leave")
        || lower.contains("go ")
        || lower.contains("look")
        || lower.contains("inventory")
}

/// Social verb matched but no NPC topic — must not guess the check target.
pub fn needs_social_no_topic(verb_label: &str, targets: &[String]) -> bool {
    targets.is_empty() && !verb_label.is_empty()
}

/// Weak social line: verb + topic but no clear pressure — flavor vs check.
pub fn needs_flavor_vs_social(input: &str, normalized: &str) -> bool {
    if crate::social::is_pure_banter(input) {
        return false;
    }
    if crate::social::wants_social_outcome(normalized) {
        return false;
    }
    true
}

pub fn purpose_needs_clarify(purpose: &PurposeResult) -> bool {
    if purpose.mode == TurnMode::Ambiguous && purpose_is_dangerous(purpose) {
        return true;
    }
    if let Some(c) = purpose.confidence {
        if c < CONFIDENCE_THRESHOLD && purpose_is_dangerous(purpose) {
            return true;
        }
    }
    false
}

pub fn purpose_is_dangerous(purpose: &PurposeResult) -> bool {
    if purpose.social_act {
        return true;
    }
    if matches!(purpose.mode, TurnMode::Act) {
        return true;
    }
    if let Some(hint) = purpose.action_hint.as_deref() {
        if is_social_check_hint(hint) {
            return true;
        }
        if matches!(
            hint,
            "pay_scam" | "fight" | "go" | "give_password" | "tip_sally" | "charm_sally"
        ) {
            return true;
        }
    }
    false
}

fn is_social_check_hint(hint: &str) -> bool {
    matches!(
        hint,
        "challenge_dewr"
            | "call_out_scam"
            | "charm_sally"
            | "persuade_groll"
            | "impress_dremis"
    )
}

pub fn prompt_for(need: ClarifyNeed, world: &WorldState) -> ClarifyPrompt {
    match need {
        ClarifyNeed::KernelWordInSpeech { word } => ClarifyPrompt {
            question: format!(
                "Did you mean the `{word}` command, or say \"{word}\" in character?"
            ),
            options: vec![
                ClarifyOption {
                    number: 1,
                    label: format!("Run `{word}` (settings / command)"),
                    resolution: ClarifyResolution::KernelHint { word },
                },
                ClarifyOption {
                    number: 2,
                    label: "Say it in character".into(),
                    resolution: ClarifyResolution::YieldFlavor,
                },
                ClarifyOption {
                    number: 3,
                    label: "Just banter — no check".into(),
                    resolution: ClarifyResolution::YieldFlavor,
                },
            ],
        },
        ClarifyNeed::FlavorVsSocial {
            target_label,
            action_hint,
            targets,
        } => ClarifyPrompt {
            question: format!("Just talking to {target_label}, or trying to press them?"),
            options: vec![
                ClarifyOption {
                    number: 1,
                    label: "Press them (skill check)".into(),
                    resolution: ClarifyResolution::TalkSocial {
                        action_hint,
                        targets: targets.clone(),
                    },
                },
                ClarifyOption {
                    number: 2,
                    label: "Just talk in character".into(),
                    resolution: ClarifyResolution::TalkFlavor {
                        targets: targets.clone(),
                    },
                },
                ClarifyOption {
                    number: 3,
                    label: "Banter only — no check".into(),
                    resolution: ClarifyResolution::YieldFlavor,
                },
            ],
        },
        ClarifyNeed::TiedTopics { labels } => {
            let mut options: Vec<ClarifyOption> = labels
                .iter()
                .take(2)
                .enumerate()
                .map(|(i, label)| ClarifyOption {
                    number: (i + 1) as u8,
                    label: label.clone(),
                    resolution: ClarifyResolution::TopicChoice {
                        targets: vec![label_to_target(label)],
                    },
                })
                .collect();
            options.push(ClarifyOption {
                number: 3,
                label: "Just banter — no check".into(),
                resolution: ClarifyResolution::YieldFlavor,
            });
            ClarifyPrompt {
                question: "A few things match — which do you mean?".into(),
                options,
            }
        }
        ClarifyNeed::SocialNoTopic { verb_label } => {
            let present = SceneCoordinator::visible_npcs(world);
            let mut options: Vec<ClarifyOption> = present
                .iter()
                .take(2)
                .enumerate()
                .map(|(i, id)| {
                    let name = id.display_name().to_string();
                    ClarifyOption {
                        number: (i + 1) as u8,
                        label: format!("{verb_label} {name}"),
                        resolution: ClarifyResolution::TalkSocial {
                            action_hint: verb_label_to_hint(&verb_label),
                            targets: vec![npc_target_string(*id)],
                        },
                    }
                })
                .collect();
            if options.is_empty() {
                options.push(ClarifyOption {
                    number: 1,
                    label: "Someone here (say who)".into(),
                    resolution: ClarifyResolution::YieldFlavor,
                });
            }
            options.push(ClarifyOption {
                number: 3,
                label: "Just banter — no check".into(),
                resolution: ClarifyResolution::YieldFlavor,
            });
            ClarifyPrompt {
                question: format!("{verb_label} whom — and about what?"),
                options,
            }
        }
        ClarifyNeed::LowConfidence {
            summary,
            action_hint,
            targets,
        } => {
            let act_label = action_hint
                .as_deref()
                .map(|h| h.replace('_', " "))
                .unwrap_or_else(|| "that action".into());
            ClarifyPrompt {
                question: format!("Not sure I read that right — {summary}"),
                options: vec![
                    ClarifyOption {
                        number: 1,
                        label: format!("Yes — {act_label}"),
                        resolution: ClarifyResolution::TalkSocial {
                            action_hint: action_hint.unwrap_or_else(|| "talk".into()),
                            targets: targets.clone(),
                        },
                    },
                    ClarifyOption {
                        number: 2,
                        label: "Just talk in character".into(),
                        resolution: ClarifyResolution::TalkFlavor {
                            targets: targets.clone(),
                        },
                    },
                    ClarifyOption {
                        number: 3,
                        label: "Banter only — no check".into(),
                        resolution: ClarifyResolution::YieldFlavor,
                    },
                ],
            }
        }
    }
}

impl ClarifyPrompt {
    pub fn display(&self) -> String {
        let mut lines = vec![format!("❓ {}", self.question)];
        for opt in &self.options {
            lines.push(format!("{}) {}", opt.number, opt.label));
        }
        lines.join("\n")
    }

    pub fn parse_answer(&self, input: &str) -> Option<ClarifyResolution> {
        let t = input.trim().to_lowercase();
        if is_shrug(&t) {
            return Some(ClarifyResolution::YieldFlavor);
        }
        if let Ok(n) = t.parse::<u8>() {
            return self
                .options
                .iter()
                .find(|o| o.number == n)
                .map(|o| o.resolution.clone());
        }
        if t.starts_with('1') {
            return self.options.first().map(|o| o.resolution.clone());
        }
        if t.starts_with('2') {
            return self.options.get(1).map(|o| o.resolution.clone());
        }
        if t.starts_with('3') {
            return self
                .options
                .iter()
                .find(|o| o.number == 3)
                .map(|o| o.resolution.clone());
        }
        if t.contains("banter") || t.contains("flavor") || t.contains("just talk") {
            return Some(ClarifyResolution::YieldFlavor);
        }
        None
    }
}

fn is_shrug(t: &str) -> bool {
    matches!(
        t,
        "idk"
            | "dunno"
            | "whatever"
            | "sure"
            | "..."
            | "¯\\_(ツ)_/¯"
            | "shrug"
            | "skip"
            | "never mind"
            | "nevermind"
            | "nvm"
    ) || t.is_empty()
}

fn label_to_target(label: &str) -> String {
    label
        .split_whitespace()
        .next()
        .unwrap_or(label)
        .to_lowercase()
}

fn verb_label_to_hint(verb_label: &str) -> String {
    match verb_label.to_lowercase().as_str() {
        "persuade" | "convince" => "persuade_groll".into(),
        "charm" | "flirt" => "charm_sally".into(),
        "intimidate" | "threaten" => "challenge_dewr".into(),
        "impress" => "impress_dremis".into(),
        _ => "talk".into(),
    }
}

fn npc_target_string(id: crate::state::NpcId) -> String {
    match id {
        crate::state::NpcId::Dewr => "dewr".into(),
        crate::state::NpcId::Sally => "sally".into(),
        crate::state::NpcId::Groll => "groll".into(),
        crate::state::NpcId::Dremis => "dremis".into(),
        crate::state::NpcId::Goblin => "goblin".into(),
        crate::state::NpcId::Patron => "patron".into(),
    }
}

pub fn tied_topic_labels(topics: &[(String, LexiconKind)]) -> Vec<String> {
    topics
        .iter()
        .map(|(id, kind)| match kind {
            LexiconKind::Actor => id
                .strip_prefix("actor.")
                .unwrap_or(id.as_str())
                .to_string(),
            LexiconKind::Secret => id
                .strip_prefix("secret.")
                .unwrap_or(id.as_str())
                .replace('_', " "),
            _ => id.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrug_yields_flavor() {
        let p = prompt_for(
            ClarifyNeed::SocialNoTopic {
                verb_label: "persuade".into(),
            },
            &WorldState::default(),
        );
        assert!(matches!(
            p.parse_answer("whatever"),
            Some(ClarifyResolution::YieldFlavor)
        ));
    }

    #[test]
    fn kernel_word_in_multiword_line() {
        assert_eq!(
            kernel_word_in_speech("i shout options at the guard"),
            Some("options".into())
        );
        assert_eq!(kernel_word_in_speech("options"), None);
    }

    #[test]
    fn unmatched_chatter_is_cheap() {
        assert!(is_unmatched_chatter("nice weather huh", 0));
        assert!(!is_unmatched_chatter("intimidate the goblin", 0));
    }
}
