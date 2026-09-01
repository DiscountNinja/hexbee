use std::collections::HashMap;

/// Lowercase, strip punctuation, collapse spaces, then phrase + token tables.
pub fn normalize_text(
    input: &str,
    phrase_aliases: &HashMap<String, String>,
    token_spelling: &HashMap<String, String>,
) -> String {
    let collapsed = collapse_punctuation(input);
    let phrase_fixed = apply_phrase_aliases(&collapsed, phrase_aliases);
    expand_tokens(&phrase_fixed, token_spelling)
}

fn collapse_punctuation(input: &str) -> String {
    let lower = input.trim().to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_space = false;
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() || c == '\'' {
            out.push(c);
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn apply_phrase_aliases(s: &str, table: &HashMap<String, String>) -> String {
    let mut phrases: Vec<(&String, &String)> = table.iter().collect();
    phrases.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
    let mut out = s.to_string();
    for (from, to) in phrases {
        if out.contains(from.as_str()) {
            out = out.replace(from.as_str(), to.as_str());
        }
    }
    out
}

fn expand_tokens(lower: &str, table: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(lower.len());
    let mut word = String::new();
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() || c == '\'' {
            word.push(c);
        } else {
            if !word.is_empty() {
                push_token(&mut out, &word, table);
                word.clear();
            }
            if c != ' ' {
                out.push(c);
            } else {
                out.push(' ');
            }
        }
    }
    if !word.is_empty() {
        push_token(&mut out, &word, table);
    }
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out.trim().to_string()
}

fn push_token(out: &mut String, token: &str, table: &HashMap<String, String>) {
    let bare = token.trim_matches('\'');
    if let Some(canonical) = table.get(bare) {
        out.push_str(canonical);
    } else {
        out.push_str(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn table() -> (HashMap<String, String>, HashMap<String, String>) {
        let mut phrase = HashMap::new();
        phrase.insert("move asside".into(), "move aside".into());
        phrase.insert("you'll get hurt".into(), "or else".into());
        let mut token = HashMap::new();
        token.insert("gobbo".into(), "goblin".into());
        token.insert("asside".into(), "aside".into());
        (phrase, token)
    }

    #[test]
    fn strips_punctuation_and_collapses_spaces() {
        let (p, t) = table();
        assert_eq!(
            normalize_text("  Hey,   Gobbo!!!  ", &p, &t),
            "hey goblin"
        );
    }
}
