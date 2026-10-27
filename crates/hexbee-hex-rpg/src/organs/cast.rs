//! Ambient cast generation + action/speech formatting helpers.

use serde::Deserialize;
use tracing::info;

use hexbee_provider::{complete_json_with_retry, DEFAULT_JSON_ATTEMPTS};
use hexbee_provider::LlmClient;

use crate::organs::dialogue::{looks_like_fact_dump, looks_like_llm_refusal, looks_like_wrong_speaker_relay};
use crate::state::NpcId;

/// One ambient extra the model may invent for a room.
#[derive(Debug, Clone, Deserialize)]
pub struct AmbientCastEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub temperament: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub voice: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CastJson {
    #[serde(default)]
    cast: Vec<AmbientCastEntry>,
}

/// Ask the model for 0–2 ambient extras. Never invents quest cast.
pub async fn generate_ambient_cast(
    llm: &dyn LlmClient,
    location: &str,
    already_present: &str,
) -> Vec<AmbientCastEntry> {
    let system = "Invent 0-2 minor tavern extras for atmosphere only.\n\
Return ONLY JSON: {\"cast\":[{\"id\":\"slug\",\"name\":\"...\",\"temperament\":\"...\",\"description\":\"one line look\",\"voice\":\"...\"}]}\n\
Never invent Dewr, Sally, Groll, Dremis, or quest solutions. Keep names ordinary.";
    let user = format!(
        "Place: {location}\nAlready here: {}\nFill the room with at most two background people (or animals).",
        if already_present.is_empty() {
            "(quest cast only / empty)"
        } else {
            already_present
        }
    );
    let entries = complete_json_with_retry(llm, system, &user, DEFAULT_JSON_ATTEMPTS, |text| {
        let parsed = parse_cast_json(text)?;
        if parsed.is_empty() {
            None
        } else {
            Some(parsed)
        }
    })
    .await
    .unwrap_or_default();
    if entries.is_empty() {
        info!("ambient cast empty after retries");
        Vec::new()
    } else {
        info!(count = entries.len(), "ambient cast from llm");
        entries.into_iter().take(2).collect()
    }
}

#[derive(Debug, Deserialize)]
struct SoftProposalJson {
    #[serde(default)]
    add: bool,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    voice: String,
    #[serde(default)]
    dc: Option<i32>,
    #[serde(default)]
    price: Option<i32>,
    #[serde(default)]
    attitude: Option<i32>,
    #[serde(default)]
    heat: Option<i32>,
}

/// Ask for one minor object, side character, or animal. `None` means leave the world alone.
pub async fn propose_soft_detail(
    llm: &dyn LlmClient,
    location: &str,
    player_line: &str,
    open_goal: &str,
    present: &str,
) -> Option<crate::world_doc::SoftProposal> {
    let system = "You may add one minor soft detail already implied by the player's line, or refuse.\n\
Return ONLY JSON: {\"add\":false} or {\"add\":true,\"kind\":\"object|person|animal\",\"name\":\"short\",\"text\":\"one sentence\",\"voice\":\"\",\"dc\":null,\"price\":null,\"attitude\":null,\"heat\":null}\n\
Only a minor object, side character, or animal in this room.\n\
Never add Dewr, Sally, Groll, Dremis, a password, an amulet, a door tax, or gold.\n\
Do not complete the open goal. If you add something, let it draw the eye that way without stating the goal or its reward.\n\
dc, if set, is 8-16. price is a local coin price, not a gift of gold.";
    let user = format!(
        "Place: {location}\nOpen goal (nudge only): {open_goal}\nAlready here: {present}\nPlayer: {player_line}"
    );
    let parsed = complete_json_with_retry(llm, system, &user, DEFAULT_JSON_ATTEMPTS, |text| {
        parse_soft_proposal(text)
    })
    .await?;
    if !parsed.add {
        return None;
    }
    let kind = match parsed.kind.trim() {
        "object" => crate::world_doc::SoftKind::Object,
        "person" => crate::world_doc::SoftKind::Person,
        "animal" => crate::world_doc::SoftKind::Animal,
        _ => return None,
    };
    Some(crate::world_doc::SoftProposal {
        kind,
        name: parsed.name,
        text: parsed.text,
        voice: parsed.voice,
        stake: crate::world_doc::SoftStake {
            attitude: parsed.attitude,
            heat: parsed.heat,
            price: parsed.price,
            dc: parsed.dc,
        },
    })
}

fn parse_soft_proposal(text: &str) -> Option<SoftProposalJson> {
    let trimmed = text.trim();
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    serde_json::from_str(&trimmed[start..=end]).ok()
}

fn parse_cast_json(text: &str) -> Option<Vec<AmbientCastEntry>> {
    let trimmed = text.trim();
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    let raw: CastJson = serde_json::from_str(&trimmed[start..=end]).ok()?;
    let mut out = Vec::new();
    for e in raw.cast {
        let id = e.id.trim();
        let name = e.name.trim();
        if id.is_empty() && name.is_empty() {
            continue;
        }
        out.push(AmbientCastEntry {
            id: if id.is_empty() {
                name.to_lowercase()
            } else {
                id.to_string()
            },
            name: if name.is_empty() {
                id.to_string()
            } else {
                name.to_string()
            },
            temperament: if e.temperament.trim().is_empty() {
                "ordinary".into()
            } else {
                e.temperament
            },
            description: if e.description.trim().is_empty() {
                "A local face in the room.".into()
            } else {
                e.description
            },
            voice: if e.voice.trim().is_empty() {
                "plain".into()
            } else {
                e.voice
            },
        });
    }
    Some(out)
}

/// Split player text into physical action (`*` / `<>`) and spoken chat.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerLineParts {
    pub action: String,
    pub speech: String,
}

pub fn split_player_line(input: &str) -> PlayerLineParts {
    let mut action_bits = Vec::new();
    let mut rest = input.to_string();

    // Extract *action*
    while let Some(start) = rest.find('*') {
        if let Some(rel_end) = rest[start + 1..].find('*') {
            let end = start + 1 + rel_end;
            let bit = rest[start + 1..end].trim();
            if !bit.is_empty() {
                action_bits.push(bit.to_string());
            }
            rest = format!("{}{}", &rest[..start], &rest[end + 1..]);
        } else {
            break;
        }
    }
    // Extract <action>
    while let Some(start) = rest.find('<') {
        if let Some(rel_end) = rest[start + 1..].find('>') {
            let end = start + 1 + rel_end;
            let bit = rest[start + 1..end].trim();
            if !bit.is_empty() {
                action_bits.push(bit.to_string());
            }
            rest = format!("{}{}", &rest[..start], &rest[end + 1..]);
        } else {
            break;
        }
    }

    // Prefer quoted speech if present.
    let mut speech = String::new();
    let mut work = rest.clone();
    while let Some(start) = work.find('"') {
        if let Some(rel_end) = work[start + 1..].find('"') {
            let end = start + 1 + rel_end;
            let bit = work[start + 1..end].trim();
            if !bit.is_empty() {
                if !speech.is_empty() {
                    speech.push(' ');
                }
                speech.push_str(bit);
            }
            work = format!("{}{}", &work[..start], &work[end + 1..]);
        } else {
            break;
        }
    }
    let leftover = work.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some((acted, said)) = split_acted_speech(&leftover) {
        if !acted.is_empty() {
            action_bits.push(acted);
        }
        if !said.is_empty() {
            if !speech.is_empty() {
                speech.push(' ');
            }
            speech.push_str(&said);
        }
    } else if speech.is_empty() {
        speech = leftover;
    } else if !leftover.is_empty() && action_bits.is_empty() {
        action_bits.push(leftover);
    } else if !leftover.is_empty() {
        speech = format!("{speech} {leftover}");
    }

    PlayerLineParts {
        action: action_bits.join("; "),
        speech: speech.trim().to_string(),
    }
}

/// Parsed NPC / narration beat with optional stage direction + speech.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StagedLine {
    pub action: String,
    pub speech: String,
    /// Raw when we could not split cleanly.
    pub raw: String,
}

pub fn parse_staged_line(line: &str) -> StagedLine {
    let trimmed = line.trim();
    let parts = split_player_line(trimmed);
    // If the model used *action* "speech", split_player_line works.
    if !parts.action.is_empty() || trimmed.contains('"') {
        let speech = if parts.speech.is_empty() {
            // Strip *actions* and leftover asterisks for speech fallback.
            trimmed
                .replace('*', " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            parts.speech
        };
        // Prefer speech without wrapping quotes for display.
        let speech = speech.trim_matches('"').trim().to_string();
        return StagedLine {
            action: parts.action,
            speech,
            raw: trimmed.to_string(),
        };
    }
    StagedLine {
        action: String::new(),
        speech: trimmed.trim_matches('"').to_string(),
        raw: trimmed.to_string(),
    }
}

/// A first-person physical attempt, optionally with words spoken after it.
fn split_acted_speech(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    const SPOKEN: &[&str] = &[
        " and say ",
        " and says ",
        " and ask ",
        " and asks ",
        " and tell ",
        " and shout ",
        " saying ",
        ", saying ",
    ];
    let lower = trimmed.to_lowercase();
    for mark in SPOKEN {
        if let Some(at) = lower.find(mark) {
            let acted = trimmed[..at].trim().trim_end_matches(['.', ',', ';']).trim();
            let said = trimmed[at + mark.len()..].trim().trim_matches('"').trim();
            if is_first_person_action(acted) {
                return Some((acted.to_string(), said.to_string()));
            }
        }
    }
    if is_first_person_action(trimmed) {
        return Some((trimmed.trim_end_matches(['.', '!']).trim().to_string(), String::new()));
    }
    None
}

fn is_first_person_action(text: &str) -> bool {
    let lower = text.trim().trim_end_matches(['.', '!', '?']).trim().to_lowercase();
    let body = lower
        .strip_prefix("i'll ")
        .or_else(|| lower.strip_prefix("i "))
        .or_else(|| lower.strip_prefix("we "))
        .or_else(|| lower.strip_prefix("let me "));
    let Some(body) = body else {
        return false;
    };
    const VERBS: &[&str] = &[
        "walk", "pet", "pat", "stroke", "scratch", "nuzzle", "sit", "stand", "look", "go",
        "approach", "open", "grab", "touch", "shove", "push", "pull", "climb", "sneak",
        "search", "examine", "pick", "take", "give", "hand", "drink", "eat", "knock", "lean",
        "step", "move", "reach", "draw", "swing", "hit", "kick", "lift", "break", "crouch",
        "kneel", "wave", "peer", "watch",
    ];
    VERBS.iter().any(|verb| body.starts_with(verb) || body.contains(&format!(" {verb}")))
}

/// Speech directed at an NPC with vocative stripped (`Hey Sally, …` → `…`).
pub fn speech_to_npc(player_line: &str, npc_name: &str) -> String {
    let parts = split_player_line(player_line);
    let mut speech = if parts.speech.is_empty() {
        player_line.trim().to_string()
    } else {
        parts.speech
    };
    let name = npc_name.trim().to_lowercase();
    if name.is_empty() {
        return speech;
    }
    let prefixes = [
        format!("hey {name}"),
        format!("hi {name}"),
        format!("hello {name}"),
        format!("say to {name}"),
        format!("talk to {name}"),
        name,
    ];
    for prefix in prefixes {
        let lower = speech.to_lowercase();
        if let Some(rest_lower) = lower.strip_prefix(&prefix) {
            let stripped_len = speech.len() - rest_lower.len();
            speech = speech[stripped_len..]
                .trim_start_matches(|c: char| ",?!.:; \t".contains(c))
                .to_string();
        }
    }
    speech.trim().to_string()
}

fn normalize_words(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// True when the NPC line is mostly the traveler's own words (tiny-model echo).
pub fn npc_line_echoes_traveler(npc_line: &str, traveler_speech: &str, npc_name: &str) -> bool {
    let traveler = speech_to_npc(traveler_speech, npc_name);
    let t = normalize_words(&traveler);
    if t.len() < 6 {
        return false;
    }
    let staged = parse_staged_line(npc_line);
    let npc_text = if !staged.speech.is_empty() {
        staged.speech.as_str()
    } else {
        staged.raw.as_str()
    };
    let n = normalize_words(npc_text);
    if n.is_empty() {
        return false;
    }
    if n == t {
        return true;
    }
    if n.contains(&t) && t.len() * 10 >= n.len() * 8 {
        return true;
    }
    if t.contains(&n) && n.len() * 10 >= t.len() * 8 {
        return true;
    }
    let tw: std::collections::HashSet<&str> = t.split_whitespace().collect();
    let nw: std::collections::HashSet<&str> = n.split_whitespace().collect();
    if tw.is_empty() || nw.is_empty() {
        return false;
    }
    let inter = tw.intersection(&nw).count();
    let union = tw.union(&nw).count();
    inter * 100 / union.max(1) >= 75 && inter >= 3
}

/// Strip speaker labels and reject traveler echoes; use template fallback when needed.
pub fn finalize_npc_line(
    line: &str,
    npc_name: &str,
    traveler_speech: &str,
    fallback: &str,
) -> String {
    finalize_npc_line_with_prior(line, npc_name, traveler_speech, fallback, &[])
}

/// Like [`finalize_npc_line`], but also rejects lines that quote earlier player turns from `prior`.
pub fn finalize_npc_line_with_prior(
    line: &str,
    npc_name: &str,
    traveler_speech: &str,
    fallback: &str,
    prior: &[&str],
) -> String {
    let cleaned = normalize_npc_line(line, npc_name);
    if cleaned.trim().is_empty() {
        return fallback.to_string();
    }
    if looks_like_llm_refusal(&cleaned.to_lowercase())
        || line_has_wrong_speaker_relay(&cleaned, npc_name)
    {
        return fallback.to_string();
    }
    if npc_line_echoes_traveler(&cleaned, traveler_speech, npc_name) {
        return fallback.to_string();
    }
    if line_echoes_prior_context(&cleaned, prior) {
        return fallback.to_string();
    }
    if looks_like_fact_dump(&cleaned.to_lowercase()) {
        return fallback.to_string();
    }
    cleaned
}

fn line_has_wrong_speaker_relay(line: &str, npc_name: &str) -> bool {
    if looks_like_wrong_speaker_relay(line, npc_name) {
        return true;
    }
    quoted_spans(line)
        .iter()
        .any(|s| looks_like_wrong_speaker_relay(s, npc_name))
}

fn quoted_spans(line: &str) -> Vec<String> {
    let mut spans = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '"' || c == '\'' || c == '“' || c == '‘' {
            let open = chars.next().unwrap();
            let close = match open {
                '"' => '"',
                '\'' => '\'',
                '“' => '”',
                '‘' => '’',
                _ => '"',
            };
            let mut inner = String::new();
            for ch in chars.by_ref() {
                if ch == close {
                    break;
                }
                inner.push(ch);
            }
            if inner.len() >= 6 {
                spans.push(inner);
            }
        } else {
            chars.next();
        }
    }
    spans
}

fn prior_player_speeches(ctx: &str) -> Vec<String> {
    let mut out = quoted_spans(ctx);
    if out.is_empty() {
        if let Some((_label, speech)) = ctx.split_once(':') {
            let t = speech.trim().trim_matches('"').trim_matches('\'');
            if t.len() >= 8 {
                out.push(t.to_string());
            }
        }
    }
    out
}

fn line_echoes_prior_context(line: &str, prior: &[&str]) -> bool {
    let nl = normalize_words(line);
    if nl.len() >= 12 {
        for ctx in prior {
            if ctx.is_empty() {
                continue;
            }
            let cl = normalize_words(ctx);
            if cl.contains(&nl) || nl.contains(&cl) {
                return true;
            }
            for speech in prior_player_speeches(ctx) {
                let sl = normalize_words(&speech);
                if sl.len() >= 8 && (nl.contains(&sl) || sl.contains(&nl)) {
                    return true;
                }
            }
        }
    }
    for quoted in quoted_spans(line) {
        let ql = normalize_words(&quoted);
        if ql.len() < 8 {
            continue;
        }
        for ctx in prior {
            if ctx.is_empty() {
                continue;
            }
            let cl = normalize_words(ctx);
            if cl.contains(&ql) || ql.contains(&cl) {
                return true;
            }
        }
    }
    false
}

/// Strip duplicated speaker labels tiny models echo (`Dewr: "..."` → `...`).
pub fn normalize_npc_line(line: &str, npc_name: &str) -> String {
    let mut out = line.trim().to_string();
    if out.is_empty() {
        return out;
    }
    let name = npc_name.trim();
    if name.is_empty() {
        return out;
    }
    let lower = out.to_lowercase();
    let prefix = format!("{name}:");
    let prefix_lower = prefix.to_lowercase();
    if lower.starts_with(&prefix_lower) {
        out = out[prefix.len()..].trim().to_string();
    }
    if out.eq_ignore_ascii_case("(none)") || out.eq_ignore_ascii_case("none") {
        return String::new();
    }
    out
}

/// Narrator prose — scene paint, action beats, atmosphere (not player/NPC/kernel).
pub fn format_narrator_line(line: &str) -> String {
    let t = line.trim();
    if t.is_empty() {
        return String::new();
    }
    // Already tagged with a non-narrator feed prefix — leave alone.
    if t.starts_with('💬')
        || t.starts_with('🎬')
        || t.starts_with('🎲')
        || t.starts_with('❓')
        || t.starts_with('❌')
        || t.starts_with('✅')
    {
        return t.to_string();
    }
    let mut out = Vec::new();
    for line in t.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('🪶') {
            out.push(trimmed.to_string());
        } else {
            out.push(format!("🪶 {trimmed}"));
        }
    }
    out.join("\n")
}

/// Feed icon for a present character (quest NPC or ambient id/name).
pub fn npc_display_icon(name: &str) -> &'static str {
    if let Some(id) = NpcId::parse(name) {
        return id.icon();
    }
    let key = name.trim().to_lowercase();
    if key.contains("carter") {
        "🛞"
    } else if key.contains("bard") || key.contains("minstrel") {
        "🎻"
    } else if key.contains("merchant") || key.contains("trader") {
        "💰"
    } else if key.contains("guard") {
        "🛡️"
    } else if key.contains("witch") || key.contains("mage") {
        "🔮"
    } else {
        "🧑"
    }
}

/// True when the traveler line is worth echoing in the feed (not bare meta commands).
#[cfg_attr(not(test), allow(dead_code))]
pub fn should_show_player_feed(raw: &str, parts: &PlayerLineParts) -> bool {
    if !parts.action.is_empty() || raw.contains('"') || raw.contains('*') || raw.contains('<') {
        return true;
    }
    let speech = parts.speech.trim();
    if speech.is_empty() {
        return false;
    }
    let lower = speech.to_lowercase();
    !matches!(
        lower.as_str(),
        "look"
            | "l"
            | "inventory"
            | "inv"
            | "i"
            | "help"
            | "?"
            | "leave"
            | "quit"
            | "exit"
            | "dict"
    ) && !lower.starts_with("look ")
}

/// Format for the CLI feed: 🎬 physical beat, then 💬 speech.
#[cfg_attr(not(test), allow(dead_code))]
pub fn format_player_display(name: &str, parts: &PlayerLineParts) -> String {
    let icon = "🎭";
    let name = name.trim();
    let name = if name.is_empty() { "You" } else { name };
    let mut out = String::new();
    if !parts.action.is_empty() {
        let beat = parts.action.trim_matches('*').trim();
        out.push_str(&format!("🎬 {icon} {name} *{beat}*"));
    }
    if !parts.speech.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("💬 {icon} {name}: \"{}\"", parts.speech.trim()));
    }
    out
}

/// Format for the CLI feed: 🎬 physical beat, then 💬 speech.
pub fn format_npc_display(name: &str, staged: &StagedLine) -> String {
    let icon = npc_display_icon(name);
    let mut out = String::new();
    if !staged.action.is_empty() {
        let beat = staged.action.trim_matches('*').trim();
        out.push_str(&format!("🎬 {icon} *{beat}*"));
    }
    let speech = if staged.speech.is_empty() {
        staged.raw.trim_matches('"').trim()
    } else {
        staged.speech.as_str()
    };
    if !speech.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("💬 {icon} {name}: \"{speech}\""));
    } else if out.is_empty() {
        out.push_str(&format!("💬 {icon} {name}: \"…\""));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hexbee_provider::FakeLlm;

    #[tokio::test]
    async fn ambient_cast_from_fake() {
        let llm = FakeLlm::new([
            r#"{"cast":[{"id":"carter","name":"a tired carter","temperament":"weary","description":"Dusty coat by the hearth.","voice":"plain"}]}"#,
        ]);
        let cast = generate_ambient_cast(&llm, "main hall", "sally,groll").await;
        assert_eq!(cast.len(), 1);
        assert_eq!(cast[0].id, "carter");
    }

    #[test]
    fn first_person_pet_is_an_action_not_speech() {
        let p = split_player_line("I walk over to the kitty and pet the kitty.");
        assert!(p.action.to_lowercase().contains("pet"));
        assert!(p.speech.is_empty(), "a bare action is not spoken, got {}", p.speech);
        let shown = format_player_display("Benny", &p);
        assert!(shown.starts_with("🎬"));
        assert!(!shown.contains("💬"));
    }

    #[test]
    fn action_then_spoken_words_split() {
        let p = split_player_line("I walk over to the kitty and say nice kitty");
        assert!(p.action.to_lowercase().contains("walk"));
        assert_eq!(p.speech, "nice kitty");
    }

    #[test]
    fn split_star_and_angle() {
        let p = split_player_line(r#"*I draw my knife* "Step aside.""#);
        assert!(p.action.to_lowercase().contains("knife"));
        assert!(p.speech.to_lowercase().contains("step aside"));
        let p2 = split_player_line("<glare> hello there");
        assert!(p2.action.contains("glare"));
        assert!(p2.speech.contains("hello"));
    }

    #[test]
    fn format_narrator_line_prefixes_once() {
        let out = format_narrator_line("Smoke hangs low over the bar.");
        assert!(out.starts_with("🪶 "));
        assert_eq!(format_narrator_line(&out), out);
    }

    #[test]
    fn format_narrator_line_tags_every_paragraph_line() {
        let out = format_narrator_line(
            "Jules entered the hall.\nHe noticed a cloak on a stool.\nHe could see the hearth.",
        );
        for line in out.lines() {
            assert!(
                line.starts_with("🪶 "),
                "every narrator line needs 🪶, got {line:?}"
            );
        }
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn normalize_strips_speaker_and_none() {
        assert_eq!(
            normalize_npc_line("Dewr: (none)", "Dewr"),
            ""
        );
        assert_eq!(
            normalize_npc_line(r#"Dewr: "Fine. Scram.""#, "Dewr"),
            r#""Fine. Scram.""#
        );
        assert_eq!(
            normalize_npc_line(r#"Sally: Hey, what's on the menu?"#, "Sally"),
            r#"Hey, what's on the menu?"#
        );
    }

    #[test]
    fn speech_to_npc_strips_vocative() {
        assert_eq!(
            speech_to_npc("Hey Sally? What's on the menu?", "Sally"),
            "What's on the menu?"
        );
    }

    #[test]
    fn finalize_rejects_prior_player_echo_from_memory() {
        let fb = r#"*cloak shifts, voice low* "Move along.""#;
        let memory = r#"💬 🎭 Jules: "Hey Groll, What's new?""#;
        let out = finalize_npc_line_with_prior(
            "Jules: Hey Groll, What's new?",
            "Groll",
            "Really? Nothing worth trading?",
            fb,
            &[memory],
        );
        assert_eq!(out, fb);
    }

    #[test]
    fn finalize_rejects_wrong_speaker_relay() {
        let fb = r#"*cloak shifts, voice low* "Move along.""#;
        for bad in [
            r#"Jules: 'Talk to Groll.'"#,
            "Jules: Hey Groll, What's new?",
            r#""Jules: Hey Groll, What's new?""#,
            r#"Groll: "Jules: Hey Groll, What's new?""#,
        ] {
            let out = finalize_npc_line(bad, "Groll", "Really? Nothing worth trading?", fb);
            assert_eq!(out, fb, "should reject relay: {bad}");
        }
    }

    #[test]
    fn finalize_rejects_llm_refusal() {
        let fb = r#"*cloak shifts, voice low* "Move along.""#;
        let out = finalize_npc_line(
            "I'm sorry, but I can't assist with that.",
            "Groll",
            "Talk to Groll.",
            fb,
        );
        assert_eq!(out, fb);
    }

    #[test]
    fn finalize_rejects_fact_dump() {
        let facts = "Cloaked_patron_in_corner=true; dewr_inside_near_door=true; kitchen_door_visible=true";
        let fb = r#"*cloak shifts, voice low* "Move along.""#;
        let out = finalize_npc_line(facts, "Groll", "Groll", fb);
        assert_eq!(out, fb);
    }

    #[test]
    fn finalize_rejects_traveler_echo() {
        let fb = "*taps board* \"Stew and ale.\"";
        let out = finalize_npc_line(
            r#"Sally: "Hey, what's on the menu?""#,
            "Sally",
            "Hey Sally? What's on the menu?",
            fb,
        );
        assert_eq!(out, fb);
        let ok = finalize_npc_line(
            r#"*smiles* "Stew, bread, and ale tonight.""#,
            "Sally",
            "What's on the menu?",
            fb,
        );
        assert!(ok.contains("Stew"));
    }

    #[test]
    fn format_npc_display_splits() {
        let staged = parse_staged_line(r#"*ears flatten* "Fine. Scram.""#);
        let out = format_npc_display("Dewr", &staged);
        assert!(out.contains("🎬 👺 *ears flatten*"));
        assert!(out.contains("💬 👺 Dewr:"));
        assert!(out.contains("Fine. Scram."));
    }

    #[test]
    fn format_player_display_uses_name_and_icons() {
        let parts = split_player_line(r#"*bows* "Good evening.""#);
        let out = format_player_display("Mira", &parts);
        assert!(out.contains("🎬 🎭 Mira *bows*"));
        assert!(out.contains("💬 🎭 Mira:"));
    }

    #[test]
    fn should_show_player_feed_skips_bare_look() {
        let parts = split_player_line("look");
        assert!(!should_show_player_feed("look", &parts));
        let parts2 = split_player_line(r#"*peers around*"#);
        assert!(should_show_player_feed(r#"*peers around*"#, &parts2));
    }
}
