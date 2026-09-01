use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tracing::info;

use hexbee_core::metrics;
use hexbee_provider::LlmClient;
use crate::organs::{
    format_narrator_line, format_npc_display, generate_action_narration, generate_ambient_cast,
    generate_npc_line, generate_scene_narration, generate_scene_transition,
    heuristic_interpret_plan, parse_staged_line, purpose_classify,
    purpose_heuristic, split_player_line, finalize_npc_line_with_prior, format_player_feed,
    template_scene_transition, wants_leave_command,
    ActionNarrationRequest, ActionPlan, ActionTag, DialogueRequest, PurposeResult, SceneRequest,
    SceneTransitionRequest,
};

use crate::cards::{card_for, load_builtin_cards, CharacterCard};
use crate::checks::{dc_for, resolve_d20_check, CheckKind};
use crate::combat::{resolve_attack, Combatant};
use crate::moves::{apply_generic_outcome, pick_generic_outcome};
use crate::parse::PlayerAction;
use crate::bind::{
    act_unbound_line, detect_missing_entity, is_entity_directed_action, look_scene_line,
    missing_entity_line, obvious_interactables, room_presence_answer, survey_location,
    talk_unbound_line,
};
use crate::clarify::{
    self, ClarifyNeed, ClarifyPrompt, ClarifyResolution, PendingClarify,
};
use crate::lexicon::{Lexicon, LexiconRoute, RouteContext};
use crate::scene::{SceneCoordinator, TalkFocus};
use crate::secrets::{BindBlock, SecretsCatalog, TalkBind};
use crate::skills::Skill;
use crate::state::{Location, NpcId, WorldState};
use crate::world_doc::{is_soft_search_line, WorldDoc};
use crate::rules::{match_rules_normalized, wants_sneak_past, TurnMode};
use crate::weapons::Weapon;
use crate::worlds::{apply_check_success, note_authored_check_fail};

enum RouteOutcome {
    Clarify(ClarifyPrompt),
    FlavorTalk(Vec<String>),
    Purpose(PurposeResult),
    Store(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneEnd {
    Victory,
    Death,
    BadEnding,
}

#[derive(Debug, Clone)]
pub enum GameEvent {
    Narration(String),
    Ended(SceneEnd),
}

/// Playable tavern scene.
pub struct TavernGame {
    pub world: WorldState,
    pub cards: Vec<CharacterCard>,
    /// Who is here / who we're talking to — engine truth for presence.
    pub scene: SceneCoordinator,
    secrets: SecretsCatalog,
    lexicon: Lexicon,
    pending_clarify: Option<PendingClarify>,
    rng: StdRng,
    ended: Option<SceneEnd>,
    /// Short rolling beat memory for tiny-model context (not a transcript dump).
    memory: Vec<String>,
    /// Location at the start of the current turn (for scene-change paint).
    pub(crate) turn_start_location: Location,
    /// Authored flavor from `campaign/world.md` (reloaded with campaign; not in runtime save).
    world_doc: WorldDoc,
}

impl TavernGame {
    pub fn new(seed: Option<u64>) -> Self {
        let seed = seed.unwrap_or_else(|| rand::thread_rng().gen());
        info!(seed, "tavern game started");
        Self {
            world: WorldState::default(),
            cards: load_builtin_cards(),
            scene: SceneCoordinator::new(),
            secrets: SecretsCatalog::bundled(),
            lexicon: Lexicon::bundled(),
            pending_clarify: None,
            rng: StdRng::seed_from_u64(seed),
            ended: None,
            memory: Vec::new(),
            turn_start_location: Location::Outside,
            world_doc: WorldDoc::bundled(),
        }
    }

    pub fn set_world_doc(&mut self, doc: WorldDoc) {
        self.world_doc = doc;
    }

    /// Look / watch / search — reveal one matching soft flag and paint it (LLM or fallback).
    async fn discover_soft_beat(
        &mut self,
        input: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Option<Vec<GameEvent>> {
        let flag = self
            .world_doc
            .match_undiscovered_search(&self.world, &self.scene, input)?;
        let id = flag.id.clone();
        let text = flag.text.clone();
        self.world_doc.discover(&mut self.world, &id);
        Some(
            self.say(
                llm,
                &format!(
                    "Player searches or observes. Reveal this in one short narrator beat (no dialogue): {text}"
                ),
                vec![format!("soft_discovered={id}")],
                Some(input),
                text,
            )
            .await,
        )
    }

    /// Act on a soft detail the player already noticed — dynamic narrator beat.
    async fn interact_soft_beat(
        &mut self,
        input: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Option<Vec<GameEvent>> {
        let flag = self
            .world_doc
            .match_discovered_interaction(&self.world, input)?;
        let id = flag.id.clone();
        let text = flag.text.clone();
        Some(
            self.say(
                llm,
                &format!(
                    "Player interacts with something they already noticed ({text}). Paint one short reactive beat (actions only, no quoted speech)."
                ),
                vec![format!("soft_interact={id}")],
                Some(input),
                text,
            )
            .await,
        )
    }

    /// Changes when location or who/what is visible changes — CLI header sync.
    pub fn scene_fingerprint(&self) -> u64 {
        self.scene.fingerprint(&self.world)
    }

    pub fn scene_paint_fingerprint(&self) -> u64 {
        self.scene.paint_fingerprint(&self.world)
    }

    fn memory_line(&self) -> String {
        self.memory.join(" | ")
    }

    /// Actor memory — drop other NPC dialogue beats to prevent cross-character bleed.
    fn memory_line_for_actor(&self, actor_name: &str) -> String {
        let actor = actor_name.trim().to_lowercase();
        let others: Vec<String> = SceneCoordinator::visible_npcs(&self.world)
            .iter()
            .map(|id| id.display_name().to_lowercase())
            .filter(|n| n != &actor && !actor.contains(n) && !n.contains(&actor))
            .collect();
        let filtered: Vec<&str> = self
            .memory
            .iter()
            .filter(|beat| {
                let b = beat.to_lowercase();
                !others.iter().any(|other| {
                    b.contains(other) && (b.contains('💬') || b.contains(" said "))
                })
            })
            .map(String::as_str)
            .collect();
        if filtered.is_empty() {
            "(nothing yet)".into()
        } else {
            filtered.join(" | ")
        }
    }

    fn line_should_use_talk(
        &self,
        input: &str,
        purpose: &PurposeResult,
    ) -> bool {
        if purpose.mode == TurnMode::Talk || purpose.action_hint.as_deref() == Some("talk") {
            return true;
        }
        let normalized = self.lexicon.normalize(input);
        crate::rules::is_talk_routing_line(&normalized)
    }

    fn push_memory(&mut self, beat: impl Into<String>) {
        let beat = beat.into();
        if beat.is_empty() {
            return;
        }
        let lower = beat.to_lowercase();
        if lower.starts_with("💬 you:")
            || lower.starts_with("🎬 you:")
            || lower.contains("that could mean a few things")
            || lower.contains("(nothing yet)")
            || lower.contains("❌ failure")
            || lower.contains("❌ blocked")
        {
            return;
        }
        let short: String = beat.chars().take(120).collect();
        self.memory.push(short);
        while self.memory.len() > 3 {
            self.memory.remove(0);
        }
    }

    /// Dump merged kernel + hex lexicon for `dict` / options.
    pub fn lexicon_dump(&self) -> String {
        self.lexicon.dump_text()
    }

    /// `alias add hat prop.ugly_hat` — runtime synonym for an existing id only.
    pub fn try_operator_alias(&mut self, input: &str) -> Option<String> {
        let trimmed = input.trim();
        let lower = trimmed.to_lowercase();
        if !lower.starts_with("alias ") {
            return None;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 4 || !parts[1].eq_ignore_ascii_case("add") {
            return Some(
                "Usage: alias add <word> <kind>.<id>  — e.g. alias add hat prop.ugly_hat".into(),
            );
        }
        let alias = parts[2];
        let target = parts[3];
        let (kind_str, id) = target.split_once('.')?;
        let kind = crate::lexicon::LexiconKind::parse(kind_str)?;
        let canonical = format!("{kind_str}.{id}");
        match self.lexicon.add_runtime_alias(alias, kind, &canonical) {
            Ok(()) => Some(format!("✅ Alias `{alias}` → `{canonical}` (runtime).")),
            Err(e) => Some(format!("❌ {e}")),
        }
    }

    pub fn is_over(&self) -> bool {
        self.ended.is_some()
    }

    pub fn ending(&self) -> Option<SceneEnd> {
        self.ended
    }

    pub fn intro(&self) -> String {
        "🌙 Muddy street. Lantern light spills from a crooked tavern door.\n\
A goblin blocks the way, knife at his belt, eyes on your coin purse.\n\
✨ You've heard a rumor: somewhere here is an amulet worth finding."
            .to_string()
    }

    /// Display name for the traveler (POV / status).
    pub fn player_display_name(&self) -> &str {
        let n = self.world.player.name.trim();
        if n.is_empty() {
            "You"
        } else {
            n
        }
    }

    pub fn set_player_name(&mut self, name: &str) {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            self.world.player.name = trimmed.to_string();
        }
    }

    pub fn set_character_build(&mut self, build: crate::skills::CharacterBuild) {
        self.world.player.build = build;
    }

    /// Sparse, non-spoiling facts for the current location (engine truth).
    pub fn look_facts(&self) -> Vec<String> {
        let mut facts = self.scene.snapshot(&self.world).facts;
        facts.extend(self.world_doc.discovered_facts(&self.world));
        facts
    }

    fn visible_npc_ids(&self) -> Vec<NpcId> {
        SceneCoordinator::visible_npcs(&self.world)
    }

    pub fn look_text(&self) -> String {
        format!(
            "{}\n{}",
            self.scene.look_prose(&self.world),
            self.status_line()
        )
    }

    /// Fill ambient cast once per location (AI invents; coordinator owns cards).
    async fn ensure_ambient_cast(&mut self, llm: Option<&dyn LlmClient>) {
        let loc = self.world.location;
        if self.scene.has_ambient_seeded(loc) {
            return;
        }
        // Storage stays sparse — dread / Dremis only.
        if loc == Location::StorageRoom {
            self.scene.mark_ambient_seeded(loc);
            return;
        }
        let present = self.scene.snapshot(&self.world).present_csv();
        let entries = if let Some(llm) = llm {
            generate_ambient_cast(llm, loc.display_name(), &present).await
        } else {
            Vec::new()
        };
        let cards: Vec<CharacterCard> = if entries.is_empty() {
            SceneCoordinator::fallback_ambient(loc)
        } else {
            entries
                .into_iter()
                .map(|e| {
                    CharacterCard::ambient(e.id, e.name, e.temperament, e.description, e.voice)
                })
                .collect()
        };
        if cards.is_empty() {
            self.scene.mark_ambient_seeded(loc);
        } else {
            info!(location = ?loc, count = cards.len(), "ambient cast registered");
            self.scene.register_ambient(loc, cards);
        }
    }

    /// Engine-owned roster for the CLI header (not LLM text).
    pub fn present_roster_lines(&self) -> Vec<String> {
        let mut lines = vec!["Present:".to_string()];
        let mut any = false;
        for id in self.visible_npc_ids() {
            if let Some(card) = card_for(&self.cards, id) {
                lines.push(card.present_line());
                any = true;
            }
        }
        for a in self.scene.ambient_here(self.world.location) {
            lines.push(a.card.present_line());
            any = true;
        }
        for line in self.world_doc.discovered_present_lines(&self.world) {
            lines.push(line);
            any = true;
        }
        let props = obvious_interactables(&self.world);
        if !props.is_empty() {
            lines.push("Here:".to_string());
            for p in props {
                lines.push(format!("  {p}"));
            }
            any = true;
        }
        if !any {
            lines.push("  (nothing notable)".into());
        }
        lines
    }

    pub async fn narrate_here(
        &mut self,
        llm: Option<&dyn LlmClient>,
        player_line: Option<&str>,
    ) -> String {
        self.ensure_ambient_cast(llm).await;
        let cards = self.visible_description_tags();
        let req = SceneRequest {
            location: self.world.location.display_name().into(),
            known_goal: "Find the amulet".into(),
            visible_cards: cards,
            visible_facts: self.look_facts(),
            player_line: player_line.map(str::to_string),
            memory: self.memory_line(),
            player_name: self.player_display_name().into(),
        };
        let line = if let Some(llm) = llm {
            generate_scene_narration(llm, &req).await.line
        } else {
            self.scene.look_prose(&self.world)
        };
        format_narrator_line(&line)
    }

    /// Seed ambient cast without producing display prose (Present roster is enough).
    pub async fn prepare_scene(&mut self, llm: Option<&dyn LlmClient>) {
        self.ensure_ambient_cast(llm).await;
    }

    /// Arrival description for the current location (resume / load).
    pub async fn describe_scene_arrival(
        &self,
        llm: Option<&dyn LlmClient>,
        player_line: Option<&str>,
    ) -> Vec<String> {
        vec![self.scene_arrival_line(llm, player_line).await]
    }

    async fn scene_arrival_line(
        &self,
        llm: Option<&dyn LlmClient>,
        player_line: Option<&str>,
    ) -> String {
        let to_name = self.world.location.display_name().to_string();
        let arrival_req = SceneRequest {
            location: to_name,
            known_goal: "Find the amulet".into(),
            visible_cards: self.visible_description_tags(),
            visible_facts: self.look_facts(),
            player_line: player_line.map(str::to_string),
            memory: self.memory_line(),
            player_name: self.player_display_name().into(),
        };
        let arrival = if let Some(llm) = llm {
            generate_scene_narration(llm, &arrival_req).await.line
        } else {
            self.scene.look_prose(&self.world)
        };
        format_narrator_line(&arrival)
    }

    /// Transition sentence + arrival description when the scene fingerprint moves.
    pub async fn describe_scene_change(
        &self,
        llm: Option<&dyn LlmClient>,
        player_line: Option<&str>,
        from_location: Location,
    ) -> Vec<String> {
        let to = self.world.location;
        let from_name = from_location.display_name().to_string();
        let to_name = to.display_name().to_string();

        let transition_req = SceneTransitionRequest {
            from_place: from_name,
            to_place: to_name.clone(),
            known_goal: "Find the amulet".into(),
            player_line: player_line.map(str::to_string),
            memory: self.memory_line(),
            player_name: self.player_display_name().into(),
        };
        let transition = if let Some(llm) = llm {
            generate_scene_transition(llm, &transition_req).await.line
        } else {
            template_scene_transition(&transition_req)
        };

        let arrival = self.scene_arrival_line(llm, player_line).await;
        vec![
            format_narrator_line(&transition),
            arrival,
        ]
    }

    /// Description tags for quest + ambient faces here.
    fn visible_description_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .visible_npc_ids()
            .iter()
            .filter_map(|id| card_for(&self.cards, *id).map(|c| c.description_tag()))
            .collect();
        for a in self.scene.ambient_here(self.world.location) {
            tags.push(a.card.description_tag());
        }
        tags
    }

    /// Resolve talk target from plan npc tag and/or names mentioned in the line.
    fn talk_request_from_input(
        &self,
        npc: Option<&str>,
        player_line: &str,
    ) -> Option<crate::scene::TalkRequest> {
        if let Some(req) = SceneCoordinator::parse_talk_request(npc) {
            return Some(req);
        }
        let lower = crate::slang::normalize(player_line);
        for a in self.scene.ambient_here(self.world.location) {
            let name = a.card.name.to_lowercase();
            if lower.contains(&a.id) || (!name.is_empty() && lower.contains(&name)) {
                return Some(crate::scene::TalkRequest::Name(a.id.clone()));
            }
            // Match a distinctive word from multi-word ambient names ("tired carter").
            for word in name.split_whitespace() {
                if word.len() >= 4 && lower.contains(word) {
                    return Some(crate::scene::TalkRequest::Name(a.id.clone()));
                }
            }
        }
        for id in SceneCoordinator::visible_npcs(&self.world) {
            let n = id.display_name().to_lowercase();
            if lower.contains(&n) {
                return Some(crate::scene::TalkRequest::Quest(id));
            }
        }
        if crate::slang::mentions_npc(&lower, "goblin") {
            return Some(crate::scene::TalkRequest::Quest(NpcId::Goblin));
        }
        None
    }

    /// AI describes a rules-resolved beat. Outcomes are locked; model only paints.
    /// When locked outcomes encode success/failure, prepends an explicit outcome line.
    async fn say(
        &self,
        llm: Option<&dyn LlmClient>,
        action_card: &str,
        locked_outcome: Vec<String>,
        player_line: Option<&str>,
        fallback: String,
    ) -> Vec<GameEvent> {
        let line = if let Some(llm) = llm {
            let req = ActionNarrationRequest {
                action_card: action_card.into(),
                locked_outcome: locked_outcome.clone(),
                location: self.world.location.display_name().into(),
                visible_cards: self.visible_description_tags(),
                visible_facts: self.look_facts().into_iter().take(8).collect(),
                player_line: player_line.map(str::to_string),
                fallback: fallback.clone(),
                memory: self.memory_line(),
                player_name: self.player_display_name().into(),
            };
            generate_action_narration(llm, &req).await.line
        } else {
            fallback.clone()
        };

        let mut events = Vec::new();
        if let Some((success, detail)) = infer_action_outcome(&locked_outcome, &fallback) {
            events.push(GameEvent::Narration(format_action_outcome(success, &detail)));
        }
        events.push(GameEvent::Narration(format_narrator_line(&line)));
        events
    }

    pub fn status_line(&self) -> String {
        self.status_line_for(self.player_display_name())
    }

    /// Status bar with an explicit acting character name (hot-seat aware).
    pub fn status_line_for(&self, actor_name: &str) -> String {
        let name = actor_name.trim();
        let name = if name.is_empty() {
            self.player_display_name()
        } else {
            name
        };
        format!(
            "🎭 {}  |  ❤️ HP {}/{}  |  🪙 {} gold  |  🗡️ {}{}",
            name,
            self.world.player.hp,
            self.world.player.max_hp,
            self.world.player.gold,
            self.world.player.weapon.display_name(),
            if self.world.player.has_amulet {
                "  |  ✨ amulet"
            } else {
                ""
            }
        )
    }

    pub fn help_text() -> &'static str {
        "Say what you do in plain language. Look around. Talk to whoever is here.\n\
You've heard of an amulet worth finding. The rest you discover by playing.\n\
Shortcuts: look · inventory · help · leave (walk away from this place)"
    }

    /// Handle one player line: rules → Purpose → Query/Talk/Act → legality → Check → Apply → paint.
    pub async fn handle_line(&mut self, input: &str, llm: Option<&dyn LlmClient>) -> Vec<GameEvent> {
        if self.ended.is_some() {
            return vec![GameEvent::Narration(
                "The scene has ended. Start again with Play Tavern.".into(),
            )];
        }

        metrics::inc_rpg_turn();
        if let Some(msg) = self.try_operator_alias(input) {
            return vec![GameEvent::Narration(msg)];
        }

        self.turn_start_location = self.world.location;

        if let Some(pending) = self.pending_clarify.take() {
            return self.resolve_clarify_answer(input, pending, llm).await;
        }

        let route = self.route_line(input, llm).await;
        let mut events = match route {
            RouteOutcome::Store(line) => vec![GameEvent::Narration(line)],
            RouteOutcome::Clarify(prompt) => {
                let display = prompt.display();
                self.pending_clarify = Some(PendingClarify {
                    original_line: input.to_string(),
                    prompt,
                });
                vec![GameEvent::Narration(display)]
            }
            RouteOutcome::FlavorTalk(targets) => {
                self.handle_flavor_talk(input, &targets, llm).await
            }
            RouteOutcome::Purpose(purpose) => {
                self.dispatch_purpose(input, &purpose, llm).await
            }
        };

        info!(
            %input,
            clarified = self.pending_clarify.is_some(),
            "rpg turn routed"
        );

        if events.is_empty() {
            events.push(GameEvent::Narration(
                "❌ Failure — nothing came of that. Try something that fits what's in front of you."
                    .into(),
            ));
        }

        if self.world.player.hp <= 0 && self.ended.is_none() {
            self.ended = Some(SceneEnd::Death);
            metrics::inc_rpg_death();
            info!("scene end: death");
            events.push(GameEvent::Narration(
                "💀 Your hit points fall to zero. Darkness takes you. The scene ends.".into(),
            ));
            events.push(GameEvent::Ended(SceneEnd::Death));
        }

        if let Some(GameEvent::Narration(first)) = events.iter().find(|e| {
            matches!(e, GameEvent::Narration(s) if !s.contains("HP ") && s.len() > 8)
        }) {
            let summary: String = first.chars().take(100).collect();
            self.push_memory(summary);
        }

        events
    }

    async fn route_line(&self, input: &str, llm: Option<&dyn LlmClient>) -> RouteOutcome {
        let ctx = RouteContext {
            world: &self.world,
            secrets: &self.secrets,
        };
        let normalized = self.lexicon.normalize(input);

        if let Some(word) = clarify::kernel_word_in_speech(&normalized) {
            return RouteOutcome::Clarify(clarify::prompt_for(
                ClarifyNeed::KernelWordInSpeech { word },
                &self.world,
            ));
        }

        if self
            .lexicon
            .match_hits(&normalized)
            .iter()
            .any(|h| h.entry.id == "command.dict")
        {
            return RouteOutcome::Purpose(LexiconRoute::Dict.to_purpose_result());
        }

        if let Some(name) = detect_missing_entity(&normalized, &self.world, &self.scene, &self.world_doc) {
            if is_entity_directed_action(&normalized) {
                return RouteOutcome::Store(missing_entity_line(&name));
            }
        }

        if let Some(hit) = match_rules_normalized(&normalized) {
            return RouteOutcome::Purpose(PurposeResult::from_rule(&hit));
        }

        if let Some(route) = self.lexicon.route(input, &ctx) {
            return match &route {
                LexiconRoute::Clarify(need) => {
                    RouteOutcome::Clarify(clarify::prompt_for(need.clone(), &self.world))
                }
                LexiconRoute::TalkSocial {
                    action_hint,
                    targets,
                    topic_id,
                    ..
                } => {
                    let target_label = topic_id
                        .strip_prefix("actor.")
                        .unwrap_or(topic_id.as_str())
                        .to_string();
                    if clarify::needs_flavor_vs_social(input, &normalized) {
                        RouteOutcome::Clarify(clarify::prompt_for(
                            ClarifyNeed::FlavorVsSocial {
                                target_label,
                                action_hint: action_hint.clone(),
                                targets: targets.clone(),
                            },
                            &self.world,
                        ))
                    } else {
                        RouteOutcome::Purpose(route.to_purpose_result())
                    }
                }
                _ => RouteOutcome::Purpose(route.to_purpose_result()),
            };
        }

        let loc = self.world.location.display_name();
        let present = self
            .present_roster_lines()
            .into_iter()
            .collect::<Vec<_>>()
            .join("; ");
        let memory = self.memory_line();
        let purpose = if let Some(llm) = llm {
            purpose_classify(llm, input, loc, &present, &memory).await
        } else {
            purpose_heuristic(input)
        };

        if purpose.mode == TurnMode::Ambiguous && !clarify::purpose_is_dangerous(&purpose) {
            let lex_hits = self.lexicon.match_hits(&normalized).len();
            if clarify::is_unmatched_chatter(&normalized, lex_hits) {
                return RouteOutcome::FlavorTalk(vec![]);
            }
            let fallback = purpose_heuristic(input);
            if fallback.mode != TurnMode::Ambiguous {
                return RouteOutcome::Purpose(fallback);
            }
            if crate::rules::is_talk_routing_line(&normalized) {
                return RouteOutcome::Purpose(PurposeResult {
                    mode: TurnMode::Talk,
                    targets: crate::rules::extract_npc_targets(&normalized),
                    summary: Some("talk".into()),
                    action_hint: Some("talk".into()),
                    social_act: false,
                    confidence: Some(0.85),
                });
            }
            return RouteOutcome::Purpose(PurposeResult {
                mode: TurnMode::Act,
                targets: vec![],
                summary: Some("act".into()),
                action_hint: None,
                social_act: false,
                confidence: Some(0.5),
            });
        }
        if clarify::purpose_needs_clarify(&purpose) {
            return RouteOutcome::Clarify(clarify::prompt_for(
                ClarifyNeed::LowConfidence {
                    summary: purpose.summary.clone().unwrap_or_else(|| input.into()),
                    action_hint: purpose.action_hint.clone(),
                    targets: purpose.targets.clone(),
                },
                &self.world,
            ));
        }

        RouteOutcome::Purpose(purpose)
    }

    async fn dispatch_purpose(
        &mut self,
        input: &str,
        purpose: &PurposeResult,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        info!(
            %input,
            mode = purpose.mode.as_str(),
            social = purpose.social_act,
            hint = ?purpose.action_hint,
            confidence = ?purpose.confidence,
            "rpg purpose"
        );
        match purpose.mode {
            TurnMode::Query => self.handle_query(input, purpose, llm).await,
            TurnMode::Talk => {
                let events = self.handle_talk(input, purpose, llm).await;
                prepend_player_feed(self.player_display_name(), input, events)
            }
            TurnMode::Act => {
                let events = self.handle_act(input, purpose, llm).await;
                prepend_player_feed(self.player_display_name(), input, events)
            }
            TurnMode::Ambiguous => {
                let events = self.handle_flavor_talk(input, &[], llm).await;
                prepend_player_feed(self.player_display_name(), input, events)
            }
        }
    }

    async fn handle_flavor_talk(
        &mut self,
        input: &str,
        targets: &[String],
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        let purpose = PurposeResult {
            mode: TurnMode::Talk,
            targets: targets.to_vec(),
            summary: Some("flavor".into()),
            action_hint: Some("talk".into()),
            social_act: false,
            confidence: Some(1.0),
        };
        self.handle_talk(input, &purpose, llm).await
    }

    async fn resolve_clarify_answer(
        &mut self,
        answer: &str,
        pending: PendingClarify,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        let original = pending.original_line;
        let resolution = pending
            .prompt
            .parse_answer(answer)
            .unwrap_or(ClarifyResolution::YieldFlavor);

        match resolution {
            ClarifyResolution::KernelHint { word } => vec![GameEvent::Narration(format!(
                "Type `{word}` at the prompt for that command — not in character."
            ))],
            ClarifyResolution::TalkSocial {
                action_hint,
                targets,
            } => {
                let purpose = PurposeResult {
                    mode: TurnMode::Talk,
                    targets,
                    summary: Some("social".into()),
                    action_hint: Some(action_hint),
                    social_act: false,
                    confidence: Some(1.0),
                };
                self.dispatch_purpose(&original, &purpose, llm).await
            }
            ClarifyResolution::TalkFlavor { targets } => {
                self.handle_flavor_talk(&original, &targets, llm).await
            }
            ClarifyResolution::TopicChoice { targets } => {
                self.handle_flavor_talk(&original, &targets, llm).await
            }
            ClarifyResolution::YieldFlavor => {
                self.handle_flavor_talk(&original, &[], llm).await
            }
        }
    }

    async fn handle_query(
        &mut self,
        input: &str,
        purpose: &PurposeResult,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        let lower = crate::slang::normalize(input);
        if let Some(ans) = room_presence_answer(&lower, &self.world, &self.scene, &self.world_doc) {
            return vec![GameEvent::Narration(ans)];
        }
        if purpose.action_hint.as_deref() == Some("dict") {
            return vec![GameEvent::Narration(self.lexicon.dump_text())];
        }
        if purpose.action_hint.as_deref() == Some("inventory")
            || lower.contains("inventory")
            || lower.contains("do i have")
            || lower.contains("what do i have")
        {
            return self
                .apply_action(PlayerAction::Inventory, input, llm)
                .await;
        }
        if lower.contains("who") || lower.contains("where") {
            return vec![GameEvent::Narration(
                self.narrate_here(llm, Some(input)).await,
            )];
        }
        // Status / sheet read — store truth, no Director prose inventing stats.
        let p = &self.world.player;
        vec![GameEvent::Narration(format!(
            "You check yourself:\n{}\n{}",
            p.build.sheet_summary(),
            self.status_line()
        ))]
    }

    /// Static block narration — no Actor, Director, or Narrator organs.
    fn reject_yield(msg: impl Into<String>) -> Vec<GameEvent> {
        let msg = msg.into();
        let line = if msg.starts_with('❌') {
            msg
        } else {
            format!("❌ Blocked — {msg}")
        };
        vec![GameEvent::Narration(line)]
    }

    /// Talk miss — not a skill-check failure; the Actor never ran.
    fn talk_miss(msg: impl Into<String>) -> GameEvent {
        GameEvent::Narration(format!("💬 Talk — {}", msg.into()))
    }

    fn act_miss(msg: impl Into<String>) -> GameEvent {
        GameEvent::Narration(format!("🎬 Act — {}", msg.into()))
    }

    async fn handle_talk(
        &mut self,
        input: &str,
        purpose: &PurposeResult,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        let _ = purpose;
        let normalized = self.lexicon.normalize(input);
        if is_soft_search_line(&normalized) {
            if let Some(events) = self.discover_soft_beat(input, llm).await {
                return events;
            }
        }

        let mut events = Vec::new();

        let tag = ActionTag {
            action: "talk".into(),
            npc: purpose.targets.first().cloned(),
            location: None,
            confidence: Some(3),
            reason: Some("talk path".into()),
        };

        if !self.action_possible(&tag) {
            events.push(Self::talk_miss(
                "That isn't possible right now — not with what's in front of you.",
            ));
            return events;
        }

        self.scene.clear_focus_if_absent(&self.world);
        let request = self.talk_request_from_input(
            tag.npc.as_deref().or_else(|| purpose.targets.first().map(|s| s.as_str())),
            input,
        );
        if let Err(miss) = self.scene.resolve_talk_target(&self.world, request) {
            let msg = match miss {
                crate::scene::TalkMiss::Ambiguous => {
                    talk_unbound_line(&self.world, &self.scene)
                }
                other => other.message(),
            };
            events.push(Self::talk_miss(msg));
            return events;
        }

        events.extend(self.talk(tag.npc, input, llm).await);
        events
    }

    async fn handle_act(
        &mut self,
        input: &str,
        purpose: &PurposeResult,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        if self.line_should_use_talk(input, purpose) {
            return self.handle_talk(input, purpose, llm).await;
        }
        let plan = self.purpose_to_plan(input, purpose);
        let had_steps = !plan.steps.is_empty();
        let plan = self.sanitize_plan(plan, input);
        info!(
            steps = plan.steps.len(),
            summary = ?plan.summary,
            "rpg act plan"
        );

        if plan.steps.is_empty() && had_steps {
            return Self::reject_yield(
                "That isn't possible right now — not with what's in front of you.",
            );
        }

        if plan.steps.is_empty() {
            let normalized = self.lexicon.normalize(input);
            if is_soft_search_line(&normalized)
                || purpose.action_hint.as_deref() == Some("look")
                || normalized.contains("look for")
                || normalized.contains("search for")
            {
                if let Some(events) = self.discover_soft_beat(input, llm).await {
                    return events;
                }
                if purpose.action_hint.as_deref() == Some("look")
                    || normalized.contains("look for")
                    || normalized.contains("search for")
                    || is_soft_search_line(&normalized)
                {
                    return vec![GameEvent::Narration(format_narrator_line(&look_scene_line(
                        &self.world,
                        &self.scene,
                    )))];
                }
            }
            if let Some(name) =
                detect_missing_entity(&normalized, &self.world, &self.scene, &self.world_doc)
            {
                return vec![GameEvent::Narration(missing_entity_line(&name))];
            }
            return vec![Self::act_miss(act_unbound_line(&self.world))];
        }

        let mut events = Vec::new();
        for tag in &plan.steps {
            if self.ended.is_some() {
                break;
            }
            if !self.action_possible(tag) {
                info!(action = %tag.action, "legality reject");
                return Self::reject_yield(
                    "That isn't possible right now — not with what's in front of you.",
                );
            }
            let action = self.materialize_action(tag);
            if matches!(
                &action,
                PlayerAction::Unknown(id) if id != "pet" && id != "sit"
            ) {
                continue;
            }
            events.extend(self.apply_action(action, input, llm).await);
        }
        events
    }

    fn purpose_to_plan(&self, input: &str, purpose: &PurposeResult) -> ActionPlan {
        let lower = input.to_lowercase();
        // Compound tip → charm remains a thin rule expansion (not catalog LLM).
        if lower.contains("tip")
            && lower.contains("sally")
            && (lower.contains("charm") || lower.contains("flirt"))
        {
            return ActionPlan {
                steps: vec![
                    ActionTag {
                        action: "tip_sally".into(),
                        npc: Some("sally".into()),
                        location: None,
                        confidence: Some(3),
                        reason: Some("compound tip".into()),
                    },
                    ActionTag {
                        action: "charm_sally".into(),
                        npc: Some("sally".into()),
                        location: None,
                        confidence: Some(3),
                        reason: Some("compound charm".into()),
                    },
                ],
                summary: purpose.summary.clone(),
            };
        }
        if let Some(hint) = purpose.action_hint.as_deref() {
            let mut tag = ActionTag {
                action: hint.to_string(),
                npc: purpose.targets.first().cloned(),
                location: if hint == "go" {
                    Some("hall".into())
                } else {
                    None
                },
                confidence: Some(3),
                reason: Some("purpose/rules".into()),
            };
            self.clamp_tag_npc(&mut tag);
            return ActionPlan {
                steps: vec![tag],
                summary: purpose.summary.clone(),
            };
        }
        // Missed hint: thin heuristic fallback (not catalog LLM authority).
        heuristic_interpret_plan(input, self.world.location.display_name())
    }

    /// Drop impossible / junk steps (e.g. impress+pay after a door intimidation).
    fn sanitize_plan(&self, plan: ActionPlan, player_text: &str) -> ActionPlan {
        let lower = player_text.to_lowercase();
        let intimidate = lower.contains("intimidate")
            || lower.contains("creep out")
            || lower.contains("threaten")
            || lower.contains("step aside")
            || lower.contains("move aside")
            || lower.contains("unless you move")
            || lower.contains("stabbing")
            || lower.contains("scram")
            || lower.contains("loom")
            || lower.contains("your blood")
            || lower.contains("paint the floor")
            || lower.contains("pint size")
            || (lower.contains("draw") && lower.contains("knife") && !lower.contains("attack"));
        let pay_intent = (has_pay_intent(&lower))
            && !lower.contains("not pay")
            && !lower.contains("won't pay")
            && !lower.contains("wont pay")
            && !lower.contains("don't pay");

        let mut steps: Vec<ActionTag> = Vec::new();
        for mut tag in plan.steps {
            if intimidate
                && self.world.location == Location::Outside
                && matches!(
                    tag.action.as_str(),
                    "fight" | "impress_dremis" | "pay_scam" | "give_password" | "persuade_groll" | "look"
                )
            {
                tag = ActionTag {
                    action: "challenge_dewr".into(),
                    npc: Some("dewr".into()),
                    location: None,
                    confidence: tag.confidence,
                    reason: Some("sanitized: intimidate→challenge".into()),
                };
            }
            self.clamp_tag_npc(&mut tag);
            // Never pay unless the player clearly intends to pay.
            if tag.action == "pay_scam" && !pay_intent {
                info!("dropped pay_scam without pay intent");
                continue;
            }
            // leave/ambush only when the player clearly abandons the scene.
            if tag.action == "leave" && !wants_leave_command(&lower) {
                info!("dropped leave without clear leave intent");
                // Asking why someone won't let you in → try the door, not ambush.
                if lower.contains("let") && (lower.contains("in") || lower.contains("pass")) {
                    tag = ActionTag {
                        action: "go".into(),
                        npc: None,
                        location: Some("hall".into()),
                        confidence: tag.confidence,
                        reason: Some("sanitized: let-me-in→go".into()),
                    };
                } else {
                    continue;
                }
            }
            if !self.action_possible(&tag) {
                info!(action = %tag.action, "dropped impossible plan step");
                continue;
            }
            // Collapse junk multi-step intimidation plans into one check.
            if intimidate
                && tag.action == "challenge_dewr"
                && steps.iter().any(|s| s.action == "challenge_dewr")
            {
                continue;
            }
            steps.push(tag);
            if steps.len() >= 2 {
                break;
            }
        }
        if steps.is_empty() {
            let fallback = heuristic_interpret_plan(player_text, self.world.location.display_name());
            for tag in fallback.steps {
                if tag.action == "pay_scam" && !pay_intent {
                    continue;
                }
                if self.action_possible(&tag) {
                    steps.push(tag);
                    break;
                }
            }
        }
        ActionPlan {
            steps,
            summary: plan.summary,
        }
    }

    fn action_possible(&self, tag: &ActionTag) -> bool {
        let loc = self.world.location;
        match tag.action.as_str() {
            "look" | "inventory" | "help" | "leave" | "talk" | "pet" | "sit" => true,
            "pay_scam" => {
                !self.world.player.dewr_scam_cleared
                    && self.world.dewr.alive
                    && matches!(loc, Location::Outside | Location::MainHall)
            }
            "call_out_scam" | "challenge_dewr" => {
                self.world.dewr.alive
                    && !self.world.player.dewr_scam_cleared
                    && matches!(loc, Location::Outside | Location::MainHall)
            }
            "fight" => {
                let npc = tag.npc.as_deref().unwrap_or("");
                if npc == "dewr" || npc == "goblin" || (npc.is_empty() && loc == Location::Outside)
                {
                    self.world.dewr.alive
                        && matches!(loc, Location::Outside | Location::MainHall)
                } else if npc == "groll" {
                    self.world.groll.alive && loc == Location::MainHall
                } else if npc == "dremis" {
                    self.world.dremis.alive
                        && loc == Location::StorageRoom
                        && self.world.player.aura_cleared
                } else {
                    loc == Location::MainHall
                }
            }
            "go" => true,
            "buy_food" => matches!(loc, Location::Kitchen | Location::MainHall),
            "sleep" => loc == Location::Upstairs,
            "tip_sally" | "charm_sally" => loc == Location::MainHall && self.world.sally.alive,
            "persuade_groll" => loc == Location::MainHall && self.world.groll.alive,
            "accept_sword" => {
                loc == Location::MainHall
                    && self.world.sword_offer_available
                    && !self.world.sword_offer_taken
            }
            "give_password" => {
                loc == Location::StorageRoom && self.world.player.has_password
            }
            "impress_dremis" => {
                loc == Location::StorageRoom
                    && self.world.player.aura_cleared
                    && self.world.dremis.alive
            }
            _ => false,
        }
    }

    /// Keep LLM npc tags tied to who is actually in the room (quest or ambient).
    fn clamp_tag_npc(&self, tag: &mut ActionTag) {
        let Some(raw) = tag.npc.as_deref() else {
            return;
        };
        let key = raw.trim().to_lowercase();
        if self.scene.find_ambient(self.world.location, &key).is_some() {
            // Ambient: keep for talk only.
            if tag.action != "talk" {
                info!(npc = %raw, "dropped ambient npc from non-talk action");
                tag.npc = None;
            } else {
                tag.npc = Some(key);
            }
            return;
        }
        let Some(id) = NpcId::parse(&key) else {
            // Unknown name — keep on talk so resolve can miss cleanly; drop otherwise.
            if tag.action != "talk" {
                tag.npc = None;
            }
            return;
        };
        let id = SceneCoordinator::canonicalize_npc(&self.world, id);
        let visible = SceneCoordinator::visible_npcs(&self.world);
        if visible.contains(&id) {
            tag.npc = Some(id.display_name().to_lowercase());
            return;
        }
        if matches!(id, NpcId::Patron | NpcId::Goblin) {
            tag.npc = None;
            return;
        }
        info!(npc = %raw, "clamped absent npc from plan tag");
        tag.npc = None;
    }

    fn materialize_action(&self, tag: &ActionTag) -> PlayerAction {
        let mut action = PlayerAction::from_action_tag(tag);
        if let PlayerAction::Fight(npc) = &action {
            let resolved = match npc {
                Some(id) => Some(SceneCoordinator::canonicalize_npc(&self.world, *id)),
                None if self.world.location == Location::Outside && self.world.dewr.alive => {
                    Some(NpcId::Dewr)
                }
                None => None,
            };
            if let Some(id) = resolved {
                action = PlayerAction::Fight(Some(id));
            }
        }
        if let PlayerAction::Talk(Some(name)) = &action {
            if let Some(id) = NpcId::parse(name) {
                let id = SceneCoordinator::canonicalize_npc(&self.world, id);
                action = PlayerAction::Talk(Some(id.display_name().to_lowercase()));
            }
        }
        action
    }

    async fn apply_action(
        &mut self,
        action: PlayerAction,
        input: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        match action {
            PlayerAction::Help => vec![GameEvent::Narration(Self::help_text().into())],
            PlayerAction::Look => {
                let normalized = self.lexicon.normalize(input);
                let searching = normalized.contains("look for")
                    || normalized.contains("search for")
                    || is_soft_search_line(&normalized);
                if searching {
                    if let Some(events) = self.discover_soft_beat(input, llm).await {
                        return events;
                    }
                    if let Some(name) = detect_missing_entity(
                        &normalized,
                        &self.world,
                        &self.scene,
                        &self.world_doc,
                    ) {
                        return vec![GameEvent::Narration(missing_entity_line(&name))];
                    }
                    return vec![GameEvent::Narration(format_narrator_line(&look_scene_line(
                        &self.world,
                        &self.scene,
                    )))];
                }
                survey_location(&mut self.world);
                vec![GameEvent::Narration(
                    self.narrate_here(llm, Some(input)).await,
                )]
            }
            PlayerAction::Inventory => {
                self.say(
                        llm,
                        "Inventory glance — list only what the player carries right now. No quest spoilers.",
                        vec![
                            format!("hp={}/{}", self.world.player.hp, self.world.player.max_hp),
                            format!("gold={}", self.world.player.gold),
                            format!("weapon={}", self.world.player.weapon.display_name()),
                            format!("has_amulet={}", self.world.player.has_amulet),
                        ],
                        Some(input),
                        self.inventory_text(),
                    )
                    .await
            }
            PlayerAction::Go(loc) => self.go_to(loc, input, llm).await,
            PlayerAction::Talk(npc) => self.talk(npc, input, llm).await,
            PlayerAction::BuyFood => self.buy_food(input, llm).await,
            PlayerAction::Sleep => self.sleep(input, llm).await,
            PlayerAction::TipSally => self.tip_sally(input, llm).await,
            PlayerAction::CharmSally => {
                self.contested(CheckKind::CharmSally, input, llm, false, None, None)
                    .await
            }
            PlayerAction::PersuadeGroll => {
                self.contested(
                    CheckKind::PersuadeGrollPassword,
                    input,
                    llm,
                    false,
                    None,
                    None,
                )
                .await
            }
            PlayerAction::ChallengeDewr => {
                self.contested(CheckKind::ChallengeDewr, input, llm, false, None, None)
                    .await
            }
            PlayerAction::ImpressDremis => {
                self.contested(CheckKind::ImpressDremis, input, llm, false, None, None)
                    .await
            }
            PlayerAction::GivePassword => self.give_password(input, llm).await,
            PlayerAction::Fight(npc) => self.fight(npc, input, llm).await,
            PlayerAction::AcceptSwordTrade => self.accept_sword_trade(input, llm).await,
            PlayerAction::PayScam => self.pay_scam(input, llm).await,
            PlayerAction::CallOutScam => {
                self.contested(CheckKind::SeeThroughScam, input, llm, false, None, None)
                    .await
            }
            PlayerAction::Leave => self.leave(input, llm).await,
            PlayerAction::Unknown(ref id) if id == "sit" => {
                self.say(
                    llm,
                    "Player sits at the tavern — one short beat, no new plot.",
                    vec!["sat_down=true".into()],
                    Some(input),
                    "You take a seat on a stool near the hearth.".into(),
                )
                .await
            }
            PlayerAction::Unknown(ref id) if id == "pet" => {
                let normalized = self.lexicon.normalize(input);
                if let Some(events) = self.interact_soft_beat(input, llm).await {
                    return events;
                }
                if let Some(name) =
                    detect_missing_entity(&normalized, &self.world, &self.scene, &self.world_doc)
                {
                    vec![GameEvent::Narration(missing_entity_line(&name))]
                } else {
                    vec![Self::act_miss(act_unbound_line(&self.world))]
                }
            }
            PlayerAction::Unknown(_) => Vec::new(),
        }
    }

    fn inventory_text(&self) -> String {
        self.status_line()
    }

    pub const SCAM_FEE: i32 = 10;

    async fn go_to(
        &mut self,
        loc: Location,
        player_line: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        if loc == Location::MainHall
            && self.world.location == Location::Outside
            && !self.world.player.dewr_scam_cleared
            && self.world.dewr.alive
        {
            if wants_sneak_past(&player_line.to_lowercase()) {
                return self.sneak_past_dewr(player_line, llm).await;
            }
            self.world.player.met_dewr = true;
            self.world.first_talk_pending = false;
            return self.say(
                    llm,
                    "Blocked doorway — a goblin refuses entry and demands coin. He does not move aside.",
                    vec![
                        "blocked=true".into(),
                        "dewr_blocks_door=true".into(),
                        "demands_coin=true".into(),
                    ],
                    Some(player_line),
                    "The goblin blocks the doorway, palm out for coin, smirk sharp as his knife."
                        .into(),
                )
                .await;
        }

        if matches!(
            loc,
            Location::Kitchen | Location::Upstairs | Location::StorageRoom
        ) && self.world.location == Location::Outside
        {
            return self.say(
                    llm,
                    "Blocked — still outside; must deal with the doorway first.",
                    vec!["blocked=true".into(), "still_outside=true".into()],
                    Some(player_line),
                    "You're still outside. Deal with the doorway first.".into(),
                )
                .await;
        }

        if loc == Location::StorageRoom && !self.world.player.has_password {
            return self.say(
                    llm,
                    "A wall seam does not yield. Something is missing — do not name what.",
                    vec!["blocked=true".into(), "wall_does_not_yield=true".into()],
                    Some(player_line),
                    "You press along the wall. Nothing yields — not yet.".into(),
                )
                .await;
        }
        if loc == Location::StorageRoom && self.world.player.has_password {
            self.world.location = Location::StorageRoom;
            self.ensure_ambient_cast(llm).await;
            return Vec::new();
        }

        self.world.location = loc;
        self.ensure_ambient_cast(llm).await;
        Vec::new()
    }

    async fn sneak_past_dewr(
        &mut self,
        player_line: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        let kind = CheckKind::SneakPastDewr;
        let dc = dc_for(kind, self.world.player.sally_tip_discount);
        let result = resolve_d20_check(kind, &self.world.player.build, dc, &mut self.rng);
        metrics::inc_rpg_check(result.success);
        info!(
            check = kind.label(),
            skill = result.skill.as_str(),
            roll = result.roll,
            total = result.total,
            dc = result.dc,
            success = result.success,
            "sneak past dewr"
        );

        let report = format_check_report(&result);
        let focus = Self::focus_for_check(kind);
        self.scene.note_talk(&focus);

        if result.success {
            self.world.location = Location::MainHall;
            self.world.first_talk_pending = false;
            self.ensure_ambient_cast(llm).await;
            let events = vec![GameEvent::Narration(report)];
            return events;
        }

        self.world.player.met_dewr = true;
        self.world.first_talk_pending = false;
        let npc_fallback = "*cackles and grabs your shoulder* \"Nice try, sweetheart — coin or walk.\""
            .to_string();
        let locked = vec![
            format!("check={}", kind.label()),
            format!("success=false"),
            format!("roll={}", result.roll),
            format!("total={}", result.total),
            "dewr_spotted=true".into(),
            "dewr_still_blocking=true".into(),
        ];
        let mut events = vec![GameEvent::Narration(report)];
        events.push(
            self.npc_reply(
                &focus,
                player_line,
                locked,
                Some(false),
                &npc_fallback,
                llm,
            )
            .await,
        );
        events
    }

    async fn pay_scam(
        &mut self,
        player_line: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        if self.world.player.dewr_scam_cleared {
            return self.say(
                    llm,
                    "No toll left to pay.",
                    vec!["blocked=true".into(), "toll_already_cleared=true".into()],
                    Some(player_line),
                    "The doorway isn't a toll booth anymore.".into(),
                )
                .await;
        }
        if !self.world.dewr.alive {
            return self.say(
                    llm,
                    "No collector left to take coin.",
                    vec!["blocked=true".into(), "collector_gone=true".into()],
                    Some(player_line),
                    "There's no one left to take your coin.".into(),
                )
                .await;
        }
        if self.world.location != Location::Outside && self.world.location != Location::MainHall {
            return self.say(
                    llm,
                    "No toll collector in this place.",
                    vec!["blocked=true".into()],
                    Some(player_line),
                    "There's no toll collector here.".into(),
                )
                .await;
        }
        if self.world.player.gold < Self::SCAM_FEE {
            return self.say(
                    llm,
                    "Player digs for coin and comes up short.",
                    vec!["blocked=true".into(), "not_enough_gold=true".into()],
                    Some(player_line),
                    "You dig for coin and come up short.".into(),
                )
                .await;
        }
        self.world.player.gold -= Self::SCAM_FEE;
        self.world.player.dewr_scam_cleared = true;
        self.world.player.met_dewr = true;
        self.world.first_talk_pending = false;
        self.world.location = Location::MainHall;
        metrics::inc_rpg_scam_paid();
        metrics::inc_rpg_scam_cleared();
        info!(fee = Self::SCAM_FEE, gold = self.world.player.gold, "dewr scam paid");
        self.ensure_ambient_cast(llm).await;
        self.say(
                llm,
                &format!(
                    "Player pays the doorway fee. Goblin pockets coin with a mocking bow, then waves them through. \
*stage direction* only — no spoken dialogue."
                ),
                vec![
                    format!("gold_spent={}", Self::SCAM_FEE),
                    format!("gold_remaining={}", self.world.player.gold),
                    "entered_main_hall=true".into(),
                    "goblin_mocks_player=true".into(),
                    "travel=true".into(),
                ],
                Some(player_line),
                format!(
                    "*You hand over {} gold.* The goblin pockets it fast and waves you through with a mocking bow.",
                    Self::SCAM_FEE
                ),
            )
            .await
    }

    async fn clear_scam_and_enter(
        &mut self,
        _player_line: Option<&str>,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        let entering = self.world.location == Location::Outside;
        self.apply_doorway_clear();
        if !entering {
            return Vec::new();
        }
        self.world.location = Location::MainHall;
        self.ensure_ambient_cast(llm).await;
        Vec::new()
    }

    fn apply_doorway_clear(&mut self) {
        self.world.player.dewr_scam_cleared = true;
        self.world.player.dewr_backed_down = true;
        self.world.player.met_dewr = true;
        self.world.first_talk_pending = false;
        metrics::inc_rpg_scam_cleared();
        info!("dewr scam cleared without paying");
    }

    async fn talk(
        &mut self,
        npc: Option<String>,
        player_line: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        self.scene.clear_focus_if_absent(&self.world);
        let request = self.talk_request_from_input(npc.as_deref(), player_line);
        let focus = match self.scene.resolve_talk_target(&self.world, request) {
            Ok(f) => f,
            Err(miss) => {
                return Self::reject_yield(miss.message());
            }
        };

        // Ambient cast only after legality passes (scene prep, not Actor).
        self.ensure_ambient_cast(llm).await;

        // Hall ambient: claiming a seat isn't a conversation.
        let lower_line = player_line.to_lowercase();
        if self.world.location == Location::MainHall
            && npc.is_none()
            && (lower_line.contains("seat")
                || lower_line.contains("sit")
                || lower_line.contains("table")
                || lower_line.contains("claim"))
            && !lower_line.contains("sally")
            && !lower_line.contains("groll")
            && !lower_line.contains("dewr")
            && !lower_line.contains("goblin")
            && !lower_line.contains("carter")
            && !lower_line.contains("piper")
        {
            return self.say(
                    llm,
                    "Player finds an open seat in the smoky hall and settles in. Atmosphere only. Mention who is nearby by look.",
                    vec![
                        "claimed_seat=true".into(),
                        "location=main hall".into(),
                    ],
                    Some(player_line),
                    "You claim an open seat near the hearth. Smoke, chatter, and the clink of mugs settle around you."
                        .into(),
                )
                .await;
        }

        self.scene.note_talk(&focus);

        match self.secrets.bind_talk(player_line, &focus, &self.world) {
            Err(BindBlock::Locked(msg)) => {
                return vec![Self::talk_miss(msg)];
            }
            Ok(TalkBind::Tagged(secret_id)) => {
                let Some(secret) = self.secrets.secret(&secret_id) else {
                    return vec![Self::talk_miss("That doesn't land here.")];
                };
                let Some(kind) = SecretsCatalog::check_to_kind(&secret.check) else {
                    return vec![Self::talk_miss("That doesn't land here.")];
                };
                let dc = self
                    .secrets
                    .resolve_dc(&secret_id, &self.world, &focus)
                    .unwrap_or(secret.base_dc);
                return self
                    .contested(kind, player_line, llm, true, Some(dc), Some(secret_id))
                    .await;
            }
            Ok(TalkBind::OffBook) => {
                return self.generic_social(player_line, &focus, llm).await;
            }
            Ok(TalkBind::Banter) => {}
        }

        let (display_name, voice, desc_fact) = self.focus_voice(&focus);
        let facts = self.focus_facts(&focus, &display_name, &desc_fact);

        let parts = split_player_line(player_line);
        let offline_fallback = match &focus {
            TalkFocus::Quest(id) => default_npc_line(*id, &self.world),
            TalkFocus::Ambient(_) => "*looks you over* \"Evenin'. Don't mind me.\"".into(),
        };
        let mut line = if let Some(llm) = llm {
            generate_npc_line(
                llm,
                &DialogueRequest {
                    npc_name: display_name.clone(),
                    card_summary: voice,
                    location: self.world.location.display_name().into(),
                    player_line: player_line.into(),
                    allowed_facts: facts,
                    memory: self.memory_line_for_actor(&display_name),
                    player_action: parts.action.clone(),
                    player_speech: parts.speech.clone(),
                    locked_outcome: vec![],
                    check_success: None,
                },
            )
            .await
            .line
        } else {
            offline_fallback.clone()
        };
        line = finalize_npc_line_with_prior(
            &line,
            &display_name,
            player_line,
            &offline_fallback,
            &[self.memory_line_for_actor(&display_name).as_str()],
        );

        let staged = parse_staged_line(&line);
        let mut events = vec![GameEvent::Narration(format_npc_display(
            &display_name,
            &staged,
        ))];

        // Occasional goblin pick-fight flavor in the hall (only if Dewr is present).
        if self.world.location == Location::MainHall
            && !matches!(focus, TalkFocus::Quest(NpcId::Dewr))
            && SceneCoordinator::visible_npcs(&self.world).contains(&NpcId::Dewr)
            && self.rng.gen_bool(0.15)
        {
            events.extend(self.say(
                    llm,
                    "Flavor beat — a bored goblin jostles the player, spoiling for a scrap. Do not start combat. Use *stage direction*.",
                    vec![
                        "goblin_jostles=true".into(),
                        "combat_not_started=true".into(),
                    ],
                    Some(player_line),
                    "*A bored goblin jostles you* — looking for a fight.".into(),
                )
                .await,
            );
        }

        events
    }

    fn focus_voice(&mut self, focus: &TalkFocus) -> (String, String, String) {
        match focus {
            TalkFocus::Quest(id) => {
                if *id == NpcId::Dewr {
                    self.world.first_talk_pending = false;
                    self.world.player.met_dewr = true;
                }
                let card = card_for(&self.cards, *id);
                let voice = card
                    .map(|c| c.voice_tag())
                    .unwrap_or_else(|| id.display_name().to_string());
                let desc = card
                    .map(|c| c.description_tag())
                    .unwrap_or_else(|| id.display_name().into());
                (id.display_name().to_string(), voice, desc)
            }
            TalkFocus::Ambient(aid) => {
                let card = self.scene.ambient_card(aid);
                let name = card
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| aid.clone());
                let voice = card
                    .map(|c| c.voice_tag())
                    .unwrap_or_else(|| name.clone());
                let desc = card
                    .map(|c| c.description_tag())
                    .unwrap_or_else(|| name.clone());
                (name, voice, desc)
            }
        }
    }

    fn focus_facts(&self, focus: &TalkFocus, display_name: &str, desc_fact: &str) -> Vec<String> {
        let mut facts = self.look_facts();
        facts.truncate(6);
        facts.push(format!("speaking_to={display_name}"));
        facts.push(format!("looks_like={desc_fact}"));
        if let TalkFocus::Quest(id) = focus {
            if self.world.player.has_password && matches!(id, NpcId::Dremis | NpcId::Groll) {
                facts.push("player_knows_a_door_word=true".into());
            }
            if self.world.player.aura_cleared && *id == NpcId::Dremis {
                facts.push("aura_cleared=true".into());
            }
        }
        facts
    }

    async fn npc_reply(
        &mut self,
        focus: &TalkFocus,
        player_line: &str,
        locked_outcome: Vec<String>,
        check_success: Option<bool>,
        fallback: &str,
        llm: Option<&dyn LlmClient>,
    ) -> GameEvent {
        let (display_name, voice, desc_fact) = self.focus_voice(focus);
        let facts = self.focus_facts(focus, &display_name, &desc_fact);
        let parts = split_player_line(player_line);
        let line = if let Some(llm) = llm {
            generate_npc_line(
                llm,
                &DialogueRequest {
                    npc_name: display_name.clone(),
                    card_summary: voice,
                    location: self.world.location.display_name().into(),
                    player_line: player_line.into(),
                    allowed_facts: facts,
                    memory: self.memory_line_for_actor(&display_name),
                    player_action: parts.action,
                    player_speech: parts.speech,
                    locked_outcome,
                    check_success,
                },
            )
            .await
            .line
        } else {
            fallback.to_string()
        };
        let line = finalize_npc_line_with_prior(
            &line,
            &display_name,
            player_line,
            fallback,
            &[self.memory_line_for_actor(&display_name).as_str()],
        );
        let staged = parse_staged_line(&line);
        GameEvent::Narration(format_npc_display(&display_name, &staged))
    }

    fn check_miss(from_talk: bool, msg: impl Into<String>) -> Vec<GameEvent> {
        if from_talk {
            vec![Self::talk_miss(msg)]
        } else {
            Self::reject_yield(msg)
        }
    }

    fn focus_for_check(kind: CheckKind) -> TalkFocus {
        match kind {
            CheckKind::PersuadeGrollPassword => TalkFocus::Quest(NpcId::Groll),
            CheckKind::CharmSally => TalkFocus::Quest(NpcId::Sally),
            CheckKind::ImpressDremis => TalkFocus::Quest(NpcId::Dremis),
            CheckKind::ChallengeDewr | CheckKind::SeeThroughScam | CheckKind::SneakPastDewr => {
                TalkFocus::Quest(NpcId::Dewr)
            }
        }
    }

    async fn generic_social(
        &mut self,
        player_line: &str,
        focus: &TalkFocus,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        let g = self.secrets.generic();
        let dc = self.secrets.resolve_generic_dc(&self.world, focus);
        let skill = match g.skill.as_str() {
            "intimidation" => Skill::Intimidation,
            "insight" => Skill::Insight,
            _ => Skill::Persuasion,
        };
        let build = &self.world.player.build;
        let ability_mod = build.abilities.modifier(skill.ability());
        let proficient = build.is_proficient(skill);
        let prof = if proficient {
            build.proficiency_bonus
        } else {
            0
        };
        let modifier = ability_mod + prof;
        let roll = self.rng.gen_range(1..=20);
        let total = roll + modifier;
        let success = total >= dc;
        metrics::inc_rpg_check(success);

        let outcome = pick_generic_outcome(success, focus, roll);
        apply_generic_outcome(&mut self.world, focus, outcome);

        let report = format!(
            "🎲 {} — {} ({}) d20 {} + {} ({}) = {} vs DC {}\n→ {} {}",
            g.label,
            skill.label(),
            skill.ability().label(),
            roll,
            modifier,
            if proficient {
                format!("+prof {prof}")
            } else {
                "untrained".into()
            },
            total,
            dc,
            if success { "✅" } else { "❌" },
            if success { "Success" } else { "Failure" },
        );

        let fallback = outcome.npc_fallback(success);
        let locked = vec![
            format!("generic_move={}", outcome.label()),
            format!("success={success}"),
        ];

        self.scene.note_talk(focus);
        let mut events = vec![GameEvent::Narration(report)];
        events.push(
            self.npc_reply(focus, player_line, locked, Some(success), &fallback, llm)
                .await,
        );
        events
    }

    async fn contested(
        &mut self,
        kind: CheckKind,
        player_text: &str,
        llm: Option<&dyn LlmClient>,
        from_talk: bool,
        dc_override: Option<i32>,
        secret_id: Option<String>,
    ) -> Vec<GameEvent> {
        if kind == CheckKind::SneakPastDewr {
            return self.sneak_past_dewr(player_text, llm).await;
        }

        let gate = match kind {
            CheckKind::CharmSally if self.world.location != Location::MainHall => Some((
                "Social check blocked — she's not here to charm.",
                "She's not here to charm.",
            )),
            CheckKind::PersuadeGrollPassword if self.world.location != Location::MainHall => Some((
                "Social check blocked — the cloaked patron isn't here.",
                "The cloaked patron isn't here.",
            )),
            CheckKind::ChallengeDewr
                if !matches!(
                    self.world.location,
                    Location::Outside | Location::MainHall
                ) =>
            {
                Some((
                    "Social check blocked — no goblin here to stare down.",
                    "There's no goblin here to stare down.",
                ))
            }
            CheckKind::SeeThroughScam
                if self.world.player.dewr_scam_cleared
                    || !self.world.dewr.alive
                    || !matches!(
                        self.world.location,
                        Location::Outside | Location::MainHall
                    ) =>
            {
                Some((
                    "Social check blocked — no toll game left to call out.",
                    "There's no toll game left to call out.",
                ))
            }
            CheckKind::ImpressDremis if self.world.location != Location::StorageRoom => Some((
                "Social check blocked — no one here to impress that way.",
                "There's no one here to impress that way.",
            )),
            CheckKind::ImpressDremis if !self.world.player.aura_cleared => Some((
                "Social check blocked — crushing dread locks the tongue.",
                "The dread locks your tongue. You can barely stand.",
            )),
            _ => None,
        };
        if let Some((_, fallback)) = gate {
            return Self::check_miss(from_talk, fallback);
        }

        if kind == CheckKind::PersuadeGrollPassword && self.world.player.has_password {
            return Self::check_miss(from_talk, "He already gave you what you needed.");
        }
        if kind == CheckKind::CharmSally && self.world.player.sally_charmed {
            return Self::check_miss(
                from_talk,
                "She already favors you — she laughs and squeezes your arm.",
            );
        }
        if kind == CheckKind::ChallengeDewr && self.world.player.dewr_backed_down {
            return Self::check_miss(
                from_talk,
                "He already backed down. He glares from a safer distance.",
            );
        }
        if kind == CheckKind::ImpressDremis && self.world.player.has_amulet {
            return Self::check_miss(from_talk, "You already carry what you came for.");
        }

        let dc = dc_override.unwrap_or_else(|| dc_for(kind, self.world.player.sally_tip_discount));
        let build = &self.world.player.build;

        let result = resolve_d20_check(kind, build, dc, &mut self.rng);
        if !result.success {
            if let Some(ref sid) = secret_id {
                note_authored_check_fail(&mut self.world, sid);
            }
        }
        metrics::inc_rpg_check(result.success);
        info!(
            check = kind.label(),
            skill = result.skill.as_str(),
            ability = result.ability.as_str(),
            modifier = result.modifier,
            proficient = result.proficient,
            dc = result.dc,
            roll = result.roll,
            total = result.total,
            success = result.success,
            "rpg check resolved"
        );

        let (npc_fallback, locked_extra): (String, Vec<String>) = if result.success {
            apply_check_success(&mut self.world, kind);
            match kind {
                CheckKind::PersuadeGrollPassword => (
                    "*cloak shifts* \"…Fine. Remember this — *grunts a word that settles in your bones*.\"".into(),
                    vec!["gained_door_word=true".into()],
                ),
                CheckKind::CharmSally => (
                    "*blushes, tipsy on your charm* \"You're trouble — I like it.\"".into(),
                    vec!["sally_charmed=true".into()],
                ),
                CheckKind::ChallengeDewr => (
                    "*ears flatten* \"Fine. Scram — before I change my mind.\"".into(),
                    vec!["dewr_backed_down=true".into()],
                ),
                CheckKind::SeeThroughScam => (
                    "*ears flatten* \"Fine. Scram.\"".into(),
                    vec!["scam_called_out=true".into(), "dewr_backed_down=true".into()],
                ),
                CheckKind::ImpressDremis => (
                    "*laughs once, dark and approving* \"You passed.\" *presses a warm amulet into your hand*".into(),
                    vec!["gained_amulet=true".into()],
                ),
                CheckKind::SneakPastDewr => unreachable!("sneak uses sneak_past_dewr"),
            }
        } else {
            match kind {
                CheckKind::ChallengeDewr => (
                    "*cackles and shoves you* \"Cute.\"".into(),
                    vec!["dewr_still_blocking=true".into()],
                ),
                CheckKind::SeeThroughScam => (
                    "*laughs in your face* \"Nice try. Coin, or get lost.\"".into(),
                    vec!["scam_still_stands=true".into()],
                ),
                CheckKind::ImpressDremis => (
                    "*smoke thickens* \"You waste my time.\"".into(),
                    vec!["offended=true".into()],
                ),
                CheckKind::CharmSally => {
                    self.world.player.sally_tip_discount = false;
                    (
                        "*polite smile* \"Sweet talk won't pay the rent, hon.\"".into(),
                        vec!["charm_failed=true".into()],
                    )
                }
                CheckKind::PersuadeGrollPassword => (
                    "*cloak does not stir* \"Move along.\"".into(),
                    vec!["refused=true".into()],
                ),
                CheckKind::SneakPastDewr => unreachable!("sneak uses sneak_past_dewr"),
            }
        };

        let mut locked = vec![
            format!("check={}", kind.label()),
            format!("success={}", result.success),
            format!("skill={}", result.skill.as_str()),
            format!("ability={}", result.ability.as_str()),
            format!("ability_mod={}", result.ability_mod),
            format!("proficient={}", result.proficient),
            format!("modifier={}", result.modifier),
            format!("dc={}", result.dc),
            format!("roll={}", result.roll),
            format!("total={}", result.total),
        ];
        locked.extend(locked_extra);

        let report = format_check_report(&result);
        let focus = Self::focus_for_check(kind);
        self.scene.note_talk(&focus);

        let mut events = vec![GameEvent::Narration(report)];
        events.push(
            self.npc_reply(
                &focus,
                player_text,
                locked,
                Some(result.success),
                &npc_fallback,
                llm,
            )
            .await,
        );

        if result.success
            && matches!(kind, CheckKind::ChallengeDewr | CheckKind::SeeThroughScam)
        {
            let should_clear_doorway = !self.world.player.dewr_scam_cleared;
            if should_clear_doorway {
                self.apply_doorway_clear();
                if self.world.location == Location::Outside {
                    self.world.location = Location::MainHall;
                    self.ensure_ambient_cast(llm).await;
                }
            }
        }

        events
    }

    async fn give_password(
        &mut self,
        player_line: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        if self.world.location != Location::StorageRoom {
            return self.say(
                    llm,
                    "Blocked — no one here needs that word.",
                    vec!["blocked=true".into()],
                    Some(player_line),
                    "There's no one here who needs that word.".into(),
                )
                .await;
        }
        if !self.world.player.has_password {
            return self.say(
                    llm,
                    "Blocked — player lacks the right word.",
                    vec!["blocked=true".into(), "missing_word=true".into()],
                    Some(player_line),
                    "You don't have the right word — not yet.".into(),
                )
                .await;
        }
        if self.world.player.aura_cleared && self.world.player.has_amulet {
            return self.say(
                    llm,
                    "Already acknowledged.",
                    vec!["already_done=true".into()],
                    Some(player_line),
                    "You've already been acknowledged.".into(),
                )
                .await;
        }

        self.world.player.aura_cleared = true;
        let mut events = self.say(
                llm,
                "Player speaks the word. Crushing dread shreds apart like wet smoke. Movement returns.",
                vec!["aura_cleared=true".into(), "spoke_the_word=true".into()],
                Some(player_line),
                "You speak the word. The crushing dread shreds apart like wet smoke. You can move again."
                    .into(),
            )
            .await;

        if !self.world.player.has_amulet {
            self.world.player.has_amulet = true;
            events.extend(self.say(
                    llm,
                    "The figure studies the player, then offers a warm amulet. The dread lifts.",
                    vec!["gained_amulet=true".into()],
                    Some(player_line),
                    "The figure studies you, then offers a warm amulet. You may leave when ready."
                        .into(),
                )
                .await,
            );
        }
        events
    }

    async fn buy_food(
        &mut self,
        player_line: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        if self.world.location != Location::Kitchen && self.world.location != Location::MainHall {
            return self.say(
                    llm,
                    "Blocked — food is not sold here.",
                    vec!["blocked=true".into()],
                    Some(player_line),
                    "Food is served in the kitchen or main hall.".into(),
                )
                .await;
        }
        if self.world.player.gold < 5 {
            return self.say(
                    llm,
                    "Blocked — not enough gold for a meal.",
                    vec!["blocked=true".into(), "not_enough_gold=true".into()],
                    Some(player_line),
                    "Not enough gold (need 5).".into(),
                )
                .await;
        }
        self.world.player.gold -= 5;
        self.world.player.hp = (self.world.player.hp + 1).min(self.world.player.max_hp);
        self.say(
                llm,
                "Player eats a simple meal. Feels a little stronger.",
                vec![
                    "gold_spent=5".into(),
                    "hp_gained=1".into(),
                    format!("hp_now={}/{}", self.world.player.hp, self.world.player.max_hp),
                    format!("gold_now={}", self.world.player.gold),
                ],
                Some(player_line),
                format!(
                    "You eat. -5 gold, +1 HP. Now HP {}/{}, gold {}.",
                    self.world.player.hp, self.world.player.max_hp, self.world.player.gold
                ),
            )
            .await
    }

    async fn sleep(
        &mut self,
        player_line: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        if self.world.location != Location::Upstairs {
            return self.say(
                    llm,
                    "Blocked — beds are upstairs.",
                    vec!["blocked=true".into()],
                    Some(player_line),
                    "Beds are upstairs. Go upstairs first.".into(),
                )
                .await;
        }
        if self.world.player.gold < 25 {
            return self.say(
                    llm,
                    "Blocked — not enough gold to rent a room.",
                    vec!["blocked=true".into(), "not_enough_gold=true".into()],
                    Some(player_line),
                    "Not enough gold (need 25).".into(),
                )
                .await;
        }
        self.world.player.gold -= 25;
        self.world.player.hp = self.world.player.max_hp;
        self.say(
                llm,
                "Player rents a room and sleeps. Fully healed by morning.",
                vec![
                    "gold_spent=25".into(),
                    "fully_healed=true".into(),
                    format!("gold_now={}", self.world.player.gold),
                ],
                Some(player_line),
                format!(
                    "You rent a room and sleep. Fully healed. Gold {}.",
                    self.world.player.gold
                ),
            )
            .await
    }

    async fn tip_sally(
        &mut self,
        player_line: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        if self.world.location != Location::MainHall {
            return self.say(
                    llm,
                    "Blocked — she's working the tables inside.",
                    vec!["blocked=true".into()],
                    Some(player_line),
                    "She's working the tables inside.".into(),
                )
                .await;
        }
        if self.world.player.gold < 5 {
            return self.say(
                    llm,
                    "Blocked — not enough gold to tip.",
                    vec!["blocked=true".into(), "not_enough_gold=true".into()],
                    Some(player_line),
                    "Not enough gold to tip (5).".into(),
                )
                .await;
        }
        self.world.player.gold -= 5;
        self.world.player.sally_tip_discount = true;
        self.say(
                llm,
                "Player tips her 5 gold. She lights up — warmer toward the player now.",
                vec![
                    "gold_spent=5".into(),
                    "tip_accepted=true".into(),
                    format!("gold_now={}", self.world.player.gold),
                ],
                Some(player_line),
                format!(
                    "You tip her 5 gold. She lights up — warmer toward you now. Gold {}.",
                    self.world.player.gold
                ),
            )
            .await
    }

    async fn accept_sword_trade(
        &mut self,
        player_line: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        if self.world.location != Location::MainHall {
            return self.say(
                    llm,
                    "Blocked — trading patron not here.",
                    vec!["blocked=true".into()],
                    Some(player_line),
                    "The trading patron is in the main hall.".into(),
                )
                .await;
        }
        if self.world.sword_offer_taken || !self.world.sword_offer_available {
            return self.say(
                    llm,
                    "Blocked — no sword trade on offer.",
                    vec!["blocked=true".into()],
                    Some(player_line),
                    "No sword trade is on offer.".into(),
                )
                .await;
        }
        if self.world.player.gold < 25 {
            return self.say(
                    llm,
                    "Blocked — need more gold for the trade.",
                    vec!["blocked=true".into(), "not_enough_gold=true".into()],
                    Some(player_line),
                    "Need 25 gold for the trade.".into(),
                )
                .await;
        }
        if self.world.player.weapon != Weapon::Knife {
            return self.say(
                    llm,
                    "Blocked — patron wants the knife specifically.",
                    vec!["blocked=true".into()],
                    Some(player_line),
                    "The patron wants your knife specifically.".into(),
                )
                .await;
        }
        self.world.player.gold -= 25;
        self.world.player.weapon = Weapon::RoyalSword;
        self.world.sword_offer_taken = true;
        self.world.sword_offer_available = false;
        self.say(
                llm,
                "A patron swaps a royal sword for 25 gold and the player's knife.",
                vec![
                    "gold_spent=25".into(),
                    "weapon_now=royal sword".into(),
                    format!("gold_now={}", self.world.player.gold),
                ],
                Some(player_line),
                format!(
                    "A non-Groll patron swaps a royal sword for 25 gold and your knife. Gold {}.",
                    self.world.player.gold
                ),
            )
            .await
    }

    async fn fight(
        &mut self,
        npc: Option<NpcId>,
        player_line: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        let target = npc.unwrap_or(match self.world.location {
            Location::StorageRoom => NpcId::Dremis,
            Location::Outside => NpcId::Dewr,
            _ => NpcId::Goblin,
        });

        if target == NpcId::Sally {
            return self.say(
                    llm,
                    "Blocked — she has no weapon and won't fight.",
                    vec!["blocked=true".into()],
                    Some(player_line),
                    "She has no weapon and won't fight you.".into(),
                )
                .await;
        }
        if target == NpcId::Dewr
            && !matches!(
                self.world.location,
                Location::Outside | Location::MainHall
            )
        {
            return self.say(
                    llm,
                    "Blocked — no goblin here to fight.",
                    vec!["blocked=true".into()],
                    Some(player_line),
                    "There's no goblin here to fight.".into(),
                )
                .await;
        }
        if target == NpcId::Dremis && self.world.location != Location::StorageRoom {
            return self.say(
                    llm,
                    "Blocked — no one here to fight.",
                    vec!["blocked=true".into()],
                    Some(player_line),
                    "There's no one here to fight.".into(),
                )
                .await;
        }
        if target == NpcId::Dremis && !self.world.player.aura_cleared {
            return self.say(
                    llm,
                    "Blocked — dread freezes the player mid-step; cannot fight.",
                    vec!["blocked=true".into(), "aura_pins=true".into()],
                    Some(player_line),
                    "The dread freezes you mid-step. You cannot fight like this.".into(),
                )
                .await;
        }

        let (enemy_hp, enemy_max, enemy_weapon, enemy_name) = match target {
            NpcId::Dewr => (
                self.world.dewr.hp,
                self.world.dewr.max_hp,
                self.world.dewr.weapon,
                "Dewr",
            ),
            NpcId::Groll => (
                self.world.groll.hp,
                self.world.groll.max_hp,
                self.world.groll.weapon,
                "Groll",
            ),
            NpcId::Dremis => (
                self.world.dremis.hp,
                self.world.dremis.max_hp,
                self.world.dremis.weapon,
                "Dremis",
            ),
            NpcId::Goblin | NpcId::Patron => (8, 8, Weapon::Knife, "Goblin"),
            NpcId::Sally => unreachable!(),
        };

        let mut events = self.say(
                llm,
                "Combat begins — steel flashes. Opening clash only; do not resolve the whole fight.",
                vec![
                    format!("foe={}", enemy_name),
                    "combat_started=true".into(),
                    format!("foe_hp={enemy_hp}/{enemy_max}"),
                ],
                Some(player_line),
                format!("Steel flashes. Combat with {enemy_name} begins!"),
            )
            .await;

        let mut player = Combatant {
            name: "You".into(),
            hp: self.world.player.hp,
            max_hp: self.world.player.max_hp,
            weapon: self.world.player.weapon,
        };
        let mut enemy = Combatant {
            name: enemy_name.into(),
            hp: enemy_hp,
            max_hp: enemy_max,
            weapon: enemy_weapon,
        };

        let mut swing_lines: Vec<String> = Vec::new();
        let mut swing_locks: Vec<String> = Vec::new();
        while player.is_alive() && enemy.is_alive() {
            metrics::inc_rpg_combat_round();
            let atk = resolve_attack(&player, &mut enemy, &mut self.rng);
            swing_lines.push(format_attack(&atk));
            swing_locks.push(format!(
                "{}→{} hit={} dmg={}",
                atk.attacker, atk.defender, atk.hit, atk.damage
            ));
            if !enemy.is_alive() {
                break;
            }
            let counter = resolve_attack(&enemy, &mut player, &mut self.rng);
            swing_lines.push(format_attack(&counter));
            swing_locks.push(format!(
                "{}→{} hit={} dmg={}",
                counter.attacker, counter.defender, counter.hit, counter.damage
            ));
        }

        // Mechanical beat-by-beat (no per-swing LLM).
        for line in &swing_lines {
            events.push(GameEvent::Narration(line.clone()));
        }

        self.world.player.hp = player.hp;
        match target {
            NpcId::Dewr => {
                self.world.dewr.hp = enemy.hp;
                self.world.dewr.alive = enemy.is_alive();
            }
            NpcId::Groll => {
                self.world.groll.hp = enemy.hp;
                self.world.groll.alive = enemy.is_alive();
            }
            NpcId::Dremis => {
                self.world.dremis.hp = enemy.hp;
                self.world.dremis.alive = enemy.is_alive();
            }
            _ => {}
        }

        if !enemy.is_alive() {
            let fallback = format!(
                "☠️ {enemy_name} falls. You stand at HP {}.",
                self.world.player.hp
            );
            let mut locked = vec![
                format!("foe_defeated={}", enemy_name),
                format!("player_hp={}", self.world.player.hp),
            ];
            locked.extend(swing_locks.into_iter().take(6));
            events.extend(self.say(
                    llm,
                    "Combat ends — foe falls. Summarize the clash in 2 sentences; honor locked hits/damage.",
                    locked,
                    Some(player_line),
                    fallback,
                )
                .await,
            );
            if target == NpcId::Dewr && !self.world.player.dewr_scam_cleared {
                events.extend(self.say(
                    llm,
                    "The doorway toll dies with him. No one left to collect.",
                    vec!["toll_dies_with_him=true".into()],
                    Some(player_line),
                    "The doorway toll dies with him. No one is left to collect.".into(),
                )
                .await);
                events.extend(self.clear_scam_and_enter(Some(player_line), llm).await);
            }
        } else if !player.is_alive() {
            events.extend(self.say(
                llm,
                "Player collapses under the assault.",
                vec!["player_down=true".into()],
                Some(player_line),
                "💀 You collapse under the assault.".into(),
            )
            .await);
        }

        events
    }

    async fn leave(
        &mut self,
        player_line: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        if self.world.player.has_amulet {
            self.ended = Some(SceneEnd::Victory);
            metrics::inc_rpg_victory();
            info!("scene end: victory");
            let mut events = self.say(
                llm,
                "Victory leave — open sky; a maiden on a horse greets the player. Scene ends in triumph.",
                vec!["victory=true".into(), "has_amulet=true".into()],
                Some(player_line),
                "🌅 You leave the tavern grounds. A maiden on a horse greets you beneath open sky. \
🏆 Victory! The scene ends here."
                    .into(),
            )
            .await;
            events.push(GameEvent::Ended(SceneEnd::Victory));
            return events;
        }

        let mut events = self.say(
                llm,
                "Bad leave — player leaves without what they came for. Six knives melt from the alley — ambush begins.",
                vec![
                    "ambush_starts=true".into(),
                    "goblin_count=6".into(),
                    "has_amulet=false".into(),
                ],
                Some(player_line),
                "You leave without what you came for. Six knives melt out of the alley — an ambush!"
                    .into(),
            )
            .await;

        let mut player = Combatant {
            name: "You".into(),
            hp: self.world.player.hp,
            max_hp: self.world.player.max_hp,
            weapon: self.world.player.weapon,
        };

        for i in 1..=6 {
            if !player.is_alive() {
                break;
            }
            let goblin = Combatant {
                name: format!("Goblin {i}"),
                hp: 8,
                max_hp: 8,
                weapon: Weapon::Knife,
            };
            while player.is_alive() && goblin.hp > 0 {
                let counter = resolve_attack(&goblin, &mut player, &mut self.rng);
                events.push(GameEvent::Narration(format_attack(&counter)));
                if !player.is_alive() {
                    break;
                }
                let mut g = goblin.clone();
                let atk = resolve_attack(&player, &mut g, &mut self.rng);
                events.push(GameEvent::Narration(format_attack(&atk)));
                if !g.is_alive() {
                    break;
                }
                if player.is_alive() && !atk.hit && !counter.hit {
                    player.hp -= 1;
                    events.extend(self.say(
                        llm,
                        "Ambush press — bodies wear the player down slightly.",
                        vec!["chip_damage=1".into(), format!("player_hp={}", player.hp)],
                        Some(player_line),
                        "The press of bodies wears you down (-1 HP).".into(),
                    )
                    .await);
                }
                break;
            }
        }

        if player.is_alive() {
            player.hp = 0;
            events.extend(self.say(
                llm,
                "Final ambush blow — last goblin finds a gap; ambush complete.",
                vec!["player_hp=0".into(), "ambush_complete=true".into()],
                Some(player_line),
                "The last goblin finds a gap in your guard. The ambush is complete.".into(),
            )
            .await);
        }

        self.world.player.hp = player.hp;
        self.ended = Some(SceneEnd::BadEnding);
        metrics::inc_rpg_bad_ending();
        info!("scene end: bad ending (ambush)");
        events.extend(self.say(
            llm,
            "Bad ending — player dies in the mud outside the tavern.",
            vec!["bad_ending=true".into(), "player_dead=true".into()],
            Some(player_line),
            "💀 You die in the mud outside the tavern. Bad ending.".into(),
        )
        .await);
        events.push(GameEvent::Ended(SceneEnd::BadEnding));
        events
    }
}

fn has_pay_intent(lower: &str) -> bool {
    lower.trim() == "pay"
        || lower.starts_with("pay ")
        || lower.contains("pay him")
        || lower.contains("pay the")
        || lower.contains("pay dewr")
        || lower.contains("hand over")
        || lower.contains("give him")
        || (lower.contains("pay")
            && (lower.contains("gold")
                || lower.contains("coin")
                || lower.contains("fee")
                || lower.contains("tax")
                || lower.contains("toll")))
}


fn prepend_player_feed(
    player_name: &str,
    input: &str,
    mut events: Vec<GameEvent>,
) -> Vec<GameEvent> {
    let parts = split_player_line(input);
    let feed = format_player_feed(input, player_name, &parts);
    if !feed.is_empty() {
        events.insert(0, GameEvent::Narration(feed));
    }
    events
}

fn format_action_outcome(success: bool, detail: &str) -> String {
    if success {
        format!("✅ Success — {detail}")
    } else {
        format!("❌ Failure — {detail}")
    }
}

fn first_sentence(text: &str) -> String {
    text.split(|c| c == '.' || c == '\n')
        .next()
        .unwrap_or(text)
        .trim()
        .to_string()
}

/// Derive a short outcome banner from locked rule facts (when present).
fn infer_action_outcome(locked: &[String], fallback: &str) -> Option<(bool, String)> {
    if locked
        .iter()
        .any(|o| o.starts_with("blocked=") || o.starts_with("blocked —"))
    {
        return Some((false, first_sentence(fallback)));
    }
    if let Some(raw) = locked.iter().find(|o| o.starts_with("success=")) {
        let ok = raw.strip_prefix("success=") == Some("true");
        let detail = format!(
            "{} check",
            locked
                .iter()
                .find_map(|o| o.strip_prefix("check="))
                .unwrap_or("action")
        );
        return Some((ok, detail));
    }
    if locked.iter().any(|o| o == "player_down=true" || o == "bad_ending=true") {
        return Some((false, first_sentence(fallback)));
    }
    if locked.iter().any(|o| {
        o.starts_with("arrived=")
            || o.starts_with("gained_")
            || o.contains("cleared=true")
            || o.contains("fully_healed=true")
            || o.contains("hp_gained=")
            || o.contains("foe_defeated=")
            || o.contains("entered_main_hall=true")
            || o.contains("claimed_seat=true")
            || o.contains("spoke_the_word=true")
            || o.contains("toll_cleared_without_pay=true")
            || o.contains("gold_spent=")
            || o.contains("victory=true")
    }) {
        return Some((true, first_sentence(fallback)));
    }
    None
}

fn format_check_report(result: &crate::checks::CheckResult) -> String {
    let mark = if result.success { "✅" } else { "❌" };
    let outcome = if result.success { "Success" } else { "Failure" };
    let prof = if result.proficient {
        format!("+prof {}", result.proficiency_bonus)
    } else {
        "untrained".into()
    };
    format!(
        "🎲 {} — {} ({}) d20 {} + {} ({prof}) = {} vs DC {}\n→ {mark} {outcome}",
        result.kind.label(),
        result.skill.label(),
        result.ability.label(),
        result.roll,
        result.modifier,
        result.total,
        result.dc,
    )
}

fn format_attack(r: &crate::combat::AttackResult) -> String {
    let verb_hit = if r.attacker == "You" { "hit" } else { "hits" };
    let verb_miss = if r.attacker == "You" { "miss" } else { "misses" };
    if r.hit {
        format!(
            "💥 {} {} {} for {} damage (hit · HP now {}).",
            r.attacker, verb_hit, r.defender, r.damage, r.defender_hp_after
        )
    } else {
        format!("💨 {} {} {} (miss).", r.attacker, verb_miss, r.defender)
    }
}

fn default_npc_line(npc: NpcId, world: &WorldState) -> String {
    match npc {
        NpcId::Dewr => {
            if world.player.dewr_scam_cleared {
                if world.player.dewr_backed_down {
                    "Yeah, yeah. I'm busy. Go bother the cloak in the corner.".into()
                } else {
                    "Heh. Softskin got lucky. Don't expect a refund.".into()
                }
            } else {
                "Door tax. Ten gold. Official-like. Pay, or keep dreaming about that warm ale."
                    .into()
            }
        }
        NpcId::Sally => {
            if world.player.sally_charmed {
                "Saved you a smile, traveler. Don't spend it all at once.".into()
            } else {
                "Welcome in. Tips keep the ale flowing — and my patience.".into()
            }
        }
        NpcId::Groll => {
            if world.player.has_password {
                "You got what you came for. Leave me to my drink.".into()
            } else {
                "Move along. Cloaks have ears, and mine are done listening.".into()
            }
        }
        NpcId::Dremis => {
            if world.player.has_amulet {
                "Go. What you carry is enough — do not waste it.".into()
            } else if !world.player.aura_cleared {
                if world.player.has_password {
                    "You feel a word burning on your tongue. He waits.".into()
                } else {
                    "The dread thickens. He waits for something you do not have.".into()
                }
            } else {
                "Prove you belong — or leave empty-handed.".into()
            }
        }
        NpcId::Patron => "Another round? Or are you here for trouble?".into(),
        NpcId::Goblin => "What you lookin' at?".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hexbee_provider::FakeLlm;

    #[tokio::test]
    async fn tip_then_charm_compound_chat() {
        let mut game = TavernGame::new(Some(5));
        game.world.player.dewr_scam_cleared = true;
        game.world.location = Location::MainHall;
        let gold_before = game.world.player.gold;
        let events = game
            .handle_line("I tip Sally some gold then try to charm her", None)
            .await;
        assert!(
            game.world.player.sally_tip_discount
                || game.world.player.sally_charmed
                || game.world.player.gold < gold_before,
            "expected tip and/or charm to resolve, got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(e, GameEvent::Narration(s) if
                s.contains("tip") || s.contains("charm") || s.contains("push the moment") || s.contains("lights up"))),
            "expected tip/charm narration, got {events:?}"
        );
    }

    #[tokio::test]
    async fn chat_refuse_scam_is_not_just_talk() {
        let mut game = TavernGame::new(Some(1));
        assert_eq!(game.world.location, Location::Outside);
        let gold_before = game.world.player.gold;
        let events = game
            .handle_line(
                "I'm not paying your fake door tax, you little scam artist",
                None,
            )
            .await;
        // Must attempt the contested call-out — not mere talk, not paying.
        assert_eq!(game.world.player.gold, gold_before, "refuse must not pay");
        assert!(
            events.iter().any(|e| matches!(e, GameEvent::Narration(s) if
                s.contains("🎲")
                    || s.contains("push the moment")
                    || s.contains("bluff")
                    || s.contains("laughs")
                    || s.contains("way is clear")
                    || s.contains("flatten"))),
            "expected scam check narration, got {events:?}"
        );
    }

    #[tokio::test]
    async fn intimidate_is_challenge_not_combat_or_pay() {
        let mut game = TavernGame::new(Some(1));
        let gold_before = game.world.player.gold;
        let events = game
            .handle_line(
                "I draw my knife to intimidate the goblin — move aside or else",
                None,
            )
            .await;
        assert_eq!(game.world.player.gold, gold_before, "intimidate must not pay");
        assert!(
            game.world.dewr.alive,
            "intimidate must not auto-kill Dewr"
        );
        assert!(
            events.iter().any(|e| matches!(e, GameEvent::Narration(s) if
                s.contains("🎲")
                    || s.contains("push the moment")
                    || s.contains("backs down")
                    || s.contains("flatten")
                    || s.contains("cackles")
                    || s.contains("way is clear"))),
            "expected challenge check, got {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(e, GameEvent::Narration(s) if s.contains("Combat"))),
            "intimidate must not start combat, got {events:?}"
        );
    }

    #[tokio::test]
    async fn contested_emits_dice_report() {
        let mut game = TavernGame::new(Some(1));
        let events = game
            .handle_line(
                "I draw my knife to intimidate the goblin — move aside or else",
                None,
            )
            .await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("🎲") && (s.contains("d20") || s.contains("rolled") || s.contains("vs DC"))
            )),
            "expected dice report, got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("✅ Success") || s.contains("❌ Failure")
            )),
            "expected explicit outcome banner, got {events:?}"
        );
    }

    #[tokio::test]
    async fn act_and_talk_do_not_double_echo_player_line() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let talk = game.handle_line("hello", None).await;
        assert!(
            !talk.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("💬 You:")
            )),
            "talk must not echo player line, got {talk:?}"
        );
        let act = game.handle_line("look around", None).await;
        assert!(
            !act.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("🎬 You:")
            )),
            "act must not echo player line, got {act:?}"
        );
        assert!(
            act.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.starts_with("🪶")
            )),
            "look should narrate scene, got {act:?}"
        );
    }

    #[tokio::test]
    async fn sneak_past_shows_stealth_roll() {
        let mut game = TavernGame::new(Some(1));
        let events = game.handle_line("Sneak past the Goblin", None).await;
        assert!(
            !events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.starts_with("🎬 You:")
            )),
            "must not echo player line, got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("🎲")
                    && s.contains("Stealth")
                    && s.contains("d20")
                    && s.contains("vs DC")
            )),
            "expected stealth roll report, got {events:?}"
        );
    }

    #[tokio::test]
    async fn blocked_go_shows_failure_outcome() {
        let mut game = TavernGame::new(Some(1));
        let events = game.handle_line("enter the hall", None).await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("❌ Failure") || s.contains("❌ Blocked")
            )),
            "expected failure outcome, got {events:?}"
        );
    }

    #[tokio::test]
    async fn combat_swings_are_ai_narrated_with_locked_outcomes() {
        let mut game = TavernGame::new(Some(7));
        // Opening + closing paints; per-swing is mechanical template.
        let llm = FakeLlm::new([
            r#"{"line":"OPENING_CLASH with the goblin."}"#,
            r#"{"line":"CLOSING_BEAT — he drops."}"#,
            r#"{"line":"Toll dies with him."}"#,
            r#"{"line":"You step inside."}"#,
        ]);
        let events = game
            .fight(Some(NpcId::Dewr), "I attack the goblin", Some(&llm as &dyn LlmClient))
            .await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, GameEvent::Narration(s) if s.contains("OPENING_CLASH"))),
            "expected AI opening combat narration, got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(e, GameEvent::Narration(s) if
                s.contains("hit") || s.contains("miss") || s.contains("CLOSING"))),
            "expected combat exchanges, got {events:?}"
        );
    }

    #[tokio::test]
    async fn junk_intimidate_plan_cannot_pay_after_fight() {
        let game = TavernGame::new(Some(1));
        // Simulate a bad LLM plan: impress → fight → pay while player only intimidated.
        let plan = ActionPlan {
            steps: vec![
                ActionTag {
                    action: "impress_dremis".into(),
                    npc: Some("dremis".into()),
                    location: None,
                    confidence: Some(2),
                    reason: Some("junk".into()),
                },
                ActionTag {
                    action: "fight".into(),
                    npc: Some("dewr".into()),
                    location: None,
                    confidence: Some(2),
                    reason: Some("junk".into()),
                },
                ActionTag {
                    action: "pay_scam".into(),
                    npc: Some("dewr".into()),
                    location: None,
                    confidence: Some(2),
                    reason: Some("junk".into()),
                },
            ],
            summary: Some("junk".into()),
        };
        let cleaned = game.sanitize_plan(
            plan,
            "I pull out my knife to intimidate the goblin",
        );
        assert_eq!(cleaned.steps.len(), 1);
        assert_eq!(cleaned.steps[0].action, "challenge_dewr");
    }

    #[tokio::test]
    async fn killing_dewr_clears_scam_without_pay() {
        let mut game = TavernGame::new(Some(7));
        let gold_before = game.world.player.gold;
        // Force a fight that will eventually kill Dewr (8 HP, knife 1-3).
        let mut finished = false;
        for _ in 0..30 {
            if !game.world.dewr.alive {
                finished = true;
                break;
            }
            if game.ended.is_some() {
                break;
            }
            // Reset player HP so we can keep fighting.
            game.world.player.hp = game.world.player.max_hp;
            game.world.location = Location::Outside;
            game.handle_line("I attack the goblin with my knife", None).await;
        }
        assert!(finished || !game.world.dewr.alive, "expected Dewr to die");
        assert!(game.world.player.dewr_scam_cleared);
        assert_eq!(
            game.world.player.gold, gold_before,
            "Dewr must not be paid after death"
        );
    }

    #[tokio::test]
    async fn describe_scene_arrival_is_single_line() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        let lines = game.describe_scene_arrival(None, None).await;
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("🪶 "));
        assert!(lines[0].to_lowercase().contains("hall") || lines[0].contains("smoky"));
    }

    #[tokio::test]
    async fn describe_scene_change_has_transition_and_arrival() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        let lines = game
            .describe_scene_change(None, Some("enter"), Location::Outside)
            .await;
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("🪶 "));
        assert!(lines[1].starts_with("🪶 "));
        assert!(
            lines[0].to_lowercase().contains("door") || lines[0].to_lowercase().contains("tavern"),
            "expected transition, got {:?}",
            lines[0]
        );
        assert!(
            lines[1].to_lowercase().contains("hall") || lines[1].contains("smoky"),
            "expected arrival, got {:?}",
            lines[1]
        );
    }

    #[tokio::test]
    async fn pay_scam_enters_tavern() {
        let mut game = TavernGame::new(Some(1));
        assert_eq!(game.world.location, Location::Outside);
        let blocked = game.handle_line("enter", None).await;
        assert!(blocked.iter().any(|e| matches!(e, GameEvent::Narration(s) if s.contains("blocks"))));
        assert_eq!(game.world.location, Location::Outside);
        game.handle_line("pay", None).await;
        assert!(game.world.player.dewr_scam_cleared);
        assert_eq!(game.world.location, Location::MainHall);
        assert_eq!(game.world.player.gold, 90);
    }

    #[tokio::test]
    async fn password_path_victory() {
        let mut game = TavernGame::new(Some(1));
        game.world.player.has_password = true;
        game.world.location = Location::StorageRoom;
        let events = game.handle_line("give password", None).await;
        assert!(game.world.player.has_amulet);
        assert!(game.world.player.aura_cleared);
        assert!(events.iter().any(|e| matches!(e, GameEvent::Narration(_))));

        let events = game.handle_line("leave", None).await;
        assert!(events
            .iter()
            .any(|e| matches!(e, GameEvent::Ended(SceneEnd::Victory))));
    }

    #[tokio::test]
    async fn leave_without_amulet_bad_ending() {
        let mut game = TavernGame::new(Some(2));
        let events = game.handle_line("leave", None).await;
        assert!(events
            .iter()
            .any(|e| matches!(e, GameEvent::Ended(SceneEnd::BadEnding))));
        assert!(game.world.player.hp <= 0);
    }

    #[tokio::test]
    async fn impress_with_high_attr_and_seed() {
        let mut game = TavernGame::new(Some(99));
        game.world.location = Location::StorageRoom;
        game.world.player.aura_cleared = true;
        // Face build: Persuasion proficient, boost CHA for reliable success vs DC 16.
        game.world.player.build = crate::skills::CharacterBuild::from_preset(
            crate::skills::BuildPreset::Face,
        );
        game.world.player.build.abilities.cha = 20; // +5 +2 prof = +7
        let mut won = false;
        for _ in 0..30 {
            game.handle_line(
                "impress dremis — I honor your wings and offer courage without insult",
                None,
            )
            .await;
            if game.world.player.has_amulet {
                won = true;
                break;
            }
        }
        assert!(won);
    }

    #[tokio::test]
    async fn food_and_sleep() {
        let mut game = TavernGame::new(Some(3));
        game.world.player.hp = 5;
        game.world.location = Location::Kitchen;
        game.handle_line("buy food", None).await;
        assert_eq!(game.world.player.hp, 6);
        assert_eq!(game.world.player.gold, 95);
        game.world.location = Location::Upstairs;
        game.handle_line("sleep", None).await;
        assert_eq!(game.world.player.hp, 10);
        assert_eq!(game.world.player.gold, 70);
    }

    #[tokio::test]
    async fn untargeted_hall_talk_asks_who() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let events = game.handle_line("hello", None).await;
        let text = events
            .iter()
            .filter_map(|e| match e {
                GameEvent::Narration(s) => Some(s.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("Who are you speaking to"),
            "expected talk bind prompt, got {text}"
        );
    }

    #[tokio::test]
    async fn talk_groll_in_hall_sticks_focus() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let events = game.handle_line("talk to groll", None).await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("Groll")
            )),
            "expected Groll dialogue, got {events:?}"
        );
        assert_eq!(game.scene.last_talk, Some(NpcId::Groll));
        // Untargeted follow-up should stick to Groll.
        let events = game.handle_line("what do you know", None).await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("Groll")
            )),
            "expected sticky Groll talk, got {events:?}"
        );
    }

    #[tokio::test]
    async fn outside_goblin_talk_is_dewr() {
        let mut game = TavernGame::new(Some(1));
        let events = game.handle_line("talk to the goblin", None).await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("Dewr")
            )),
            "goblin should canonicalize to Dewr, got {events:?}"
        );
        assert_eq!(game.scene.last_talk, Some(NpcId::Dewr));
    }

    #[tokio::test]
    async fn gobbo_slang_talk_is_dewr() {
        let mut game = TavernGame::new(Some(1));
        let events = game.handle_line("hey gobbo, what do you want?", None).await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("Dewr")
            )),
            "gobbo should resolve to Dewr, got {events:?}"
        );
        assert_eq!(game.scene.last_talk, Some(NpcId::Dewr));
    }

    #[test]
    fn scene_fingerprint_changes_when_dewr_dies() {
        let mut game = TavernGame::new(Some(1));
        let before = game.scene_fingerprint();
        game.world.dewr.alive = false;
        assert_ne!(before, game.scene_fingerprint());
    }

    #[test]
    fn status_line_includes_player_name() {
        let mut game = TavernGame::new(Some(1));
        game.set_player_name("Mira");
        let line = game.status_line();
        assert!(line.contains("Mira"), "got {line}");
        assert!(line.contains("HP"), "got {line}");
        assert_eq!(
            game.status_line_for("Sally").contains("Sally"),
            true,
            "hot-seat name"
        );
    }

    #[tokio::test]
    async fn ambient_carter_is_talkable_in_hall() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        game.scene.register_ambient(
            Location::MainHall,
            vec![CharacterCard::ambient(
                "carter",
                "a tired carter",
                "weary",
                "A road-dusted carter nurses a mug.",
                "plain",
            )],
        );
        let events = game.handle_line("talk to the carter", None).await;
        let text = events
            .iter()
            .filter_map(|e| match e {
                GameEvent::Narration(s) => Some(s.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.to_lowercase().contains("carter") || text.contains("💬"),
            "expected ambient carter talk, got {text}"
        );
        assert_eq!(game.scene.last_talk_ambient.as_deref(), Some("carter"));
    }

    #[tokio::test]
    async fn look_seeds_ambient_without_llm() {
        let mut game = TavernGame::new(Some(2));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        game.handle_line("look", None).await;
        assert!(game.scene.has_ambient_seeded(Location::MainHall));
        assert!(!game.scene.ambient_here(Location::MainHall).is_empty());
    }

    #[tokio::test]
    async fn query_inventory_reads_store() {
        let mut game = TavernGame::new(Some(1));
        let events = game.handle_line("inventory", None).await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.to_lowercase().contains("knife")
                    || s.to_lowercase().contains("gold")
                    || s.to_lowercase().contains("carry")
            )),
            "expected inventory read, got {events:?}"
        );
    }

    #[tokio::test]
    async fn impossible_charm_outside_is_talk_miss_not_act() {
        let mut game = TavernGame::new(Some(1));
        assert_eq!(game.world.location, Location::Outside);
        let gold = game.world.player.gold;
        let events = game.handle_line("charm sally", None).await;
        assert_eq!(game.world.player.gold, gold);
        assert!(!game.world.player.sally_charmed);
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("💬 Talk —") && s.contains("isn't here")
            )),
            "expected talk miss, got {events:?}"
        );
    }

    #[tokio::test]
    async fn natural_persuasion_runs_check_then_npc_reply() {
        let llm = hexbee_provider::FakeLlm::new([r#"{"line":"*sighs* \"…remember this word.\""}"#]);
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let events = game
            .handle_line(
                "say to groll: surely you know a word for the hidden door?",
                Some(&llm as &dyn hexbee_provider::LlmClient),
            )
            .await;
        assert!(
            events.iter().any(|e| matches!(e, GameEvent::Narration(s) if s.contains("🎲"))),
            "expected skill roll, got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("🐉 Groll:") || s.contains("🐉 groll:") || s.contains("Groll:")
            )),
            "expected NPC reply after check, got {events:?}"
        );
    }

    #[tokio::test]
    async fn blue_pants_is_banter_only() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let events = game
            .handle_line("say to groll: my pants are blue", None)
            .await;
        assert!(
            !events.iter().any(|e| matches!(e, GameEvent::Narration(s) if s.contains("🎲"))),
            "banter must not roll, got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("🐉 Groll:") || s.contains("🐉 groll:") || s.contains("Groll:")
            )),
            "expected NPC banter, got {events:?}"
        );
    }

    #[tokio::test]
    async fn talk_npc_replies_without_player_echo() {
        let llm = hexbee_provider::FakeLlm::new([r#"{"line":"*grins* \"Well hello there, traveler.\""}"#]);
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let events = game
            .handle_line(
                "say to sally: I like your apron",
                Some(&llm as &dyn hexbee_provider::LlmClient),
            )
            .await;
        assert!(
            !events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("💬 You:")
            )),
            "must not echo player line, got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("🍺 Sally:") || s.contains("🍺 sally:") || (s.contains("Sally:") && s.contains("💬"))
            )),
            "expected NPC reply, got {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("❌ Failure") || s.contains("❌ Blocked")
            )),
            "talk must not show failure/blocked, got {events:?}"
        );
    }

    #[tokio::test]
    async fn clarify_kernel_word_asks_before_command() {
        let mut game = TavernGame::new(Some(1));
        let events = game
            .handle_line("I yell options at the goblin", None)
            .await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.starts_with("❓") && s.contains('1')
            )),
            "expected numbered clarify, got {events:?}"
        );
        assert!(game.pending_clarify.is_some());
    }

    #[tokio::test]
    async fn shrug_after_clarify_commits_flavor_not_second_ask() {
        let mut game = TavernGame::new(Some(1));
        game.handle_line("I yell options at the goblin", None).await;
        let events = game.handle_line("whatever", None).await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("👺 Dewr:") || s.contains("💬 Talk —")
            )),
            "expected flavor talk after shrug, got {events:?}"
        );
        assert!(game.pending_clarify.is_none());
        assert!(
            !events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.starts_with("❓")
            )),
            "must not chain clarify prompts"
        );
    }

    #[tokio::test]
    async fn talk_ambiguous_shows_who_not_failure() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let events = game.handle_line("hello", None).await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("💬 Talk —") && s.contains("Who are you speaking to")
            )),
            "expected talk bind prompt, got {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("❌ Failure") || s.contains("❌ Blocked")
            )),
            "ambiguous talk is not a failed check, got {events:?}"
        );
    }

    #[tokio::test]
    async fn talk_mode_with_social_flag_still_gets_npc_reply() {
        let llm = hexbee_provider::FakeLlm::new([r#"{"line":"*nods* \"Sure thing, hon.\""}"#]);
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let purpose = crate::organs::PurposeResult {
            mode: crate::rules::TurnMode::Talk,
            targets: vec!["sally".into()],
            summary: Some("chat".into()),
            action_hint: Some("talk".into()),
            social_act: true,
            confidence: None,
        };
        let events = game
            .handle_talk("hey sally, nice night", &purpose, Some(&llm as &dyn hexbee_provider::LlmClient))
            .await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("🍺 Sally:") || s.contains("🍺 sally:") || (s.contains("Sally:") && s.contains("💬"))
            )),
            "talk mode should still invoke Actor, got {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("🎲") || s.contains("❌ Failure")
            )),
            "plain talk must not roll or fail, got {events:?}"
        );
    }

    #[tokio::test]
    async fn talk_keeps_targeted_npc_reply() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let line = "say to sally: I like your apron";
        let events = game.handle_line(line, None).await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("🍺 Sally:") || s.contains("🍺 sally:") || (s.contains("Sally:") && s.contains("💬"))
            )),
            "expected Sally reply, got {events:?}"
        );
    }

    #[tokio::test]
    async fn check_report_is_d20_plus_attr() {
        let mut game = TavernGame::new(Some(1));
        let events = game
            .handle_line(
                "I draw my knife to intimidate the goblin — move aside or else",
                None,
            )
            .await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("🎲") && s.contains("d20") && s.contains("Intimidation")
            )),
            "expected d20+skill report, got {events:?}"
        );
    }

    #[tokio::test]
    async fn query_sheet_shows_fivee_abilities() {
        let mut game = TavernGame::new(Some(1));
        game.world.player.build =
            crate::skills::CharacterBuild::from_preset(crate::skills::BuildPreset::Face);
        let events = game.handle_line("sheet", None).await;
        let text = events
            .iter()
            .filter_map(|e| match e {
                GameEvent::Narration(s) => Some(s.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("STR"), "got {text}");
        assert!(text.contains("CHA"), "got {text}");
        assert!(text.contains("Persuasion") || text.contains("Proficient"), "got {text}");
    }

    #[tokio::test]
    async fn talk_miss_without_actor_reply() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;
        let events = game.handle_line("talk to dremis", None).await;
        assert!(
            !events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("💬 You:")
            )),
            "must not echo player line, got {events:?}"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("💬 Talk —") && s.contains("isn't here")
            )),
            "expected talk miss (not a skill failure), got {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("💬 Dremis:") || s.contains("🎲")
            )),
            "Actor must not run when target absent, got {events:?}"
        );
    }

    #[tokio::test]
    async fn contested_block_is_static_no_dice() {
        let mut game = TavernGame::new(Some(1));
        assert_eq!(game.world.location, Location::Outside);
        let events = game.handle_line("charm sally", None).await;
        assert!(
            events.iter().all(|e| match e {
                GameEvent::Narration(s) => !s.contains("🎲"),
                _ => true,
            }),
            "blocked check must not roll, got {events:?}"
        );
    }

    #[tokio::test]
    async fn screenshot_bind_failures_in_main_hall() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;
        game.world.player.dewr_scam_cleared = true;

        let sit = game.handle_line("Take a seat.", None).await;
        assert!(
            sit.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.starts_with("🪶") && s.contains("seat")
            )),
            "sit should narrate, not talk bind, got {sit:?}"
        );
        assert!(
            !sit.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("Who are you speaking to")
            )),
            "sit must not hit talk bind, got {sit:?}"
        );

        let animals = game.handle_line("Are there any animals in the Tavern?", None).await;
        assert!(
            animals.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("No animals")
            )),
            "undiscovered soft flags stay hidden from room queries, got {animals:?}"
        );

        let roster_before = game.present_roster_lines().join("\n");
        assert!(
            !roster_before.contains("kitty"),
            "soft detail must not appear in Present until observed, got {roster_before}"
        );

        let pet_early = game.handle_line("I pet the sleeping kitty.", None).await;
        assert!(
            pet_early.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.contains("There is no sleeping")
            )),
            "cannot interact with undiscovered soft detail, got {pet_early:?}"
        );

        let look_pets = game.handle_line("I look for pets.", None).await;
        assert!(
            look_pets.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.starts_with("🪶") && s.contains("kitty")
            )),
            "look-for should discover and narrate soft flag, got {look_pets:?}"
        );

        let pet_kitty = game.handle_line("I pet the sleeping kitty.", None).await;
        assert!(
            pet_kitty.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.starts_with("🪶")
            )),
            "discovered soft detail should get a dynamic beat, got {pet_kitty:?}"
        );

        let roster = game.present_roster_lines().join("\n");
        assert!(roster.contains("kitty"), "observed soft detail may appear in Present, got {roster}");
        assert!(roster.contains("hearth"), "got {roster}");
        assert!(
            !roster.contains("kitchen door"),
            "hidden props stay off roster until discovered, got {roster}"
        );
    }

    #[tokio::test]
    async fn soft_flag_sally_hum_on_watch() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::MainHall;

        let roster_before = game.present_roster_lines().join("\n");
        assert!(
            !roster_before.to_lowercase().contains("hum"),
            "Sally's hum stays hidden until observed, got {roster_before}"
        );

        let watch = game.handle_line("I watch Sally.", None).await;
        assert!(
            watch.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.starts_with("🪶")
                    && s.to_lowercase().contains("hum")
            )),
            "watch should discover Sally's soft detail, got {watch:?}"
        );

        let roster = game.present_roster_lines().join("\n");
        assert!(
            roster.to_lowercase().contains("hum"),
            "observed soft detail may appear in Present, got {roster}"
        );
    }

    #[tokio::test]
    async fn soft_flag_kitchen_cook_on_look_for() {
        let mut game = TavernGame::new(Some(1));
        game.world.location = Location::Kitchen;

        let roster_before = game.present_roster_lines().join("\n");
        assert!(
            !roster_before.to_lowercase().contains("cook"),
            "kitchen cook stays hidden until observed, got {roster_before}"
        );

        let look = game.handle_line("I look for the cook.", None).await;
        assert!(
            look.iter().any(|e| matches!(
                e,
                GameEvent::Narration(s) if s.starts_with("🪶")
                    && s.to_lowercase().contains("goblin")
            )),
            "look-for should discover kitchen cook soft flag, got {look:?}"
        );

        let roster = game.present_roster_lines().join("\n");
        assert!(
            roster.to_lowercase().contains("cook"),
            "observed soft detail may appear in Present, got {roster}"
        );
    }
}
