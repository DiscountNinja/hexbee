//! Integration tests for persistence and capability gate.

use hexbee_hex_rpg::{bundled_campaign_dir, sync_world_to_campaign, /* Location, */ TavernHex};
use hexbee_kernel::{classify_line, CapabilityGate, /* GateDecision, */ IntentKind};
use hexbee_store::{Campaign, HexConfig, StorePatch};

#[test]
fn kill_reload_preserves_state() {
    let dir = std::env::temp_dir().join(format!("hexbee-persist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Copy bundled campaign to temp dir.
    copy_dir_all(&bundled_campaign_dir(), &dir).unwrap();

    let mut campaign = Campaign::load(&dir).unwrap();
    let mut hex = TavernHex::new(&campaign);
    hex.game.world.player.gold = 42;
    hex.game.world.location = hexbee_hex_rpg::Location::MainHall;
    sync_world_to_campaign(&mut campaign, &hex.game.world);
    campaign.save().unwrap();

    let reloaded = Campaign::load(&dir).unwrap();
    let hex2 = TavernHex::new(&reloaded);
    assert_eq!(hex2.game.world.player.gold, 42);
    assert_eq!(
        hex2.game.world.location,
        hexbee_hex_rpg::Location::MainHall
    );
    assert_eq!(reloaded.state.current_scene, "main_hall");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hot_seat_actor_in_transcript() {
    let mut campaign = Campaign::load(bundled_campaign_dir()).unwrap();
    campaign.state.current_actor = "sally".into();
    campaign.append_transcript(hexbee_store::TranscriptBeat {
        actor_id: "sally".into(),
        intent_kind: "speech".into(),
        raw: "Hey there, handsome.".into(),
        narration: Some("Sally winks.".into()),
        organ_trace: vec!["actor".into()],
    });
    assert_eq!(campaign.transcript.beats.last().unwrap().actor_id, "sally");
}

#[test]
fn capability_gate_denies_stat_cheat() {
    let gate = CapabilityGate::new(HexConfig::default());
    let decision = gate.check_patches(&[StorePatch::set_i64("actors.pc.stats.might", 99)]);
    assert!(matches!(
        decision,
        hexbee_kernel::GateDecision::Deny(_)
    ));
}

#[test]
fn options_during_play_is_command() {
    let intent = classify_line("options", "pc", true);
    assert_eq!(intent.kind, IntentKind::Command);
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}
