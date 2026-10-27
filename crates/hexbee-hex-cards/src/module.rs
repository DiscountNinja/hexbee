//! HexModule adapter for the cards table.

use hexbee_kernel::{HexContext, HexModule, HexOutput, Intent};
use hexbee_store::Campaign;

use crate::game::{CardsGame, GameEvent};

pub struct CardsHex {
    pub game: CardsGame,
}

impl CardsHex {
    pub fn new(campaign: &Campaign) -> Self {
        let seed = campaign.state.rng_seed;
        let mut game = CardsGame::new(seed);
        game.load_scores(&campaign.state.runtime);
        Self { game }
    }

    pub fn open_bundled() -> (Self, Campaign) {
        let campaign = Campaign::load(bundled_campaign_dir()).expect("bundled cards campaign");
        let hex = Self::new(&campaign);
        (hex, campaign)
    }

    pub fn reset_from_campaign(&mut self, campaign: &Campaign) {
        let seed = campaign.state.rng_seed;
        let mut game = CardsGame::new(seed);
        game.load_scores(&campaign.state.runtime);
        self.game = game;
    }
}

pub fn bundled_campaign_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("campaign")
}

impl HexModule for CardsHex {
    fn id(&self) -> &str {
        "cards"
    }

    fn handle_intent(
        &mut self,
        ctx: &mut HexContext<'_>,
        intent: &Intent,
    ) -> Result<HexOutput, String> {
        let llm = ctx.llm;
        let events = run_line(&mut self.game, &intent.raw, llm)?;
        ctx.campaign.state.runtime = self.game.sync_runtime();
        ctx.campaign.dirty = true;

        let mut lines = Vec::new();
        for event in events {
            match event {
                GameEvent::Lines(ls) => lines.extend(ls),
            }
        }
        Ok(HexOutput {
            lines,
            organ_trace: vec!["cards".into()],
            ended: self.game.over,
            ..Default::default()
        })
    }

    fn scene_header(&self, _campaign: &Campaign) -> Vec<String> {
        vec![self.game.intro(), String::new(), self.game.table.scores_line()]
    }

    fn sticky_header(&self, campaign: &Campaign, include_intro: bool) -> Vec<String> {
        if include_intro {
            self.scene_header(campaign)
        } else {
            vec![
                format!(
                    "🃏 {}",
                    self.game
                        .table
                        .game
                        .map(|g| g.display())
                        .unwrap_or("Lobby")
                ),
                self.game.table.scores_line(),
            ]
        }
    }

    fn status_footer(&self, _campaign: &Campaign) -> String {
        self.game.status_line()
    }

    fn is_over(&self) -> bool {
        self.game.over
    }

    fn reset(&mut self, campaign: &Campaign) {
        self.reset_from_campaign(campaign);
    }

    fn needs_chargen(&self, _campaign: &Campaign) -> bool {
        false
    }
}

fn run_line(
    game: &mut CardsGame,
    input: &str,
    llm: Option<&dyn hexbee_provider::LlmClient>,
) -> Result<Vec<GameEvent>, String> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Ok(tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(game.handle_line(input, llm))
        }));
    }
    Err("no tokio runtime".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_loads() {
        let (hex, campaign) = CardsHex::open_bundled();
        assert_eq!(hex.id(), "cards");
        assert_eq!(campaign.config.id, "cards");
    }
}
