//! Terminal viewport — frame width tracks the live text window.

/// Outer frame width in columns (clamped 40–200).
pub fn term_width() -> usize {
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(n) = cols.parse::<usize>() {
            return n.clamp(40, 200);
        }
    }
    if let Ok(output) = std::process::Command::new("stty")
        .arg("size")
        .stdin(std::process::Stdio::inherit())
        .output()
    {
        if output.status.success() {
            if let Ok(text) = String::from_utf8(output.stdout) {
                if let Some(cols) = text.split_whitespace().nth(1) {
                    if let Ok(n) = cols.parse::<usize>() {
                        return n.clamp(40, 200);
                    }
                }
            }
        }
    }
    80
}

/// Terminal height in rows (clamped 20–120).
pub fn term_height() -> usize {
    if let Ok(rows) = std::env::var("LINES") {
        if let Ok(n) = rows.parse::<usize>() {
            return n.clamp(20, 120);
        }
    }
    if let Ok(output) = std::process::Command::new("stty")
        .arg("size")
        .stdin(std::process::Stdio::inherit())
        .output()
    {
        if output.status.success() {
            if let Ok(text) = String::from_utf8(output.stdout) {
                if let Some(rows) = text.split_whitespace().next() {
                    if let Ok(n) = rows.parse::<usize>() {
                        return n.clamp(20, 120);
                    }
                }
            }
        }
    }
    24
}

/// Amber frame geometry derived from the current terminal width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLayout {
    /// Full width including both vertical rules.
    pub outer_width: usize,
    /// Text column width between `| … |`.
    pub inner_width: usize,
}

impl FrameLayout {
    /// `| ` + inner + ` |` → outer = inner + 4.
    pub fn from_terminal() -> Self {
        let outer_width = term_width();
        let inner_width = outer_width.saturating_sub(4).max(20);
        Self {
            outer_width,
            inner_width,
        }
    }

    /// Width for unframed scroll lines (no side rules).
    pub fn scroll_width(&self) -> usize {
        self.inner_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_inner_fits_outer() {
        let f = FrameLayout {
            outer_width: 80,
            inner_width: 76,
        };
        assert_eq!(f.outer_width, f.inner_width + 4);
    }

    #[test]
    fn from_terminal_is_sane() {
        let f = FrameLayout::from_terminal();
        assert!(f.outer_width >= 40);
        assert_eq!(f.inner_width + 4, f.outer_width);
    }
}
