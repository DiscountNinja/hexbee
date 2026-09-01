//! Hexbee CLI — kernel-driven REPL with tavern hex module.

mod ui;

use hexbee_core::{init_logging, metrics_snapshot, Config};
use hexbee_hex_rpg::TavernHex;
use hexbee_kernel::{shell_route, Kernel};
use hexbee_provider::{LlmClient, OllamaClient};
use tracing::{info, warn};

use ui::{draw_board, print_scroll_lines, scroll_tail, term_height, with_spinner, LineEditor};

#[tokio::main]
async fn main() {
    let config = Config::default();
    let log_file = match init_logging() {
        Ok(path) => {
            info!(path = %path.display(), "logging initialized");
            path.display().to_string()
        }
        Err(e) => {
            eprintln!("warning: logging init failed: {e}");
            config.log_file.clone()
        }
    };

    let llm = OllamaClient::new(&config.ollama_base_url, &config.ollama_model);
    let mut kernel = Kernel::new(config.clone());
    let mut editor = LineEditor::new().expect("readline editor");

    let mut log = vec![
        "🐝 Hexbee v0.2 — kernel + store".into(),
        format!(
            "🤖 Model: {} @ {}",
            config.ollama_model, config.ollama_base_url
        ),
        format!("📜 Logs: {log_file}  (tail -f {log_file})"),
        "📊 Type 'metrics' anytime. Kernel cmds: options, save, reset, actor, goal".into(),
        String::new(),
        "Say 🎮 Play Tavern to begin. Auto-saves. Type `reset` for a fresh start.".into(),
    ];

    info!("hexbee cli ready");

    loop {
        let prompt_label = if kernel.has_active_hex() {
            format!("hexbee [{}]>", kernel.hex_id().unwrap_or("hex"))
        } else {
            "hexbee>".into()
        };

        if kernel.has_active_hex() {
            run_active_hex_turn(&mut kernel, &llm, &mut editor).await;
            if !kernel.has_active_hex() {
                log.push("🎲 Returned from hex session.".into());
                log.push(metrics_snapshot().render_text());
            }
            continue;
        }

        draw_board(
            "Hexbee",
            &log.join("\n"),
            "💡 Play Tavern · help · reset · load <path> · metrics · quit",
            "",
        );
        let Some(line) = (match editor.prompt(&prompt_label) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("input error: {e}");
                kernel.save_if_dirty();
                eprintln!("👋 Bye.");
                std::process::exit(1);
            }
        }) else {
            // Ctrl-C / Ctrl-D / EOF — save and exit the process.
            kernel.save_if_dirty();
            eprintln!("👋 Bye.");
            std::process::exit(0);
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if is_reset_command(line) {
            match reset_bundled_campaign() {
                Ok(()) => log.push(
                    "🔄 Bundled campaign reset. Say Play Tavern to start fresh.".into(),
                ),
                Err(e) => log.push(format!("Reset failed: {e}")),
            }
            continue;
        }

        if try_start_hex(&mut kernel, line, &mut editor) {
            run_active_hex_turn(&mut kernel, &llm, &mut editor).await;
            if !kernel.has_active_hex() {
                log.push("🎲 Returned from hex session.".into());
                log.push(metrics_snapshot().render_text());
            }
            continue;
        }

        match kernel.run_turn(line, Some(&llm as &dyn LlmClient)) {
            Ok(out) => {
                if out.quit {
                    log.push("👋 Bye.".into());
                    log.push(metrics_snapshot().render_text());
                    break;
                }
                if out.reset {
                    match reset_bundled_campaign() {
                        Ok(()) => log.push(
                            "🔄 Bundled campaign reset. Say Play Tavern to start fresh.".into(),
                        ),
                        Err(e) => log.push(format!("Reset failed: {e}")),
                    }
                    continue;
                }
                log.extend(out.lines);
            }
            Err(e) => log.push(format!("Error: {e}")),
        }

        const MAX_LOG: usize = 24;
        if log.len() > MAX_LOG {
            let skip = log.len() - MAX_LOG;
            log = log.split_off(skip);
            log.insert(0, "…".into());
        }
    }
}

fn try_start_hex(kernel: &mut Kernel, line: &str, editor: &mut LineEditor) -> bool {
    let lower = line.to_lowercase();
    if let Some(route) = shell_route(line, &lower) {
        use hexbee_kernel::ShellRoute;
        match route {
            ShellRoute::PlayHex(_) => {
                start_tavern(kernel, editor);
                return true;
            }
            ShellRoute::LoadCampaign(path) => {
                start_tavern_at(kernel, &path, editor);
                return kernel.has_active_hex();
            }
            _ => {}
        }
    }
    false
}

fn is_reset_command(line: &str) -> bool {
    matches!(
        line.trim().to_lowercase().as_str(),
        "reset" | "restart" | "new game"
    )
}

fn reset_bundled_campaign() -> Result<(), String> {
    let mut campaign = hexbee_store::Campaign::load(hexbee_hex_rpg::bundled_campaign_dir())
        .map_err(|e| e.to_string())?;
    campaign.reset_progress();
    campaign.save().map_err(|e| e.to_string())?;
    info!("bundled campaign progress wiped");
    Ok(())
}

fn start_tavern(kernel: &mut Kernel, editor: &mut LineEditor) {
    let (hex, campaign) = TavernHex::open_bundled();
    if let Err(e) = kernel.start_hex(Box::new(hex), campaign) {
        warn!(%e, "failed to start tavern");
    } else {
        info!("tavern hex started from bundled campaign");
        run_character_creator(kernel, editor, false);
    }
}

fn start_tavern_at(kernel: &mut Kernel, path: &str, editor: &mut LineEditor) {
    let hex = TavernHex::new(&hexbee_store::Campaign::load(path).unwrap_or_else(|e| {
        warn!(%e, path, "campaign load failed, using bundled");
        hexbee_store::Campaign::load(hexbee_hex_rpg::bundled_campaign_dir()).expect("bundled")
    }));
    if let Ok(campaign) = hexbee_store::Campaign::load(path) {
        if kernel.start_hex(Box::new(hex), campaign).is_ok() {
            run_character_creator(kernel, editor, false);
        }
    }
}

fn prompt_line(kernel: &mut Kernel, editor: &mut LineEditor, label: &str) -> String {
    match editor.prompt(label) {
        Ok(Some(v)) => v,
        Ok(None) => {
            kernel.save_if_dirty();
            eprintln!("👋 Bye.");
            std::process::exit(0);
        }
        Err(_) => {
            kernel.save_if_dirty();
            eprintln!("👋 Bye.");
            std::process::exit(1);
        }
    }
}

/// Name → build preset/custom → skills → write sheet. Replaces name-only prompt.
fn run_character_creator(kernel: &mut Kernel, editor: &mut LineEditor, force: bool) {
    if !force && !kernel.needs_chargen() {
        return;
    }

    // 1. Name
    let name = loop {
        draw_board(
            "Tavern RPG",
            "Who walks into this muddy street?\nChoose a character name.",
            "letters or numbers · Enter when ready",
            "",
        );
        let reply = prompt_line(kernel, editor, "name>");
        match kernel.apply_player_name(&reply) {
            Ok(()) => break reply.trim().to_string(),
            Err(e) => warn!(%e, "invalid player name"),
        }
    };

    // 2. Preset or Custom
    let presets = hexbee_hex_rpg::BuildPreset::ALL;
    let preset_lines: String = presets
        .iter()
        .enumerate()
        .map(|(i, p)| format!("  {} — {} ({})", i + 1, p.label(), p.blurb()))
        .collect::<Vec<_>>()
        .join("\n");
    draw_board(
        "Character Creator",
        &format!(
            "Welcome, {name}.\nPick a build preset, or type custom:\n{preset_lines}"
        ),
        "1–4 · face/scout/scholar/bruiser · custom",
        &name,
    );
    let mut build = loop {
        let reply = prompt_line(kernel, editor, "build>").trim().to_lowercase();
        if reply == "custom" || reply == "c" {
            break custom_array_build(kernel, editor);
        }
        if let Some(p) = hexbee_hex_rpg::BuildPreset::parse(&reply) {
            break hexbee_hex_rpg::CharacterBuild::from_preset(p);
        }
        warn!("unknown build; try 1–4 or custom");
    };

    // 3. Confirm / edit skills
    build = edit_skills(kernel, editor, build);

    draw_board(
        "Character Creator",
        &format!("{name}\n\n{}", build.sheet_summary()),
        "Enter to confirm · or type redo to restart skills",
        &name,
    );
    let confirm = prompt_line(kernel, editor, "ok>");
    if confirm.trim().eq_ignore_ascii_case("redo") {
        build = edit_skills(kernel, editor, build);
    }

    let json = build.to_stats_json();
    if let Err(e) = kernel.apply_character_build(json) {
        warn!(%e, "failed to apply character build");
    } else {
        info!(%name, preset = ?build.preset, "character created");
    }
}

fn custom_array_build(kernel: &mut Kernel, editor: &mut LineEditor) -> hexbee_hex_rpg::CharacterBuild {
    use hexbee_hex_rpg::{AbilityScores, STANDARD_ARRAY};
    draw_board(
        "Custom Build",
        &format!(
            "Assign the standard array {:?} to STR DEX CON INT WIS CHA.\n\
Enter six numbers separated by spaces (must be those values).",
            STANDARD_ARRAY
        ),
        "e.g. 15 14 13 12 10 8",
        "",
    );
    let scores = loop {
        let reply = prompt_line(kernel, editor, "array>");
        let nums: Vec<i32> = reply
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.len() != 6 {
            warn!("need six numbers");
            continue;
        }
        let mut sorted = nums.clone();
        sorted.sort_unstable();
        let mut expected = STANDARD_ARRAY.to_vec();
        expected.sort_unstable();
        if sorted != expected {
            warn!("must use each of {:?} exactly once", STANDARD_ARRAY);
            continue;
        }
        break [
            nums[0], nums[1], nums[2], nums[3], nums[4], nums[5],
        ];
    };
    let mut build = hexbee_hex_rpg::CharacterBuild::default();
    build.abilities = AbilityScores::from_ordered(scores);
    build.preset = Some("custom".into());
    build.skills.clear();
    build
}

fn edit_skills(
    kernel: &mut Kernel,
    editor: &mut LineEditor,
    mut build: hexbee_hex_rpg::CharacterBuild,
) -> hexbee_hex_rpg::CharacterBuild {
    use hexbee_hex_rpg::{Skill, SKILL_PICKS};
    let catalog: String = Skill::ALL
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{:2}. {}", i + 1, s.label()))
        .collect::<Vec<_>>()
        .join("\n");
    let current: String = build
        .skills
        .iter()
        .map(|s| s.label())
        .collect::<Vec<_>>()
        .join(", ");
    draw_board(
        "Skill Proficiencies",
        &format!(
            "Pick {SKILL_PICKS} skills (numbers or names, comma/space separated).\n\
Current: {current}\n\n{catalog}\n\nEnter to keep current."
        ),
        "e.g. 15 16 11 12  or  persuasion intimidation insight stealth",
        "",
    );
    let reply = prompt_line(kernel, editor, "skills>");
    let trimmed = reply.trim();
    if trimmed.is_empty() {
        if build.skills.len() == SKILL_PICKS {
            return build;
        }
        // Ensure we have exactly SKILL_PICKS from Face default if empty.
        if build.skills.is_empty() {
            build.skills = hexbee_hex_rpg::BuildPreset::Face.skills().to_vec();
        }
        return build;
    }
    let mut picked = Vec::new();
    for token in trimmed.split(|c: char| c == ',' || c.is_whitespace()) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if let Ok(n) = token.parse::<usize>() {
            if (1..=Skill::ALL.len()).contains(&n) {
                let sk = Skill::ALL[n - 1];
                if !picked.contains(&sk) {
                    picked.push(sk);
                }
                continue;
            }
        }
        if let Some(sk) = Skill::parse(token) {
            if !picked.contains(&sk) {
                picked.push(sk);
            }
        }
    }
    if picked.len() != SKILL_PICKS {
        warn!(
            "need exactly {SKILL_PICKS} skills (got {}); keeping previous",
            picked.len()
        );
        return build;
    }
    build.skills = picked;
    build
}

async fn run_active_hex_turn(kernel: &mut Kernel, llm: &OllamaClient, editor: &mut LineEditor) {
    let resuming = kernel.is_resumed_session();
    with_spinner("setting the scene", async {
        kernel.prepare_scene(Some(llm as &dyn LlmClient));
    })
    .await;
    let mut last_paint = kernel.scene_paint_fingerprint();
    let mut sticky = kernel.sticky_header(!resuming);
    let mut feed: Vec<String> = Vec::new();
    if resuming {
        let scene_lines = with_spinner("setting the scene", async {
            kernel.scene_arrival_lines(Some(llm as &dyn LlmClient), None)
        })
        .await;
        push_scene_lines(&mut feed, &scene_lines);
        last_paint = kernel.scene_paint_fingerprint();
    }
    refresh_hex_page(&sticky, &feed, kernel);
    let mut feed_printed = feed.len();

    loop {
        if kernel.hex_is_over() {
            feed.push("🏁 Scene complete.".into());
            feed.push("Type `reset` for a new game, or press Enter to leave.".into());
            feed.push(metrics_snapshot().render_text());
            print_scroll_lines(&feed[feed_printed..], &kernel.player_display_name());
            refresh_hex_page(&sticky, &feed, kernel);
            let reply = match editor.prompt("reset / Enter>") {
                Ok(v) => v.unwrap_or_default(),
                Err(_) => String::new(),
            };
            if is_reset_command(reply.trim()) {
                if let Ok(out) = kernel.run_turn("reset", None) {
                    if out.reset {
                        run_character_creator(kernel, editor, true);
                        last_paint = kernel.scene_paint_fingerprint();
                        with_spinner("setting the scene", async {
                            kernel.prepare_scene(Some(llm as &dyn LlmClient));
                        })
                        .await;
                        sticky = kernel.sticky_header(false);
                        feed.clear();
                        feed.push("🔄 Fresh start.".into());
                        refresh_hex_page(&sticky, &feed, kernel);
                        feed_printed = feed.len();
                        continue;
                    }
                }
            }
            kernel.end_session();
            break;
        }

        let prompt = format!("hexbee [{}]>", kernel.hex_id().unwrap_or("hex"));
        let Some(line) = (match editor.prompt(&prompt) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("input error: {e}");
                kernel.save_if_dirty();
                eprintln!("👋 Bye.");
                std::process::exit(1);
            }
        }) else {
            // Ctrl-C / Ctrl-D / EOF — save and exit the process (not empty-line continue).
            kernel.save_if_dirty();
            eprintln!("👋 Bye.");
            std::process::exit(0);
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if matches!(line.to_lowercase().as_str(), "quit" | "exit") {
            kernel.save_if_dirty();
            kernel.end_session();
            break;
        }

        let feed_before = feed.len();

        let turn = with_spinner(
            "waiting on the model",
            async { kernel.run_turn(line, Some(llm as &dyn LlmClient)) },
        )
        .await;

        match turn {
            Ok(out) => {
                if out.quit {
                    kernel.end_session();
                    break;
                }
                push_turn_lines(&mut feed, &out.lines);
                if out.reset {
                    run_character_creator(kernel, editor, true);
                    last_paint = kernel.scene_paint_fingerprint();
                    with_spinner("setting the scene", async {
                        kernel.prepare_scene(Some(llm as &dyn LlmClient));
                    })
                    .await;
                    sticky = kernel.sticky_header(true);
                    feed.clear();
                    feed.push("🔄 Fresh start.".into());
                    refresh_hex_page(&sticky, &feed, kernel);
                    feed_printed = feed.len();
                    continue;
                }
                if out.ended {
                    feed.push("🏁 Scene complete. Type `reset` at the shell for a new game.".into());
                    print_scroll_lines(&feed[feed_printed..], &kernel.player_display_name());
                    refresh_hex_page(&sticky, &feed, kernel);
                    let _ = editor.prompt("press Enter");
                    kernel.end_session();
                    break;
                }
            }
            Err(e) => feed.push(format!("⚠️ {e}")),
        }

        if feed.len() == feed_before {
            feed.push(
                "… (no readable beat — try again, or look / talk more directly)".into(),
            );
        }

        let paint = kernel.scene_paint_fingerprint();
        let scene_moved = paint != last_paint;
        if scene_moved {
            last_paint = paint;
            let scene_lines = with_spinner("setting the scene", async {
                kernel.scene_change_lines(Some(llm as &dyn LlmClient), Some(line))
            })
            .await;
            push_scene_lines(&mut feed, &scene_lines);
            sticky = kernel.sticky_header(false);
            refresh_hex_page(&sticky, &feed, kernel);
            feed_printed = feed.len();
        } else if feed.len() > feed_printed {
            print_scroll_lines(&feed[feed_printed..], &kernel.player_display_name());
            feed_printed = feed.len();
        }

        const MAX_FEED: usize = 500;
        if feed.len() > MAX_FEED {
            let skip = feed.len() - MAX_FEED;
            feed = feed.split_off(skip);
            feed.insert(0, "…".into());
            feed_printed = feed_printed.saturating_sub(skip);
            if feed_printed > feed.len() {
                feed_printed = feed.len();
            }
        }
    }
}

/// Append scene paint lines (`— scene —` header + transition/arrival).
fn push_scene_lines(feed: &mut Vec<String>, lines: &[String]) {
    let mut any = false;
    for l in lines {
        let l = l.trim();
        if !l.is_empty() && !is_ui_noise(l) {
            if !any {
                feed.push("— scene —".into());
                any = true;
            }
            feed.push(l.to_string());
        }
    }
}

/// Append one turn's lines in order; drop duplicate success banners after a check line.
fn push_turn_lines(feed: &mut Vec<String>, lines: &[String]) {
    let mut saw_check_outcome = false;
    let mut narrating = false;
    for l in lines {
        for part in l.split('\n') {
            let part = part.trim();
            if part.is_empty() || is_ui_noise(part) {
                continue;
            }
            if part.contains('→') && (part.contains('✅') || part.contains('❌')) {
                saw_check_outcome = true;
                narrating = false;
                feed.push(part.to_string());
                continue;
            }
            if saw_check_outcome
                && (part.starts_with("✅ Success") || part.starts_with("❌ Failure"))
            {
                continue;
            }
            saw_check_outcome = false;

            if part.starts_with('🪶') {
                narrating = true;
                feed.push(part.to_string());
            } else if narrating
                && !part.starts_with('💬')
                && !part.starts_with("🎬")
                && !part.starts_with('🎲')
            {
                feed.push(format!("🪶 {part}"));
            } else {
                narrating = part.starts_with('🪶');
                feed.push(part.to_string());
            }
        }
    }
}

/// Full-screen framed page (scene change, session start, game over).
fn refresh_hex_page(sticky: &[String], feed: &[String], kernel: &Kernel) {
    let footer = kernel.status_footer();
    let player = kernel.player_display_name();
    let sticky_lines = sticky.len() + 2;
    let footer_lines = footer.lines().count() + 4;
    let scroll_rows = term_height().saturating_sub(sticky_lines + footer_lines + 2);
    let visible = scroll_tail(feed, scroll_rows);
    let mut body = sticky.join("\n");
    if !visible.is_empty() {
        body.push_str("\n\n");
        body.push_str(&visible.join("\n"));
    }
    draw_board("Tavern RPG", &body, &footer, &player);
}

fn is_ui_noise(part: &str) -> bool {
    let lower = part.to_lowercase();
    lower.starts_with("loc:")
        || lower.starts_with("memory:")
        || lower.starts_with("❤️ hp")
        || lower.starts_with("hp ")
        || (lower.contains("❤️ hp") && lower.contains("gold"))
        || lower.contains("return only json")
        || lower.starts_with("framing:")
        || lower.starts_with("beat:")
}
