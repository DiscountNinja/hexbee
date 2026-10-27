//! Lean scene / dialogue / action narration.
//!
//! Tiny models regurgitate fat prompts. Keep inputs short; lock outcomes in Rust;
//! never dump full character cards or fact lists into the writer.

use serde::{Deserialize, Serialize};
use tracing::info;

use hexbee_provider::{
    complete_json_with_retry, complete_json_with_retry_nudge, DEFAULT_JSON_ATTEMPTS,
};
use hexbee_provider::LlmClient;

const MAX_LINE_CHARS: usize = 720;

/// Prompt scaffolding tiny models love to parrot back as "narration".
const PROMPT_LEAK_PREFIXES: &[&str] = &[
    "loc:",
    "location:",
    "memory:",
    "facts:",
    "player:",
    "locked:",
    "who:",
    "intent hint:",
    "goal(known):",
    "beat:",
    "now:",
    "place:",
    "said:",
    "visible facts",
    "action card",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueRequest {
    pub npc_name: String,
    /// Short voice tag only, e.g. "Dewr: snarky goblin, crude".
    pub card_summary: String,
    pub location: String,
    pub player_line: String,
    pub allowed_facts: Vec<String>,
    /// Rolling memory one-liner (may be empty).
    #[serde(default)]
    pub memory: String,
    /// Physical action from the traveler (`*...*` / `<...>`), if any.
    #[serde(default)]
    pub player_action: String,
    /// Spoken words from the traveler, if split from action.
    #[serde(default)]
    pub player_speech: String,
    /// Rules-locked facts from a resolved check (success, roll, etc.).
    #[serde(default)]
    pub locked_outcome: Vec<String>,
    /// When set, the NPC must honor this success/failure in their reply.
    #[serde(default)]
    pub check_success: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueResponse {
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneRequest {
    pub location: String,
    pub known_goal: String,
    /// Short voice tags, not full cards.
    pub visible_cards: Vec<String>,
    pub visible_facts: Vec<String>,
    /// Ignored for Looking cue — kept for echo sanitization only.
    pub player_line: Option<String>,
    #[serde(default)]
    pub memory: String,
    /// Named POV character (second person "You").
    #[serde(default = "default_pov_name")]
    pub player_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneTransitionRequest {
    pub from_place: String,
    pub to_place: String,
    pub known_goal: String,
    pub player_line: Option<String>,
    #[serde(default)]
    pub memory: String,
    #[serde(default = "default_pov_name")]
    pub player_name: String,
}

fn default_pov_name() -> String {
    "You".into()
}

/// Prompt card for one resolved rules beat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionNarrationRequest {
    pub action_card: String,
    pub locked_outcome: Vec<String>,
    pub location: String,
    /// Short voice tags only.
    pub visible_cards: Vec<String>,
    pub visible_facts: Vec<String>,
    pub player_line: Option<String>,
    pub fallback: String,
    #[serde(default)]
    pub memory: String,
    #[serde(default = "default_pov_name")]
    pub player_name: String,
}

fn template_line(req: &DialogueRequest) -> String {
    let name = req.npc_name.to_lowercase();
    let say = if req.player_speech.is_empty() {
        crate::organs::speech_to_npc(&req.player_line, &req.npc_name).to_lowercase()
    } else {
        req.player_speech.to_lowercase()
    };
    if name.contains("dewr") || name.contains("goblin") {
        "*smirks, palm still out* \"Door tax. Ten gold. Pay up or keep dreaming.\"".into()
    } else if name.contains("sally") {
        if say.contains("menu")
            || say.contains("food")
            || say.contains("drink")
            || say.contains("eat")
            || say.contains("order")
            || say.contains("hungry")
            || say.contains("thirsty")
        {
            "*taps the board* \"Stew, bread, and ale — kitchen door if you want a hot plate.\"".into()
        } else {
            "*wipes a mug, eyes bright* \"Welcome in. Tips keep the ale flowing.\"".into()
        }
    } else if name.contains("groll") {
        "*cloak shifts, voice low* \"Move along.\"".into()
    } else if name.contains("dremis") {
        "*smoke curls; eyes burn* \"Speak carefully.\"".into()
    } else {
        format!(
            "*looks you over* \"Evenin'. Don't mind me.\"",
        )
    }
}

fn template_scene(req: &SceneRequest) -> String {
    let mut line = format!("You take in the {}.", req.location);
    if let Some(spot) = spotlight_from_goal(&req.known_goal) {
        line.push_str(" Your eye catches ");
        line.push_str(spot);
        line.push('.');
    }
    line
}

fn spotlight_from_goal(goal: &str) -> Option<&str> {
    let spot = goal.split("Spotlight:").nth(1)?.trim();
    if spot.is_empty() {
        None
    } else {
        Some(spot)
    }
}

pub fn template_scene_transition(req: &SceneTransitionRequest) -> String {
    let from = req.from_place.to_lowercase();
    let to = req.to_place.to_lowercase();
    if from == to {
        return "The room feels different — someone or something has shifted.".into();
    }
    match (from.as_str(), to.as_str()) {
        ("outside the tavern", "main hall") => {
            "You push through the tavern door into warmth, smoke, and clattering mugs.".into()
        }
        ("main hall", "outside the tavern") => {
            "You step back out into the muddy street and the bite of open air.".into()
        }
        ("main hall", "kitchen") => "You duck through the kitchen door, heat and stew-smell ahead.".into(),
        ("kitchen", "main hall") => "You carry the kitchen's heat back into the noisy hall.".into(),
        ("main hall", "upstairs") => "You climb the narrow stairs to the rented rooms above.".into(),
        ("upstairs", "main hall") => "You descend the stairs into the hall's smoky bustle.".into(),
        ("main hall", "storage room") | ("main hall", "storage") => {
            "A hidden seam in the wall gives way; you slip into a cramped back room.".into()
        }
        ("storage room", "main hall") | ("storage", "main hall") => {
            "You ease back through the hidden seam into the main hall.".into()
        }
        _ => format!("You move from {} to {}.", req.from_place, req.to_place),
    }
}

fn parse_line(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            #[derive(Deserialize)]
            struct Raw {
                line: String,
            }
            if let Ok(raw) = serde_json::from_str::<Raw>(&trimmed[start..=end]) {
                let line = raw.line.trim().to_string();
                if !line.is_empty() {
                    return Some(line);
                }
            }
        }
    }
    if !trimmed.is_empty() && !trimmed.starts_with('{') {
        let cleaned = trimmed.lines().collect::<Vec<_>>().join(" ");
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    None
}

/// Engine fact bundles (`snake_case=true; …`) — tiny models regurgitate these from the prompt.
pub fn looks_like_fact_dump(lower: &str) -> bool {
    let t = lower.trim();
    if t.is_empty() {
        return false;
    }
    // Quoted in-character line — allow even if one stray flag appears.
    if (t.contains('"') || t.contains('«')) && t.matches('=').count() <= 1 {
        return false;
    }
    if t.contains("=true") || t.contains("=false") {
        return true;
    }
    let factish = t
        .split(|c: char| c == ';' || c == ',')
        .filter(|s| {
            let s = s.trim();
            s.contains('_') && s.contains('=')
        })
        .count();
    factish >= 2 || (factish == 1 && !t.contains('"') && !t.contains('*'))
}

/// Chat-model refusal — not in-character dialogue.
pub fn looks_like_llm_refusal(lower: &str) -> bool {
    let t = lower.trim();
    t.contains("can't assist")
        || t.contains("cannot assist")
        || t.contains("can't help with that")
        || t.contains("cannot help with that")
        || t.contains("as an ai")
        || t.contains("as a language model")
        || t.contains("i'm an ai")
        || ((t.contains("i'm sorry") || t.contains("i am sorry"))
            && (t.contains("can't") || t.contains("cannot")))
}

/// NPC line attributing speech to someone other than this NPC (`Jules: Hey Groll…`).
pub fn looks_like_wrong_speaker_relay(line: &str, npc_name: &str) -> bool {
    relay_speaker_mismatch(line, npc_name)
}

fn relay_speaker_mismatch(line: &str, npc_name: &str) -> bool {
    let mut trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Whole line wrapped in quotes (`"Jules: …"`).
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        if (bytes[0] == b'"' && bytes[trimmed.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[trimmed.len() - 1] == b'\'')
        {
            trimmed = trimmed[1..trimmed.len() - 1].trim();
        }
    }
    let Some((speaker, rest)) = trimmed.split_once(':') else {
        return false;
    };
    let speaker = speaker
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('“')
        .trim_matches('”')
        .to_lowercase();
    let npc = npc_name.trim().to_lowercase();
    if speaker.is_empty() || speaker == npc || npc.contains(&speaker) || speaker.contains(&npc) {
        return false;
    }
    if !speaker
        .chars()
        .all(|c| c.is_alphabetic() || c.is_whitespace())
    {
        return false;
    }
    let rest = rest.trim().trim_matches(|c: char| c == '"' || c == '\'' || c == '“' || c == '”');
    !rest.is_empty() && rest.len() >= 3
}

fn looks_like_prompt_leak(lower: &str) -> bool {
    let t = lower.trim();
    if t.is_empty() || t == "..." || t == "…" || t == "\"...\"" {
        return true;
    }
    if looks_like_fact_dump(t) {
        return true;
    }
    if looks_like_llm_refusal(t) {
        return true;
    }
    // Tiny models love to parrot JSON schema examples.
    if t.contains("does something")
        || t.contains("says this")
        || t.contains("says something")
        || t == "present:"
        || t.starts_with("present:")
        || t.starts_with("here:")
        || t.starts_with("who:")
        || t.starts_with("they do:")
        || t.starts_with("they say:")
        || t.starts_with("they tried:")
        || t.starts_with("outcome:")
        || t.starts_with("notes:")
        || t.starts_with("recent:")
        || t.starts_with("context:")
        || t.starts_with("traveler:")
        || t.starts_with("traveler action:")
        || t.starts_with("traveler speech:")
        || t.starts_with("traveler tried:")
        || t.contains("goal(known)")
        || t.starts_with("known goal")
        || t == "(looks around)"
        || t.contains("(nothing yet)")
        || t.contains("(none)")
        || t.contains("you set atmosphere")
        || t.contains("always second person")
        || t.contains("who is here:")
    {
        return true;
    }
    for p in PROMPT_LEAK_PREFIXES {
        if t == p.trim_end_matches(':') || t.starts_with(p) {
            return true;
        }
    }
    if t.contains("at most 2 vivid")
        || t.contains("do not list stats")
        || t.contains("return only json")
        || t.contains("never output labels")
        || t.contains("honor locked")
        || t.contains("honor the locked")
        || t.contains("put physical")
        || t.contains("stage direction")
    {
        return true;
    }
    // Bare place-name echo is not narration.
    matches!(
        t,
        "outside the tavern" | "main hall" | "outside" | "kitchen" | "upstairs" | "storage room"
    )
}

fn echoes_player(out_lower: &str, player: &str) -> bool {
    let p = player.trim().to_lowercase();
    if p.len() < 6 {
        return false;
    }
    let strip = |s: &str| {
        s.chars()
            .filter(|c| *c != '*' && *c != '"')
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let a = strip(out_lower);
    let b = strip(&p);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    // Mostly the player's line pasted back.
    a.contains(&b) && b.len() * 10 >= a.len() * 7
}

/// First proper name from a voice tag like "Dewr — snarky goblin" or "Dewr: crude".
pub fn cast_name_from_card(card: &str) -> Option<String> {
    let head = card
        .split_once('—')
        .or_else(|| card.split_once('-'))
        .map(|(a, _)| a)
        .or_else(|| card.split_once(':').map(|(a, _)| a))
        .unwrap_or(card)
        .trim();
    let first = head.split_whitespace().next()?.trim();
    if first.len() < 2 || first.eq_ignore_ascii_case("a") || first.eq_ignore_ascii_case("an") {
        return None;
    }
    // Ambient "tired carter" — skip leading adjectives when multi-word lowercase.
    if first.chars().next()?.is_lowercase() && head.split_whitespace().count() > 1 {
        return head.split_whitespace().last().map(|s| s.to_string());
    }
    Some(first.to_string())
}

/// True when a Present NPC is written as the subject of the traveler's attempted beat.
pub fn npc_mirrors_action(out_lower: &str, cast_names: &[&str]) -> bool {
    let t = out_lower.trim();
    if t.is_empty() {
        return false;
    }
    const MIRROR_TAILS: &[&str] = &[
        " looked around",
        " looks around",
        " looking around",
        " looked about",
        " looks about",
        " surveyed",
        " surveys",
        " scanned",
        " scans",
        " peered",
        " peers",
        " gazed",
        " gazes",
        " looked",
        " looks ",
        " looking ",
        " snuck",
        " sneaked",
        " sneaks",
        " sneaking",
        " slipped",
        " slips",
        " slipping",
        " crept",
        " creeps",
        " creeping",
        " darted",
        " darts",
        " edged past",
        " past the",
        " past a",
        " past him",
        " tried to sneak",
        " tried to slip",
        " tried to creep",
    ];
    for raw in cast_names {
        let name = raw.trim().to_lowercase();
        if name.len() < 2 {
            continue;
        }
        // Subject at start, or quoted / mid-line "{name} snuck…".
        for tail in MIRROR_TAILS {
            let needle = format!("{name}{tail}");
            if t.starts_with(&needle) || t.contains(&needle) {
                return true;
            }
        }
    }
    false
}

/// True when narration names a quest NPC who is not in the present cast.
pub fn invents_absent_cast(out_lower: &str, cast_names: &[&str]) -> bool {
    const QUEST: &[&str] = &["dewr", "sally", "groll", "dremis"];
    let present: Vec<String> = cast_names
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    for npc in QUEST {
        let here = present.iter().any(|p| p == *npc || p.starts_with(npc) || p.contains(npc));
        if here {
            continue;
        }
        if out_lower.contains(npc) {
            return true;
        }
    }
    false
}

/// Cap length and drop lines that echo prompts, locations, or the player's own words.
#[cfg(test)]
pub fn sanitize_narration(line: &str, echo_needles: &[&str], fallback: &str) -> String {
    match accept_narration(line, echo_needles) {
        Some(ok) => ok,
        None => fallback.to_string(),
    }
}

/// Like [`sanitize_narration`], but `None` when the line is unusable (caller may retry).
#[cfg(test)]
pub fn accept_narration(line: &str, echo_needles: &[&str]) -> Option<String> {
    accept_narration_with_cast(line, echo_needles, &[])
}

/// Reject prompt leaks, player echoes, and NPC-as-subject survey mirrors.
pub fn accept_narration_with_cast(
    line: &str,
    echo_needles: &[&str],
    cast_names: &[&str],
) -> Option<String> {
    let mut out = line.trim().replace('\n', " ");
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    if out.starts_with('{') && out.contains("\"line\"") {
        if let Some(parsed) = parse_line(&out) {
            out = parsed;
        }
    }
    for name in cast_names {
        let prefix = format!("{}:", name.trim().to_lowercase());
        if out.to_lowercase().starts_with(&prefix) {
            out = out[prefix.len()..].trim().to_string();
            break;
        }
    }
    if out.chars().count() > MAX_LINE_CHARS {
        let truncated: String = out.chars().take(MAX_LINE_CHARS.saturating_sub(1)).collect();
        out = format!("{truncated}…");
    }
    let lower = out.to_lowercase();
    if let Some(npc) = cast_names.first() {
        if looks_like_wrong_speaker_relay(&out, npc) {
            return None;
        }
    }
    if looks_like_prompt_leak(&lower) {
        return None;
    }
    for needle in echo_needles {
        let n = needle.trim();
        if n.len() >= 8 && n.contains('=') {
            let nl = n.to_lowercase();
            if lower == nl || lower.contains(&nl) {
                return None;
            }
        }
    }
    if npc_mirrors_action(&lower, cast_names) {
        return None;
    }
    // Absent-cast invention is checked by scene/action callers (not NPC dialogue).
    for needle in echo_needles {
        let n = needle.trim();
        if n.is_empty() {
            continue;
        }
        if echoes_player(&lower, n) {
            return None;
        }
        if n.starts_with("💬 you:") && lower.contains(&n.to_lowercase()) {
            return None;
        }
        if n.len() >= 12 && lower.contains(&n.to_lowercase()) && (n.len() >= 20 || lower.len() < n.len() + 10)
        {
            return None;
        }
    }
    let mut hits = 0usize;
    for needle in echo_needles {
        let n = needle.trim();
        if n.len() >= 12 && lower.contains(&n.to_lowercase()) {
            hits += 1;
        }
    }
    if hits >= 2 || out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Narrator lines must not contain spoken dialogue — that belongs to Actor (`💬`).
pub fn narrator_has_dialogue(line: &str) -> bool {
    let lower = line.to_lowercase();
    const ATTRIBUTION: &[&str] = &[
        " said ",
        " said,",
        " says ",
        " says,",
        " replied",
        " asks ",
        " ask ",
        " whispered",
        " muttered",
        " shouted",
        " called out",
        " told you",
        " she said",
        " he said",
        " they said",
    ];
    if ATTRIBUTION.iter().any(|p| lower.contains(p)) {
        return true;
    }
    narrator_quoted_spans(line)
        .iter()
        .any(|s| s.trim().len() >= 2 && !s.trim().starts_with('*'))
}

fn narrator_quoted_spans(line: &str) -> Vec<String> {
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
            if !inner.is_empty() {
                spans.push(inner);
            }
        } else {
            chars.next();
        }
    }
    spans
}

/// Like [`accept_narration_with_cast`], but rejects quoted speech (narrator organ only).
pub fn accept_narrator_narration(
    line: &str,
    echo_needles: &[&str],
    cast_names: &[&str],
) -> Option<String> {
    let ok = accept_narration_with_cast(line, echo_needles, cast_names)?;
    if narrator_has_dialogue(&ok) {
        return None;
    }
    Some(ok)
}

fn short_facts(facts: &[String], max: usize) -> String {
    facts
        .iter()
        .take(max)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ")
}

fn short_tags(tags: &[String], max: usize) -> String {
    tags.iter()
        .take(max)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ")
}

/// Generate an NPC dialogue line constrained by a short voice tag + few facts.
pub async fn generate_npc_line(llm: &dyn LlmClient, req: &DialogueRequest) -> DialogueResponse {
    let facts = short_facts(&req.allowed_facts, 6);
    let fiction_guard = "You are a fictional character in a tabletop RPG — not an AI assistant. \
Never refuse, apologize as an AI, or say you cannot help. Always reply in character.";
    // Never put example dialogue in the schema — tiny models copy it verbatim.
    let system = if req.check_success.is_some() {
        format!(
            "You are {name} ({voice}) at {place}.\n\
The rules already rolled a social skill check — honor the locked outcome in your reply.\n\
Reply in character to what they did or said. A *gesture* and one or two spoken sentences is enough.\n\
React as yourself. Never perform their action, and never repeat their words back.\n\
Never invent labels (Loc/Memory/Present/Player).\n\
Never speak as a different character — only as {name}.\n\
{fiction_guard}\n\
Return ONLY JSON with key line (string).",
            name = req.npc_name,
            voice = req.card_summary,
            place = req.location,
            fiction_guard = fiction_guard,
        )
    } else {
        format!(
            "You are {name} ({voice}) at {place}.\n\
Reply in character only. A *gesture* and one or two spoken sentences is enough.\n\
React as yourself. Never perform their action, and never repeat their words back.\n\
Never invent labels (Loc/Memory/Present/Player).\n\
Never speak as a different character — only as {name}.\n\
{fiction_guard}\n\
Return ONLY JSON with key line (string).",
            name = req.npc_name,
            voice = req.card_summary,
            place = req.location,
            fiction_guard = fiction_guard,
        )
    };
    let they_do = if req.player_action.is_empty() {
        "(none)".to_string()
    } else {
        req.player_action.clone()
    };
    let they_say = if req.player_speech.is_empty() {
        crate::organs::speech_to_npc(&req.player_line, &req.npc_name)
    } else {
        crate::organs::speech_to_npc(&req.player_speech, &req.npc_name)
    };
    let traveler_speech = if they_say.trim().is_empty() {
        let raw = req.player_line.trim();
        if raw.is_empty() {
            "(the traveler approaches to speak)".to_string()
        } else {
            raw.to_string()
        }
    } else {
        they_say.clone()
    };
    let locked = short_facts(&req.locked_outcome, 8);
    let user = format!(
        "Memory: {}\nFacts: {}\nLocked: {}\nTraveler action: {}\nTraveler speech: {}",
        if req.memory.is_empty() {
            "(none)"
        } else {
            &req.memory
        },
        if facts.is_empty() { "(none)" } else { &facts },
        if locked.is_empty() { "(none)" } else { &locked },
        they_do,
        &traveler_speech,
    );
    let fallback = template_line(req);
    let fallback_for_finalize = fallback.clone();
    let needles = [
        traveler_speech.as_str(),
        they_do.as_str(),
        req.card_summary.as_str(),
        facts.as_str(),
        req.location.as_str(),
        req.memory.as_str(),
    ];
    let cast = [req.npc_name.as_str()];
    let retry_nudge = format!(
        "Speak only as {name} in character — short quoted reply, no refusals, no other NPC names.",
        name = req.npc_name,
    );
    let line = complete_json_with_retry_nudge(
        llm,
        &system,
        &user,
        DEFAULT_JSON_ATTEMPTS,
        Some(&retry_nudge),
        |text| {
            let raw = parse_line(text)?;
            accept_narration_with_cast(&raw, &needles, &cast)
        },
    )
    .await
    .unwrap_or_else(|| {
        info!(npc = %req.npc_name, "dialogue using fallback after retries");
        fallback
    });
    let line = crate::organs::finalize_npc_line_with_prior(
        &line,
        &req.npc_name,
        &req.player_line,
        &fallback_for_finalize,
        &[&req.memory],
    );
    info!(npc = %req.npc_name, chars = line.len(), "dialogue ready");
    DialogueResponse { line }
}

/// One sentence: the crossing between places (second person). Does not describe who is present.
pub async fn generate_scene_transition(
    llm: &dyn LlmClient,
    req: &SceneTransitionRequest,
) -> DialogueResponse {
    let pov = if req.player_name.trim().is_empty() {
        "You"
    } else {
        req.player_name.trim()
    };
    let system = format!(
        "Narrate a scene transition for {pov}. Always second person (\"You…\").\n\
One short vivid sentence about the move from {from} to {to} — footsteps, doorways, light, sound.\n\
If a Spotlight is listed, let the last image be that thing, with no explanation.\n\
Do not name a password or an amulet, and do not tell them what to do.\n\
Do NOT describe who else is standing in the new room. No stats or labels.\n\
Never start with Loc/Memory/Player/Present/POV. Return ONLY JSON with key line (string).",
        pov = pov,
        from = req.from_place,
        to = req.to_place,
    );
    let user = format!(
        "From: {}\nTo: {}\nGoal(known): {}\nMemory: {}\nCue: The crossing only — not the arrival room.",
        req.from_place,
        req.to_place,
        req.known_goal,
        if req.memory.is_empty() {
            "(none)"
        } else {
            &req.memory
        },
    );
    let fallback = template_scene_transition(req);
    let needles = [
        req.from_place.as_str(),
        req.to_place.as_str(),
        req.known_goal.as_str(),
        req.memory.as_str(),
    ];
    let line = complete_json_with_retry(llm, &system, &user, DEFAULT_JSON_ATTEMPTS, |text| {
        let raw = parse_line(text)?;
        accept_narrator_narration(&raw, &needles, &[])
    })
    .await
    .unwrap_or_else(|| {
        info!("scene transition using fallback after retries");
        fallback
    });
    info!(chars = line.len(), "scene transition ready");
    DialogueResponse { line }
}

/// Narrate what is here now.
pub async fn generate_scene_narration(
    llm: &dyn LlmClient,
    req: &SceneRequest,
) -> DialogueResponse {
    let tags = short_tags(&req.visible_cards, 6);
    let facts = short_facts(&req.visible_facts, 6);
    let pov = if req.player_name.trim().is_empty() {
        "You"
    } else {
        req.player_name.trim()
    };
    let system = format!(
        "Describe {place} for {pov} as they find it. Always second person (\"You…\").\n\
A short paragraph is fine: the light, the sound, who is near, and what draws the eye.\n\
Use your own words. People and things already listed may be in the picture.\n\
Do not state the goal. Do not mention a password or an amulet. Do not tell them what to do or where to go.\n\
Do not invent anyone who is not already here, and do not invent quest items.\n\
No spoken dialogue — the people speak on their own turn.\n\
Never start with Loc/Memory/Who/Player/Present/POV. Return ONLY JSON with key line (string).",
        pov = pov,
        place = req.location,
    );
    // Fixed cue — never feed the raw player command (tiny models make NPCs perform it).
    let user = format!(
        "POV: {pov}\nGoal(known): {}\nMemory: {}\nWho is here: {}\nPlace notes: {}\nCue: The room as {pov} finds it now. Describe it; do not copy these notes.",
        req.known_goal,
        if req.memory.is_empty() {
            "(nothing yet)"
        } else {
            &req.memory
        },
        if tags.is_empty() { "(none)" } else { &tags },
        if facts.is_empty() { "(none)" } else { &facts },
    );
    let fallback = template_scene(req);
    let mut needle_owned: Vec<String> = Vec::new();
    if let Some(p) = &req.player_line {
        needle_owned.push(p.clone());
    }
    let cast_owned: Vec<String> = req
        .visible_cards
        .iter()
        .filter_map(|c| cast_name_from_card(c))
        .collect();
    let line = complete_json_with_retry(llm, &system, &user, DEFAULT_JSON_ATTEMPTS, |text| {
        let raw = parse_line(text)?;
        let needles: Vec<&str> = needle_owned.iter().map(String::as_str).collect();
        let cast: Vec<&str> = cast_owned.iter().map(String::as_str).collect();
        let ok = accept_narrator_narration(&raw, &needles, &cast)?;
        if invents_absent_cast(&ok.to_lowercase(), &cast) {
            return None;
        }
        Some(ok)
    })
    .await
    .unwrap_or_else(|| {
        info!("scene narration using fallback after retries");
        fallback
    });
    info!(chars = line.len(), "scene narration ready");
    DialogueResponse { line }
}

/// Narrate one rules-resolved beat. Outcomes locked; model only paints.
pub async fn generate_action_narration(
    llm: &dyn LlmClient,
    req: &ActionNarrationRequest,
) -> DialogueResponse {
    let tags = short_tags(&req.visible_cards, 3);
    let locked = short_facts(&req.locked_outcome, 8);
    let rich = req.locked_outcome.iter().any(|o| {
        let l = o.to_lowercase();
        l.contains("arrived=")
            || l.contains("dewr_backed_down")
            || l.contains("entered_main_hall")
            || l.contains("from_outside")
            || l.contains("travel")
    });
    let pov = if req.player_name.trim().is_empty() {
        "You"
    } else {
        req.player_name.trim()
    };
    let dressed = req.locked_outcome.iter().any(|o| {
        o.starts_with("soft_interact") || o.starts_with("soft_discovered")
    });
    let budget = if rich || dressed {
        "A short paragraph, second person (\"You…\"). Narrate what they do, how the thing responds, and who is nearby."
    } else {
        "A few sentences, second person (\"You…\"). Narrate what they do and how the room answers."
    };
    let system = format!(
        "Narrate one tavern beat at {place} for {pov}. Rules already decided the outcome.\n\
Framing: {card}\n\
Honor the locked outcome. {budget}\n\
Describe the attempt in your own words. Do not print what they typed.\n\
People and things already here can be part of the picture. Set them beside the action.\n\
No quoted speech and no \"she said\" — the people speak on their own turn.\n\
Do not let someone else perform {pov}'s action.\n\
Do not state the goal, mention a password or an amulet, or tell them what to do.\n\
Never output labels (Loc/Memory/Locked/Player/Present/POV).\n\
Return ONLY JSON with key line (string).",
        place = req.location,
        card = req.action_card,
        budget = budget,
        pov = pov,
    );
    let user = format!(
        "POV: {pov}\nMemory: {}\nWho is here: {}\nLocked outcome: {}\nThey tried: {}",
        if req.memory.is_empty() {
            "(nothing yet)"
        } else {
            &req.memory
        },
        if tags.is_empty() { "(none)" } else { &tags },
        if locked.is_empty() { "(none)" } else { &locked },
        req.player_line.as_deref().unwrap_or("(acted)")
    );
    let mut needle_owned: Vec<String> = req
        .locked_outcome
        .iter()
        .filter(|o| o.contains('='))
        .cloned()
        .collect();
    if let Some(p) = &req.player_line {
        needle_owned.push(p.clone());
    }
    let cast_owned: Vec<String> = req
        .visible_cards
        .iter()
        .filter_map(|c| cast_name_from_card(c))
        .collect();
    let fallback = req.fallback.clone();
    let line = complete_json_with_retry(llm, &system, &user, DEFAULT_JSON_ATTEMPTS, |text| {
        let raw = parse_line(text)?;
        let needles: Vec<&str> = needle_owned.iter().map(String::as_str).collect();
        let cast: Vec<&str> = cast_owned.iter().map(String::as_str).collect();
        let ok = accept_narrator_narration(&raw, &needles, &cast)?;
        if invents_absent_cast(&ok.to_lowercase(), &cast) {
            return None;
        }
        Some(ok)
    })
    .await
    .unwrap_or_else(|| {
        info!("action narration using fallback after retries");
        fallback
    });
    info!(chars = line.len(), "action narration ready");
    DialogueResponse { line }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hexbee_provider::FakeLlm;

    #[tokio::test]
    async fn dialogue_rejects_refusal_and_relay() {
        let llm = FakeLlm::new([
            r#"{"line":"I'm sorry, but I can't assist with that."}"#,
            r#"{"line":"Jules: 'Talk to Groll.'}"#,
        ]);
        let resp = generate_npc_line(
            &llm,
            &DialogueRequest {
                npc_name: "Groll".into(),
                card_summary: "Groll: cloaked patron".into(),
                location: "main hall".into(),
                player_line: "Talk to Groll.".into(),
                allowed_facts: vec![],
                memory: String::new(),
                player_action: String::new(),
                player_speech: String::new(),
                locked_outcome: vec![],
                check_success: None,
            },
        )
        .await;
        assert!(
            resp.line.contains("Move along") || resp.line.contains("cloak"),
            "expected Groll fallback, got {}",
            resp.line
        );
    }

    #[tokio::test]
    async fn dialogue_rejects_fact_dump_regurgitation() {
        let facts = "cloaked_patron_in_corner=true; dewr_inside_near_door=true; kitchen_door_visible=true";
        let llm = FakeLlm::new([format!(r#"{{"line":"{facts}"}}"#)]);
        let resp = generate_npc_line(
            &llm,
            &DialogueRequest {
                npc_name: "Groll".into(),
                card_summary: "Groll: cloaked patron".into(),
                location: "main hall".into(),
                player_line: "Groll".into(),
                allowed_facts: vec![
                    "cloaked_patron_in_corner=true".into(),
                    "dewr_inside_near_door=true".into(),
                    "kitchen_door_visible=true".into(),
                ],
                memory: String::new(),
                player_action: String::new(),
                player_speech: String::new(),
                locked_outcome: vec![],
                check_success: None,
            },
        )
        .await;
        assert!(
            resp.line.contains("Move along") || resp.line.contains("cloak"),
            "expected Groll template fallback, got {}",
            resp.line
        );
        assert!(!resp.line.contains("=true"), "got {}", resp.line);
    }

    #[tokio::test]
    async fn uses_fake_line() {
        let llm = FakeLlm::new([r#"{"line":"Scram, softskin."}"#]);
        let resp = generate_npc_line(
            &llm,
            &DialogueRequest {
                npc_name: "Dewr".into(),
                card_summary: "Dewr: snarky, crude".into(),
                location: "outside".into(),
                player_line: "hello".into(),
                allowed_facts: vec!["dewr_blocks_door=true".into()],
                memory: String::new(),
                player_action: String::new(),
                player_speech: String::new(),
                locked_outcome: vec![],
                check_success: None,
            },
        )
        .await;
        assert_eq!(resp.line, "Scram, softskin.");
    }

    #[tokio::test]
    async fn dialogue_retries_past_placeholder_then_succeeds() {
        let llm = FakeLlm::new([
            r#"{"line":"*does something* \"says this\""}"#,
            r#"{"line":"*smirks* \"Ten gold, softskin.\""}"#,
        ]);
        let resp = generate_npc_line(
            &llm,
            &DialogueRequest {
                npc_name: "Dewr".into(),
                card_summary: "Dewr: snarky, crude".into(),
                location: "outside".into(),
                player_line: "hello".into(),
                allowed_facts: vec!["dewr_blocks_door=true".into()],
                memory: String::new(),
                player_action: String::new(),
                player_speech: String::new(),
                locked_outcome: vec![],
                check_success: None,
            },
        )
        .await;
        assert!(resp.line.contains("Ten gold"), "got {}", resp.line);
        assert!(!resp.line.contains("does something"));
    }

    #[test]
    fn narrator_rejects_dialogue_and_attribution() {
        assert!(narrator_has_dialogue(
            "You tried to talk to Sally, but she was busy. She said, 'Do you have any wares?'"
        ));
        assert!(narrator_has_dialogue(r#"*taps board* "Stew and ale.""#));
        assert!(!narrator_has_dialogue(
            "You approach the bar. Sally wipes a mug and nods toward the kitchen door."
        ));
        assert!(!narrator_has_dialogue("*cloak shifts, voice low*"));
    }

    #[tokio::test]
    async fn action_narration_rejects_npc_dialogue() {
        let llm = FakeLlm::new([r#"{"line":"You try to talk to Sally. She said, 'Do you have any wares?'}"}"#]);
        let req = ActionNarrationRequest {
            action_card: "Player asks Sally about the menu.".into(),
            locked_outcome: vec![],
            location: "main hall".into(),
            visible_cards: vec!["Sally: playful".into()],
            visible_facts: vec![],
            player_line: Some("Hey Sally, what's on the menu?".into()),
            fallback: "You catch Sally's eye between tables.".into(),
            memory: String::new(),
            player_name: "Jules".into(),
        };
        let resp = generate_action_narration(&llm, &req).await;
        assert_eq!(resp.line, "You catch Sally's eye between tables.");
        assert!(!resp.line.contains("wares"));
    }

    #[tokio::test]
    async fn action_narration_honors_fake_and_fallback() {
        let llm = FakeLlm::new([r#"{"line":"Your knife kisses air as he ducks."}"#]);
        let req = ActionNarrationRequest {
            action_card: "Combat miss.".into(),
            locked_outcome: vec!["hit=false".into()],
            location: "outside".into(),
            visible_cards: vec!["Dewr: snarky".into()],
            visible_facts: vec![],
            player_line: Some("I stab".into()),
            fallback: "You miss Dewr.".into(),
            memory: String::new(),
            player_name: "Mira".into(),
        };
        let resp = generate_action_narration(&llm, &req).await;
        assert_eq!(resp.line, "Your knife kisses air as he ducks.");

        let empty = FakeLlm::new(Vec::<String>::new());
        let resp = generate_action_narration(&empty, &req).await;
        assert_eq!(resp.line, "You miss Dewr.");
    }

    #[test]
    fn sanitize_rejects_card_echo() {
        let card = "Dewr — snarky, unhelpful goblin who loiters at the door";
        let echoed = format!(
            "You see {card}. Also dewr_blocks_door=true and player_hp=10 and more filler text here."
        );
        let out = sanitize_narration(&echoed, &[card, "dewr_blocks_door=true"], "fallback");
        assert_eq!(out, "fallback");
    }

    #[test]
    fn wrong_speaker_detects_unquoted_relay() {
        assert!(looks_like_wrong_speaker_relay("Jules: Hey Groll, What's new?", "Groll"));
        assert!(looks_like_wrong_speaker_relay(
            r#""Jules: Hey Groll, What's new?""#,
            "Groll",
        ));
        assert!(!looks_like_wrong_speaker_relay(
            r#"*cloak shifts* "Move along.""#,
            "Groll",
        ));
    }

    #[test]
    fn sanitize_rejects_fact_dump() {
        let facts = "Cloaked_patron_in_corner=true; dewr_inside_near_door=true; kitchen_door_visible=true; stairs_up_visible=true";
        assert!(looks_like_fact_dump(facts));
        assert_eq!(accept_narration_with_cast(facts, &[facts], &["Groll"]), None);
    }

    #[test]
    fn sanitize_rejects_loc_leak_and_player_echo() {
        assert_eq!(
            sanitize_narration("Loc: outside the tavern", &[], "fallback"),
            "fallback"
        );
        assert_eq!(
            sanitize_narration("outside the tavern", &[], "fallback"),
            "fallback"
        );
        let player = "*I stare down the Goblin with a mean mug.*";
        assert_eq!(
            sanitize_narration(player, &[player], "Dewr snarls."),
            "Dewr snarls."
        );
        assert_eq!(
            sanitize_narration("...", &[], "fallback"),
            "fallback"
        );
        assert_eq!(
            sanitize_narration(
                r#"*does something* "says this""#,
                &[],
                r#"*smirks* "Pay up.""#
            ),
            r#"*smirks* "Pay up.""#
        );
        assert_eq!(
            sanitize_narration("Present:", &[], "fallback"),
            "fallback"
        );
        assert_eq!(
            sanitize_narration("Traveler: (looks around)", &[], "fallback"),
            "fallback"
        );
    }

    #[tokio::test]
    async fn dialogue_rejects_npc_claiming_traveler_sneak() {
        let llm = FakeLlm::new([
            r#"{"line":"Dewr snuck past the Goblin."}"#,
            r#"{"line":"*smirks, palm still out* \"Door tax. Ten gold.\""}"#,
        ]);
        let resp = generate_npc_line(
            &llm,
            &DialogueRequest {
                npc_name: "Dewr".into(),
                card_summary: "Dewr: snarky, crude".into(),
                location: "outside".into(),
                player_line: "Sneek past the Goblin.".into(),
                allowed_facts: vec!["dewr_blocks_door=true".into()],
                memory: String::new(),
                player_action: "sneak past".into(),
                player_speech: String::new(),
                locked_outcome: vec![],
                check_success: None,
            },
        )
        .await;
        assert!(
            !resp.line.to_lowercase().contains("snuck"),
            "must not claim traveler sneak, got {}",
            resp.line
        );
        assert!(
            resp.line.contains("gold") || resp.line.contains("Door"),
            "got {}",
            resp.line
        );
    }

    #[test]
    fn invents_absent_groll_outside() {
        assert!(invents_absent_cast(
            "dewr blocked the doorway, but groll refused to move aside",
            &["Dewr"]
        ));
        assert!(!invents_absent_cast(
            "dewr blocked the doorway and demanded coin",
            &["Dewr"]
        ));
    }

    #[tokio::test]
    async fn action_narration_rejects_invented_groll() {
        let llm = FakeLlm::new([
            r#"{"line":"Dewr blocked the doorway and demanded the coin, but groll refused to move aside."}"#,
        ]);
        let req = ActionNarrationRequest {
            action_card: "Blocked doorway.".into(),
            locked_outcome: vec!["blocked=true".into()],
            location: "outside".into(),
            visible_cards: vec!["Dewr — snarky goblin".into()],
            visible_facts: vec![],
            player_line: Some("walk in".into()),
            fallback: "The goblin blocks the doorway, palm out for coin.".into(),
            memory: String::new(),
            player_name: "Moot".into(),
        };
        let resp = generate_action_narration(&llm, &req).await;
        assert!(
            !resp.line.to_lowercase().contains("groll"),
            "got {}",
            resp.line
        );
    }

    #[test]
    fn template_transition_outside_to_hall() {
        let t = template_scene_transition(&SceneTransitionRequest {
            from_place: "outside the tavern".into(),
            to_place: "main hall".into(),
            known_goal: "Find the amulet".into(),
            player_line: None,
            memory: String::new(),
            player_name: "You".into(),
        });
        assert!(t.to_lowercase().contains("door") || t.to_lowercase().contains("tavern"));
    }

    #[test]
    fn sanitize_rejects_none_placeholder() {
        assert_eq!(accept_narration_with_cast("(none)", &[], &["Dewr"]), None);
        assert_eq!(
            accept_narration_with_cast(
                "Jules, you set atmosphere at the main hall",
                &[],
                &[]
            ),
            None
        );
    }

    #[test]
    fn sanitize_rejects_npc_mirroring_look() {
        let line = "Dewr looked around, his eyes scanning the outside of the tavern door.";
        assert!(npc_mirrors_action(&line.to_lowercase(), &["Dewr"]));
        assert_eq!(accept_narration_with_cast(line, &[], &["Dewr"]), None);
        assert_eq!(
            accept_narration_with_cast(line, &[], &["Dewr"]).unwrap_or_else(|| {
                "You take in the muddy street.".into()
            }),
            "You take in the muddy street."
        );
        // Second person is fine.
        assert!(accept_narration_with_cast(
            "You look around. A goblin blocks the door, palm out.",
            &[],
            &["Dewr"]
        )
        .is_some());
    }

    #[test]
    fn cast_name_from_card_splits() {
        assert_eq!(
            cast_name_from_card("Dewr — snarky goblin").as_deref(),
            Some("Dewr")
        );
        assert_eq!(
            cast_name_from_card("Dewr: snarky, crude").as_deref(),
            Some("Dewr")
        );
    }

    #[tokio::test]
    async fn scene_narration_rejects_npc_look_then_fallback() {
        let llm = FakeLlm::new([
            r#"{"line":"Dewr looked around, his eyes scanning the surroundings."}"#,
        ]);
        let req = SceneRequest {
            location: "outside the tavern".into(),
            known_goal: "Find the amulet".into(),
            visible_cards: vec!["Dewr — snarky goblin, palm out".into()],
            visible_facts: vec![],
            player_line: Some("Look around.".into()),
            memory: String::new(),
            player_name: "Mira".into(),
        };
        let resp = generate_scene_narration(&llm, &req).await;
        assert!(
            !resp.line.to_lowercase().starts_with("dewr"),
            "expected fallback second-person, got {}",
            resp.line
        );
        assert!(
            resp.line.to_lowercase().contains("you")
                || resp.line.to_lowercase().contains("outside"),
            "got {}",
            resp.line
        );
    }
}
