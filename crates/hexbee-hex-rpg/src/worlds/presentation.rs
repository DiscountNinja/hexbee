//! **Presentation** — what it feels like (atmosphere, NPC stammer, smell of the hall).
//!
//! May expand every turn via Narrator and Actor organs. Must not mutate canon or
//! situation except by returning prose to the feed.
//!
//! Organs in this layer receive read-only snapshots of canon + situation facts.
//! They never receive mutable [`WorldState`].

/// Marker: presentation output is feed text only.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PresentationBeat {
    pub line: String,
}

#[allow(dead_code)]
impl PresentationBeat {
    pub fn new(line: impl Into<String>) -> Self {
        Self { line: line.into() }
    }
}

/// Facts safe to show models for painting — never write these back as truth.
#[allow(dead_code)]
pub fn paint_facts(location: &str, memory: &str, roster: &[String]) -> Vec<String> {
    let mut out = vec![format!("location={location}")];
    if !memory.is_empty() {
        out.push(format!("memory={memory}"));
    }
    if !roster.is_empty() {
        out.push(format!("present={}", roster.join("; ")));
    }
    out
}
