use hexbee_kernel::{HexContext, HexModule, HexOutput, Intent, IntentKind};
use hexbee_store::Campaign;
// use tracing::info;

use crate::bridge::{default_world_from_campaign, patches_from_world, sync_world_to_campaign};
use crate::game::{GameEvent, SceneEnd, TavernGame};
use crate::organs::GmOrgan;

pub struct TavernHex {
    pub game: TavernGame,
    intro_shown: bool,
}

impl TavernHex {
    pub fn new(campaign: &Campaign) -> Self {
        let seed = campaign.state.rng_seed;
        let mut game = TavernGame::new(seed);
        game.world = default_world_from_campaign(campaign);
        game.set_world_doc(crate::world_doc::WorldDoc::parse(&campaign.world_md));
        game.load_secrets_from(&campaign.root.join("secrets.yaml"));
        game.set_goal_nudge(crate::organs::GmOrgan::next_open(
            &campaign.goals,
            &campaign.state.runtime,
        ));
        game.turn_start_location = game.world.location;
        Self {
            game,
            intro_shown: false,
        }
    }

    pub fn open_bundled() -> (Self, Campaign) {
        let campaign = Campaign::load(bundled_campaign_dir()).expect("bundled campaign");
        let hex = Self::new(&campaign);
        (hex, campaign)
    }

    pub fn reset_from_campaign(&mut self, campaign: &Campaign) {
        let seed = campaign.state.rng_seed;
        let mut game = TavernGame::new(seed);
        game.world = default_world_from_campaign(campaign);
        game.set_world_doc(crate::world_doc::WorldDoc::parse(&campaign.world_md));
        game.load_secrets_from(&campaign.root.join("secrets.yaml"));
        game.set_goal_nudge(crate::organs::GmOrgan::next_open(
            &campaign.goals,
            &campaign.state.runtime,
        ));
        game.turn_start_location = game.world.location;
        self.game = game;
        self.intro_shown = false;
    }

    fn flush_world(&mut self, campaign: &mut hexbee_store::Campaign) {
        sync_world_to_campaign(campaign, &self.game.world);
        if let Some(yaml) = self.game.take_world_yaml() {
            campaign.world_md = yaml;
            campaign.world_kind = hexbee_store::WorldFileKind::Yaml;
            campaign.dirty = true;
        }
    }

    /// Saved session with a character ready — not a fresh street intro.
    pub fn is_resumed_session(&self, campaign: &Campaign) -> bool {
        if self.needs_chargen(campaign) {
            return false;
        }
        if !campaign.transcript.beats.is_empty() {
            return true;
        }
        let fresh = crate::state::WorldState::default();
        let w = &self.game.world;
        w.location != fresh.location
            || w.player.met_dewr
            || w.player.dewr_scam_cleared
            || w.player.gold != fresh.player.gold
            || w.player.has_password
            || w.player.has_amulet
    }
}

/// Path to bundled campaign shipped with hexbee-hex-rpg.
pub fn bundled_campaign_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("campaign")
}

impl HexModule for TavernHex {
    fn id(&self) -> &str {
        "tavern"
    }

    fn handle_intent(
        &mut self,
        ctx: &mut HexContext<'_>,
        intent: &Intent,
    ) -> Result<HexOutput, String> {
        hexbee_core::metrics::inc_rpg_turn();
        self.game.set_goal_nudge(crate::organs::GmOrgan::next_open(
            &ctx.campaign.goals,
            &ctx.campaign.state.runtime,
        ));

        if intent.kind == IntentKind::Query {
            if intent.raw.to_lowercase().contains("goal") {
                return Ok(HexOutput {
                    lines: GmOrgan::goal_status(&ctx.campaign.goals, &ctx.campaign.state.runtime),
                    ..Default::default()
                });
            }
            // Route store reads through the turn loop (inventory / sheet / who).
            let events = futures_helper(&mut self.game, &intent.raw, ctx.llm)?;
            let mut lines = Vec::new();
            for event in events {
                if let GameEvent::Narration(text) = event {
                    for part in text.split('\n') {
                        let part = part.trim();
                        if !part.is_empty() {
                            lines.push(part.to_string());
                        }
                    }
                }
            }
            self.flush_world(ctx.campaign);
            return Ok(HexOutput {
                lines,
                organ_trace: vec!["query".into()],
                ..Default::default()
            });
        }

        let old_world = self.game.world.clone();

        let llm = ctx.llm;
        let input = &intent.raw;

        let events = futures_helper(&mut self.game, input, llm)?;

        let mut lines = Vec::new();
        let mut ended = false;
        let mut narration = None;

        for event in events {
            match event {
                GameEvent::Narration(text) => {
                    narration = Some(text.clone());
                    for part in text.split('\n') {
                        let part = part.trim();
                        if !part.is_empty() {
                            lines.push(part.to_string());
                        }
                    }
                }
                GameEvent::Ended(end) => {
                    ended = true;
                    lines.push(format!("🏁 Scene complete: {end:?}"));
                    let ending = match end {
                        SceneEnd::Victory => "victory",
                        SceneEnd::Death => "death",
                        SceneEnd::BadEnding => "bad_ending",
                    };
                    ctx.campaign.state.runtime["ending"] =
                        serde_json::Value::String(ending.into());
                }
            }
        }

        let mut actor_patches = Vec::new();
        if !ended && self.game.line_spends_turn(input) {
            let summary = turn_summary(&ctx.actor_id, input, &lines);
            let extra = self.advance_table(ctx, &summary);
            narration = compose_narration(narration, extra.narration);
            lines.extend(extra.lines);
            actor_patches = extra.patches;
        }

        self.flush_world(ctx.campaign);
        let mut patches = patches_from_world(&old_world, &self.game.world);
        patches.extend(actor_patches);

        Ok(HexOutput {
            lines,
            patches,
            narration,
            organ_trace: vec![
                "purpose".into(),
                "legality".into(),
                "check".into(),
                "director".into(),
                "narrator".into(),
            ],
            ended,
        })
    }

    fn pass_seat(&mut self, ctx: &mut HexContext<'_>) -> Result<Option<HexOutput>, String> {
        let summary = format!("{}: passes", ctx.actor_id);
        let extra = self.advance_table(ctx, &summary);
        if extra.skipped {
            return Ok(None);
        }
        self.flush_world(ctx.campaign);
        let mut patches = patches_from_world(&self.game.world, &self.game.world);
        patches.extend(extra.patches);
        Ok(Some(HexOutput {
            lines: extra.lines,
            patches,
            narration: extra.narration,
            organ_trace: vec!["round".into()],
            ended: false,
        }))
    }

    fn sticky_header(&self, campaign: &Campaign, include_intro: bool) -> Vec<String> {
        let _ = campaign;
        let mut h = Vec::new();
        if include_intro {
            h.push(self.game.intro());
            h.push(String::new());
        } else {
            h.push(format!("📍 {}", self.game.world.location.display_name()));
            h.push(String::new());
        }
        h.extend(self.game.present_roster_lines());
        h
    }

    fn status_footer(&self, campaign: &Campaign) -> String {
        let name = current_actor_display_name(campaign, self.game.player_display_name());
        self.game.status_line_for(&name)
    }

    fn scene_fingerprint(&self) -> u64 {
        self.game.scene_fingerprint()
    }

    fn scene_paint_fingerprint(&self) -> u64 {
        self.game.scene_paint_fingerprint()
    }

    fn scene_change_lines(
        &mut self,
        llm: Option<&dyn hexbee_provider::LlmClient>,
        player_line: Option<&str>,
    ) -> Vec<String> {
        if self.game.take_arrival_narrated() {
            return Vec::new();
        }
        if tokio::runtime::Handle::try_current().is_ok() {
            return tokio::task::block_in_place(|| {
                let from = self.game.turn_start_location;
                tokio::runtime::Handle::current().block_on(
                    self.game.describe_scene_change(llm, player_line, from),
                )
            });
        }
        Vec::new()
    }

    fn scene_arrival_lines(
        &mut self,
        llm: Option<&dyn hexbee_provider::LlmClient>,
        player_line: Option<&str>,
    ) -> Vec<String> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(self.game.describe_scene_arrival(llm, player_line))
            });
        }
        Vec::new()
    }

    fn is_resumed_session(&self, campaign: &Campaign) -> bool {
        TavernHex::is_resumed_session(self, campaign)
    }

    fn refresh_living(
        &mut self,
        llm: Option<&dyn hexbee_provider::LlmClient>,
        player_line: Option<&str>,
    ) -> String {
        self.scene_change_lines(llm, player_line)
            .join("\n")
    }

    /// Seed ambient cast without LLM scene prose (Present roster is the roster).
    fn take_world_yaml(&mut self) -> Option<String> {
        self.game.take_world_yaml()
    }

    fn prepare_scene(&mut self, llm: Option<&dyn hexbee_provider::LlmClient>) {
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(self.game.prepare_scene(llm))
            });
        }
    }

    fn scene_header(&self, campaign: &Campaign) -> Vec<String> {
        self.sticky_header(campaign, !self.intro_shown)
    }

    fn is_over(&self) -> bool {
        self.game.is_over()
    }

    fn reset(&mut self, campaign: &Campaign) {
        self.reset_from_campaign(campaign);
    }

    fn set_player_name(&mut self, name: &str) {
        self.game.set_player_name(name);
    }

    fn player_name(&self) -> String {
        self.game.player_display_name().into()
    }

    fn apply_character_build(&mut self, build_json: &serde_json::Value) -> Result<(), String> {
        let build = crate::skills::CharacterBuild::from_stats_json(build_json)
            .ok_or_else(|| "invalid character build".to_string())?;
        self.game.set_character_build(build);
        Ok(())
    }

    fn needs_chargen(&self, campaign: &Campaign) -> bool {
        campaign
            .actors
            .get("pc")
            .map(crate::bridge::needs_chargen)
            .unwrap_or(true)
    }

    fn operator_dict(&self) -> Option<String> {
        Some(self.game.lexicon_dump())
    }
}

fn current_actor_display_name(campaign: &Campaign, pc_fallback: &str) -> String {
    actor_label(campaign, &campaign.state.current_actor, pc_fallback)
}

fn actor_label(campaign: &Campaign, id: &str, pc_fallback: &str) -> String {
    if let Some(sheet) = campaign.actors.get(id) {
        let n = sheet.name.trim();
        if !n.is_empty() {
            // Main PC sheet still says "You" until renamed — prefer world name.
            if id == "pc" && crate::bridge::is_default_player_name(n) {
                return pc_fallback.to_string();
            }
            return n.to_string();
        }
    }
    if id == "pc" {
        pc_fallback.to_string()
    } else {
        id.to_string()
    }
}

struct TableAdvance {
    skipped: bool,
    lines: Vec<String>,
    patches: Vec<hexbee_store::StorePatch>,
    narration: Option<String>,
}

impl TavernHex {
    fn advance_table(&mut self, ctx: &mut HexContext<'_>, summary: &str) -> TableAdvance {
        let seats = crate::rounds::seat_ids(ctx.campaign);
        let actor = ctx.actor_id.clone();
        let step = crate::rounds::finish_spent_seat(
            &mut self.game,
            &actor,
            summary,
            &seats,
            ctx.llm,
        );
        if step.skipped {
            return TableAdvance {
                skipped: true,
                lines: Vec::new(),
                patches: Vec::new(),
                narration: None,
            };
        }
        let narration = if step.closed {
            Some(step.round_lines.join("\n"))
        } else {
            None
        };
        let mut lines = step.round_lines;
        let mut patches = Vec::new();
        if step.next_actor != ctx.campaign.state.current_actor {
            let label = actor_label(
                ctx.campaign,
                &step.next_actor,
                self.game.player_display_name(),
            );
            lines.push(format!("🎭 Now playing as {label}."));
            patches.push(hexbee_store::StorePatch::set_str(
                "state.current_actor",
                &step.next_actor,
            ));
            ctx.campaign.state.current_actor = step.next_actor;
            ctx.campaign.dirty = true;
        }
        TableAdvance {
            skipped: false,
            lines,
            patches,
            narration,
        }
    }
}

fn compose_narration(outcome: Option<String>, reaction: Option<String>) -> Option<String> {
    match (outcome, reaction) {
        (Some(outcome), Some(reaction))
            if !outcome.trim().is_empty() && !reaction.trim().is_empty() =>
        {
            Some(format!("{outcome}\n{reaction}"))
        }
        (Some(outcome), _) if !outcome.trim().is_empty() => Some(outcome),
        (_, Some(reaction)) if !reaction.trim().is_empty() => Some(reaction),
        _ => None,
    }
}

fn turn_summary(actor: &str, raw: &str, lines: &[String]) -> String {
    crate::rounds::plain_round_fact(actor, raw, lines)
}

fn futures_helper(
    game: &mut TavernGame,
    input: &str,
    llm: Option<&dyn hexbee_provider::LlmClient>,
) -> Result<Vec<GameEvent>, String> {
    tokio_task(game, input, llm)
}

fn tokio_task(
    game: &mut TavernGame,
    input: &str,
    llm: Option<&dyn hexbee_provider::LlmClient>,
) -> Result<Vec<GameEvent>, String> {
    // When called from async CLI context, we're already in tokio runtime.
    if tokio::runtime::Handle::try_current().is_ok() {
        return Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(game.handle_line(input, llm))
        }));
    }
    Err("no tokio runtime".into())
}

// trait Pipe: Sized {
//     fn pipe<F, R>(self, f: F) -> R
//     where
//         F: FnOnce(Self) -> R,
//     {
//         f(self)
//     }
// }
//
// impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narration_keeps_the_outcome_ahead_of_the_reaction() {
        let text = compose_narration(
            Some("🪶 You climb the narrow stairs to the rented rooms above.".into()),
            Some("— round —\n💬 Groll: \"Mm.\"".into()),
        )
        .unwrap();
        assert!(text.starts_with("🪶 You climb"));
        assert!(text.find("climb").unwrap() < text.find("Groll").unwrap());
    }

    #[test]
    fn bundled_campaign_loads() {
        let (hex, campaign) = TavernHex::open_bundled();
        assert_eq!(hex.id(), "tavern");
        assert!(!campaign.actors.is_empty());
    }

    #[test]
    fn resumed_session_detects_saved_progress() {
        let (_, mut campaign) = TavernHex::open_bundled();
        campaign.reset_progress();
        if let Some(pc) = campaign.actors.get_mut("pc") {
            pc.name = "Jules".into();
            pc.stats = serde_json::json!({ "chargen_done": true });
        }
        let hex = TavernHex::new(&campaign);
        assert!(!hex.is_resumed_session(&campaign));
        campaign.state.current_scene = "main_hall".into();
        let hex = TavernHex::new(&campaign);
        assert!(hex.is_resumed_session(&campaign));
    }
}
