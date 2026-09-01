//! Amber framed terminal UI that clears and resizes with the window.
//! Line input uses rustyline so arrow keys / history work.

use std::future::Future;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub use hexbee_kernel::{term_height, FrameLayout};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

const AMBER: &str = "\x1b[38;5;214m";
const DIM_AMBER: &str = "\x1b[38;5;172m";
const ORANGE: &str = "\x1b[38;5;208m";
const NARRATION: &str = "\x1b[38;5;141m";
const GREEN: &str = "\x1b[32m";
const WHITE: &str = "\x1b[97m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";
const HIDE_CARET: &str = "\x1b[?25l";
const SHOW_CARET: &str = "\x1b[?25h";

/// Braille spinner frames — cheap to draw while waiting on the model.
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Keep the newest lines that fit in the scroll region.
pub fn scroll_tail(lines: &[String], max_rows: usize) -> Vec<String> {
    if max_rows == 0 {
        return Vec::new();
    }
    if lines.len() <= max_rows {
        return lines.to_vec();
    }
    let mut out = vec!["…".into()];
    out.extend(
        lines
            .iter()
            .skip(lines.len().saturating_sub(max_rows - 1))
            .cloned(),
    );
    out
}

fn amber_rule(width: usize) -> String {
    let unit = "===+";
    let mut s = String::new();
    while s.len() + unit.len() <= width {
        s.push_str(unit);
    }
    while s.len() < width {
        s.push('=');
    }
    s
}

pub fn clear_screen() {
    print!("\x1b[2J\x1b[H");
    let _ = io::stdout().flush();
}

/// Pad plain text to `inner_width` columns and close with the right frame rule.
fn framed_row(plain: &str, colored: &str, inner_width: usize) -> String {
    let pad = inner_width.saturating_sub(display_len(plain));
    format!(
        "{AMBER}|{RESET} {colored}{}{AMBER}|{RESET}",
        " ".repeat(pad)
    )
}

pub fn print_scroll_lines(lines: &[String], player_name: &str) {
    let frame = FrameLayout::from_terminal();
    for line in lines {
        let kind = line_kind(line);
        for chunk in wrap_line(line, frame.scroll_width()) {
            println!("{}", colorize_with_kind(&chunk, player_name, kind));
        }
    }
    let _ = io::stdout().flush();
}

/// Clear the terminal and draw an amber ===+ frame around `body` lines.
pub fn draw_board(title: &str, body: &str, footer: &str, player_name: &str) {
    clear_screen();
    let frame = FrameLayout::from_terminal();
    let inner = frame.inner_width;
    let rule = amber_rule(frame.outer_width);

    println!("{AMBER}{rule}{RESET}");
    let title_shown = format!("🐝 {title}");
    let title_colored = format!("{BOLD}{CYAN}{title_shown}{RESET}");
    println!("{}", framed_row(&title_shown, &title_colored, inner));
    println!("{AMBER}{rule}{RESET}");

    for line in body.lines() {
        let kind = line_kind(line);
        for chunk in wrap_line(line, inner) {
            let colored = colorize_with_kind(&chunk, player_name, kind);
            println!("{}", framed_row(&chunk, &colored, inner));
        }
    }

    if !footer.is_empty() {
        println!("{DIM_AMBER}{rule}{RESET}");
        for line in footer.lines() {
            for chunk in wrap_line(line, inner) {
                let colored = colorize_status_footer(&chunk, player_name);
                println!("{}", framed_row(&chunk, &colored, inner));
            }
        }
    }

    println!("{AMBER}{rule}{RESET}");
    let _ = io::stdout().flush();
}

fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut current = String::new();
    for word in line.split(' ') {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if display_len(&candidate) <= width {
            current = candidate;
        } else {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            if display_len(word) <= width {
                current = word.to_string();
            } else {
                let chars: Vec<char> = word.chars().collect();
                for chunk in chars.chunks(width) {
                    out.push(chunk.iter().collect());
                }
            }
        }
    }
    if !current.is_empty() || out.is_empty() {
        out.push(current);
    }
    out
}

fn display_len(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    s.width()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Plain,
    Narration,
    Action,
    Chat,
    Check,
    Success,
    Failure,
}

fn line_kind(line: &str) -> LineKind {
    if line.contains('🎲')
        || line.contains('💥')
        || line.contains('💨')
        || line.contains("⚔️")
    {
        LineKind::Check
    } else if line.starts_with("🎬") {
        LineKind::Action
    } else if line.starts_with('🪶') {
        LineKind::Narration
    } else if line.starts_with("💬") {
        LineKind::Chat
    } else if line.contains('✅')
        || line.contains('🏆')
        || line.contains("SUCCESS")
        || line.contains('✨')
    {
        LineKind::Success
    } else if line.contains('❌')
        || line.contains('💀')
        || line.contains("FAIL")
        || line.contains('🚫')
    {
        LineKind::Failure
    } else {
        LineKind::Plain
    }
}

fn highlight_player_name(line: &str, base_color: &str, player_name: &str) -> String {
    let name = player_name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case("you") || !line.contains(name) {
        return format!("{base_color}{line}{RESET}");
    }
    let mut out = String::from(base_color);
    let mut rest = line;
    while let Some(pos) = rest.find(name) {
        out.push_str(&rest[..pos]);
        out.push_str(&format!("{GREEN}{name}{RESET}{base_color}"));
        rest = &rest[pos + name.len()..];
    }
    out.push_str(rest);
    out.push_str(RESET);
    out
}

fn colorize_status_footer(footer: &str, player_name: &str) -> String {
    let name = player_name.trim();
    if name.is_empty() {
        return format!("{DIM_AMBER}{footer}{RESET}");
    }
    let marker = format!("🎭 {name}");
    if let Some(pos) = footer.find(&marker) {
        let before = &footer[..pos];
        let after = &footer[pos + marker.len()..];
        format!(
            "{DIM_AMBER}{before}{RESET}{GREEN}{marker}{RESET}{DIM_AMBER}{after}{RESET}"
        )
    } else {
        format!("{DIM_AMBER}{footer}{RESET}")
    }
}

fn colorize_with_kind(line: &str, player_name: &str, kind: LineKind) -> String {
    match kind {
        LineKind::Check => format!("{CYAN}{line}{RESET}"),
        LineKind::Action => highlight_player_name(line, ORANGE, player_name),
        LineKind::Narration => format!("{NARRATION}{line}{RESET}"),
        LineKind::Chat => highlight_player_name(line, WHITE, player_name),
        LineKind::Success => format!("{GREEN}{line}{RESET}"),
        LineKind::Failure => format!("{RED}{line}{RESET}"),
        LineKind::Plain => line.to_string(),
    }
}

#[allow(dead_code)] // exercised by unit tests in this module
pub fn colorize_event_line(line: &str, player_name: &str) -> String {
    colorize_with_kind(line, player_name, line_kind(line))
}

/// Await `fut` while showing a spinning status line.
///
/// `label` is the wait reason (e.g. "waiting on the model"). The same hook can
/// later surface longer background tasks you leave and return to.
pub async fn with_spinner<T>(label: &str, fut: impl Future<Output = T>) -> T {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let label = label.to_string();

    // Draw on stderr so we don't fight the framed board on stdout as much.
    let spin = tokio::spawn(async move {
        eprint!("{HIDE_CARET}");
        let _ = io::stderr().flush();
        let mut i = 0usize;
        while !stop_flag.load(Ordering::Relaxed) {
            let frame = SPINNER_FRAMES[i % SPINNER_FRAMES.len()];
            eprint!("\r{AMBER}{frame}{RESET} {DIM_AMBER}{label}…{RESET}   ");
            let _ = io::stderr().flush();
            i = i.wrapping_add(1);
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
        // Clear the spinner line, restore caret.
        eprint!("\r{}\r{SHOW_CARET}", " ".repeat(label.len().saturating_add(12)));
        let _ = io::stderr().flush();
    });

    let result = fut.await;
    stop.store(true, Ordering::Relaxed);
    let _ = spin.await;
    result
}

/// Line editor with arrow-key movement, history, and Ctrl-C / Ctrl-D handling.
pub struct LineEditor {
    inner: DefaultEditor,
}

impl LineEditor {
    pub fn new() -> io::Result<Self> {
        DefaultEditor::new()
            .map(|inner| Self { inner })
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    /// Prompt for a line. Returns `Ok(None)` on Ctrl-C / Ctrl-D / EOF.
    pub fn prompt(&mut self, label: &str) -> io::Result<Option<String>> {
        // Amber prompt prefix (rustyline owns the editable line).
        let prompt = format!("{AMBER}{label}{RESET} ");
        match self.inner.readline(&prompt) {
            Ok(line) => {
                let _ = self.inner.add_history_entry(line.as_str());
                Ok(Some(line))
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => Ok(None),
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_tail_keeps_recent() {
        let lines: Vec<String> = (0..10).map(|i| i.to_string()).collect();
        let tail = scroll_tail(&lines, 4);
        assert_eq!(tail.len(), 4);
        assert_eq!(tail[0], "…");
        assert_eq!(tail[3], "9");
    }

    #[test]
    fn rule_matches_width() {
        let r = amber_rule(40);
        assert_eq!(r.len(), 40);
        assert!(r.contains("===+"));
    }

    #[test]
    fn framed_row_aligns_with_emoji() {
        let inner = 40;
        let plain = "🐝 Hexbee v0.2";
        let row = framed_row(plain, plain, inner);
        assert!(row.ends_with(&format!("{AMBER}|{RESET}")));
        assert!(display_len(plain) > plain.chars().count());
    }

    #[test]
    fn wrapped_narration_keeps_purple_on_continuation_chunk() {
        let long = format!("🪶 {}", "word ".repeat(30));
        let kind = line_kind(&long);
        let chunks = wrap_line(&long, 40);
        assert!(chunks.len() > 1);
        for chunk in &chunks[1..] {
            let colored = colorize_with_kind(chunk, "", kind);
            assert!(
                colored.contains(NARRATION),
                "continuation chunk lost narration color: {chunk}"
            );
        }
    }

    #[test]
    fn colorize_chat_highlights_player_name() {
        let line = r#"💬 🎭 Mira: "Hello there.""#;
        let colored = colorize_event_line(line, "Mira");
        assert!(colored.contains(GREEN));
        assert!(colored.contains(WHITE));
    }

    #[test]
    fn colorize_action_is_orange() {
        let line = "🎬 🎭 Mira *draws a knife*";
        let colored = colorize_event_line(line, "Mira");
        assert!(colored.contains(ORANGE));
        assert!(colored.contains(GREEN));
    }

    #[test]
    fn colorize_narration_is_purple() {
        let line = "🪶 Smoke hangs low over the bar.";
        let colored = colorize_event_line(line, "");
        assert!(colored.contains(NARRATION));
    }

    #[tokio::test]
    async fn spinner_completes_with_future() {
        let v = with_spinner("test wait", async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            42
        })
        .await;
        assert_eq!(v, 42);
    }
}
