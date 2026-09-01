use crate::organs::PurposeResult;
use crate::rules::{match_rules_normalized, RuleHit, TurnMode};
use crate::secrets::SecretsCatalog;
use crate::state::WorldState;

use super::types::{Lexicon, LexiconHit, LexiconKind, VerbClass};

#[derive(Debug, Clone)]
pub struct RouteContext<'a> {
    pub world: &'a WorldState,
    pub secrets: &'a SecretsCatalog,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LexiconRoute {
    Command(RuleHit),
    TalkSocial {
        topic_id: String,
        topic_kind: LexiconKind,
        verb_id: String,
        targets: Vec<String>,
        action_hint: String,
    },
    TalkFlavor {
        topic_id: String,
        topic_kind: LexiconKind,
        targets: Vec<String>,
    },
    Clarify(crate::clarify::ClarifyNeed),
    Dict,
}

impl Lexicon {
    pub fn route(&self, raw_input: &str, ctx: &RouteContext<'_>) -> Option<LexiconRoute> {
        let normalized = self.normalize(raw_input);
        if normalized.is_empty() {
            return None;
        }

        let hits = self.match_hits(&normalized);

        if hits.iter().any(|h| h.entry.id == "command.dict") {
            return Some(LexiconRoute::Dict);
        }

        if let Some(cmd) = self.command_route(&normalized, &hits) {
            return Some(LexiconRoute::Command(cmd));
        }

        let topics = self.collect_topics(&hits, ctx);
        let verbs = self.collect_verbs(&hits);

        if topics.len() > 1 {
            let top_priority = topics[0].1.kind.topic_priority();
            let tied: Vec<_> = topics
                .iter()
                .filter(|(_, e)| e.kind.topic_priority() == top_priority)
                .collect();
            if tied.len() > 1 {
                let labels = crate::clarify::tied_topic_labels(
                    &tied
                        .iter()
                        .map(|(id, e)| (id.clone(), e.kind))
                        .collect::<Vec<_>>(),
                );
                return Some(LexiconRoute::Clarify(
                    crate::clarify::ClarifyNeed::TiedTopics { labels },
                ));
            }
        }

        let topic = topics.first().map(|(id, e)| (id.clone(), e.clone()));
        let social_verb = verbs.iter().find(|v| v.verb_class == Some(VerbClass::Social));
        let flavor_verb = verbs.iter().find(|v| v.verb_class == Some(VerbClass::Flavor));

        if let (Some((topic_id, topic_entry)), Some(verb)) = (&topic, social_verb) {
            if topic_entry.kind == LexiconKind::Secret {
                if !self.secret_exists_for_holder(&topic_id, topic_entry.holder.as_deref(), ctx) {
                    return None;
                }
            }
            let targets = topic_targets(topic_id, topic_entry.kind);
            let action_hint = verb_action_hint(&verb.id)?;
            return Some(LexiconRoute::TalkSocial {
                topic_id: topic_id.clone(),
                topic_kind: topic_entry.kind,
                verb_id: verb.id.clone(),
                targets,
                action_hint,
            });
        }

        if let Some((topic_id, topic_entry)) = &topic {
            if topic_entry.kind == LexiconKind::Secret
                && !self.secret_exists_for_holder(&topic_id, topic_entry.holder.as_deref(), ctx)
            {
                return None;
            }
            let targets = topic_targets(topic_id, topic_entry.kind);
            if matches!(topic_entry.kind, LexiconKind::Actor | LexiconKind::Secret)
                || flavor_verb.is_some()
            {
                return Some(LexiconRoute::TalkFlavor {
                    topic_id: topic_id.clone(),
                    topic_kind: topic_entry.kind,
                    targets,
                });
            }
        }

        if let Some(verb) = social_verb {
            if let Some(action_hint) = verb_action_hint(&verb.id) {
                let targets = crate::slang::extract_npc_targets(&normalized);
                if crate::clarify::needs_social_no_topic(&verb_label(&verb.id), &targets) {
                    return Some(LexiconRoute::Clarify(
                        crate::clarify::ClarifyNeed::SocialNoTopic {
                            verb_label: verb_label(&verb.id),
                        },
                    ));
                }
                return Some(LexiconRoute::TalkSocial {
                    topic_id: verb.id.clone(),
                    topic_kind: LexiconKind::Verb,
                    verb_id: verb.id.clone(),
                    targets,
                    action_hint,
                });
            }
        }

        None
    }

    fn command_route(&self, normalized: &str, hits: &[LexiconHit]) -> Option<RuleHit> {
        if hits.iter().any(|h| h.entry.kind == LexiconKind::Command) {
            return match_rules_normalized(normalized);
        }
        None
    }

    fn collect_topics<'a>(
        &'a self,
        hits: &'a [LexiconHit],
        ctx: &RouteContext<'_>,
    ) -> Vec<(String, super::types::LexiconEntry)> {
        let mut topics: Vec<(String, super::types::LexiconEntry)> = Vec::new();
        for hit in hits {
            let kind = hit.entry.kind;
            if !matches!(
                kind,
                LexiconKind::Actor
                    | LexiconKind::Secret
                    | LexiconKind::Prop
                    | LexiconKind::Item
                    | LexiconKind::Scene
                    | LexiconKind::Flag
                    | LexiconKind::Stat
            ) {
                continue;
            }
            if kind == LexiconKind::Secret
                && !self.secret_exists_for_holder(
                    &hit.entry.id,
                    hit.entry.holder.as_deref(),
                    ctx,
                )
            {
                continue;
            }
            let id = hit.entry.id.clone();
            if topics.iter().any(|(existing, _)| existing == &id) {
                continue;
            }
            topics.push((id, hit.entry.clone()));
        }
        topics.sort_by_key(|(_, e)| std::cmp::Reverse(e.kind.topic_priority()));
        topics
    }

    fn collect_verbs<'a>(&'a self, hits: &'a [LexiconHit]) -> Vec<super::types::LexiconEntry> {
        hits.iter()
            .filter(|h| h.entry.kind == LexiconKind::Verb)
            .map(|h| h.entry.clone())
            .collect()
    }

    fn secret_exists_for_holder(
        &self,
        secret_entry_id: &str,
        holder: Option<&str>,
        ctx: &RouteContext<'_>,
    ) -> bool {
        let secret_id = secret_entry_id
            .strip_prefix("secret.")
            .unwrap_or(secret_entry_id);
        let Some(secret) = ctx.secrets.secret(secret_id) else {
            return false;
        };
        if let Some(h) = holder {
            return secret.holder == h;
        }
        secret.holder == secret_holder_from_id(secret_id)
    }
}

impl LexiconRoute {
    pub fn to_purpose_result(&self) -> PurposeResult {
        match self {
            Self::Command(hit) => PurposeResult::from_rule(hit),
            Self::TalkSocial {
                targets,
                action_hint,
                ..
            } => PurposeResult {
                mode: TurnMode::Talk,
                targets: targets.clone(),
                summary: Some("social".into()),
                action_hint: Some(action_hint.clone()),
                social_act: false,
                confidence: None,
            },
            Self::TalkFlavor { targets, .. } => PurposeResult {
                mode: TurnMode::Talk,
                targets: targets.clone(),
                summary: Some("talk".into()),
                action_hint: Some("talk".into()),
                social_act: false,
                confidence: None,
            },
            Self::Dict => PurposeResult {
                mode: TurnMode::Query,
                targets: vec![],
                summary: Some("dict".into()),
                action_hint: Some("dict".into()),
                social_act: false,
                confidence: Some(1.0),
            },
            Self::Clarify(_) => PurposeResult::ambiguous("clarify"),
        }
    }
}

fn topic_targets(topic_id: &str, kind: LexiconKind) -> Vec<String> {
    match kind {
        LexiconKind::Actor => {
            if let Some(npc) = topic_id.strip_prefix("actor.") {
                vec![npc.into()]
            } else {
                vec![]
            }
        }
        LexiconKind::Secret => {
            topic_id
                .strip_prefix("secret.")
                .map(secret_holder_from_id)
                .filter(|h| !h.is_empty())
                .map(|h| vec![h.into()])
                .unwrap_or_default()
        }
        _ => vec![],
    }
}

fn verb_action_hint(verb_id: &str) -> Option<String> {
    verb_id
        .strip_prefix("verb.")
        .map(str::to_string)
        .or_else(|| match verb_id {
            "challenge_dewr" => Some("challenge_dewr".into()),
            "call_out_scam" => Some("call_out_scam".into()),
            "charm_sally" => Some("charm_sally".into()),
            "persuade_groll" => Some("persuade_groll".into()),
            "impress_dremis" => Some("impress_dremis".into()),
            _ => None,
        })
}

fn verb_label(verb_id: &str) -> String {
    verb_id
        .strip_prefix("verb.")
        .unwrap_or(verb_id)
        .replace('_', " ")
}

fn secret_holder_from_id(secret_id: &str) -> &'static str {
    match secret_id {
        "groll_storage_password" => "groll",
        "sally_charm" => "sally",
        "dewr_intimidate" | "dewr_scam" => "dewr",
        "dremis_amulet" => "dremis",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_over_actor_when_both_match() {
        let lex = Lexicon::bundled();
        let world = WorldState::default();
        let secrets = SecretsCatalog::bundled();
        let ctx = RouteContext {
            world: &world,
            secrets: &secrets,
        };
        let route = lex
            .route("groll password", &ctx)
            .expect("should route secret bind");
        assert!(matches!(
            route,
            LexiconRoute::TalkFlavor { .. } | LexiconRoute::TalkSocial { .. }
        ));
    }
}
