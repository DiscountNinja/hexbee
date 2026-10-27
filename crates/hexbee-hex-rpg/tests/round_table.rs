//! Solo close, two-seat wait, and a free look.

use hexbee_hex_rpg::TavernHex;
use hexbee_kernel::{CapabilityGate, HexContext, HexModule, Intent, IntentKind};
use hexbee_store::Campaign;

fn intent(actor: &str, raw: &str) -> Intent {
    Intent {
        kind: IntentKind::Action,
        hex_target: None,
        actor_id: actor.into(),
        verb: None,
        object_ids: vec![],
        raw: raw.into(),
        confidence: 1.0,
        notes: None,
    }
}

fn play(hex: &mut TavernHex, campaign: &mut Campaign, actor: &str, raw: &str) -> Vec<String> {
    campaign.state.current_actor = actor.into();
    let gate = CapabilityGate::new(campaign.config.clone());
    let mut ctx = HexContext {
        campaign,
        gate: &gate,
        llm: None,
        actor_id: actor.into(),
    };
    hex.handle_intent(&mut ctx, &intent(actor, raw))
        .expect("turn")
        .lines
}

#[tokio::test(flavor = "multi_thread")]
async fn solo_spending_action_closes_the_round() {
    let mut campaign = Campaign::load(hexbee_hex_rpg::bundled_campaign_dir()).unwrap();
    let mut hex = TavernHex::new(&campaign);
    let seats = campaign
        .controllable_actors()
        .into_iter()
        .map(|a| a.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(seats, vec!["pc".to_string()]);

    let lines = play(&mut hex, &mut campaign, "pc", "sit down");
    assert!(
        lines.iter().any(|l| l == "— round —"),
        "solo turn should close the round, got {lines:?}"
    );
    assert!(hex.game.world.round_gone.is_empty());
    assert_eq!(hex.game.world.situation.clock_beats, 1);
    assert_eq!(campaign.state.current_actor, "pc");
}

#[tokio::test(flavor = "multi_thread")]
async fn two_seats_wait_until_both_have_gone() {
    let mut campaign = Campaign::load(hexbee_hex_rpg::bundled_campaign_dir()).unwrap();
    let mut mira = campaign.actors.get("pc").unwrap().clone();
    mira.id = "mira".into();
    mira.name = "Mira".into();
    mira.player_controllable = true;
    campaign.actors.insert("mira".into(), mira);
    let mut hex = TavernHex::new(&campaign);

    let first = play(&mut hex, &mut campaign, "pc", "sit down");
    assert!(
        first.iter().all(|l| l != "— round —"),
        "first seat must not close the round, got {first:?}"
    );
    assert_eq!(campaign.state.current_actor, "mira");
    assert_eq!(hex.game.world.situation.clock_beats, 0);
    assert!(hex.game.world.round_gone.iter().any(|id| id == "pc"));

    let second = play(&mut hex, &mut campaign, "mira", "sit down");
    assert!(
        second.iter().any(|l| l == "— round —"),
        "second seat should close the round, got {second:?}"
    );
    assert!(hex.game.world.round_gone.is_empty());
    assert_eq!(hex.game.world.situation.clock_beats, 1);
    assert_eq!(campaign.state.current_actor, "mira");
}

#[tokio::test(flavor = "multi_thread")]
async fn look_does_not_spend_the_turn() {
    let mut campaign = Campaign::load(hexbee_hex_rpg::bundled_campaign_dir()).unwrap();
    let mut hex = TavernHex::new(&campaign);
    let lines = play(&mut hex, &mut campaign, "pc", "look around");
    assert!(
        lines.iter().all(|l| l != "— round —"),
        "look should stay on the same turn, got {lines:?}"
    );
    assert!(hex.game.world.round_gone.is_empty());
    assert_eq!(hex.game.world.situation.clock_beats, 0);
    assert_eq!(campaign.state.current_actor, "pc");
}
