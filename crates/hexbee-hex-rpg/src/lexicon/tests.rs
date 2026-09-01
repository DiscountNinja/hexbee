#[cfg(test)]
mod tests {
    use super::super::types::Lexicon;
    use crate::rules::{match_rules, TurnMode};
    use crate::state::NpcId;

    #[test]
    fn gobbo_normalizes_to_goblin() {
        let lex = Lexicon::bundled();
        assert_eq!(lex.normalize("filthy gobbo"), "filthy goblin");
    }

    #[test]
    fn resolve_gobbo_via_slang() {
        assert_eq!(crate::slang::resolve_npc_alias("gobbo"), Some("goblin"));
        assert_eq!(NpcId::parse("gobbo"), Some(NpcId::Goblin));
    }

    #[test]
    fn asside_typo_routes_challenge() {
        let h = match_rules("Move asside before you get hurt.").unwrap();
        assert_eq!(h.mode, TurnMode::Talk);
        assert_eq!(h.action_hint.as_deref(), Some("challenge_dewr"));
    }
}
