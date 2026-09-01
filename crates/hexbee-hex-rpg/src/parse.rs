use crate::state::{Location, NpcId};
use crate::organs::ActionTag;

/// Player commands understood by the keyword parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerAction {
    Look,
    Inventory,
    Help,
    Go(Location),
    Talk(Option<String>),
    BuyFood,
    Sleep,
    TipSally,
    CharmSally,
    PersuadeGroll,
    ChallengeDewr,
    GivePassword,
    ImpressDremis,
    Fight(Option<NpcId>),
    AcceptSwordTrade,
    /// Pay Dewr's fake door tax (scam).
    PayScam,
    /// Call out / refuse the door scam.
    CallOutScam,
    Leave,
    Unknown(String),
}

impl PlayerAction {
    /// True when the keyword parser did not confidently map the line.
    pub fn is_unknown(&self) -> bool {
        matches!(self, PlayerAction::Unknown(_))
    }

    /// Convert an LLM/heuristic [`ActionTag`] into a concrete action.
    pub fn from_action_tag(tag: &ActionTag) -> Self {
        let npc = tag.npc.as_deref().and_then(NpcId::parse);
        match tag.action.as_str() {
            "look" => PlayerAction::Look,
            "inventory" => PlayerAction::Inventory,
            "help" => PlayerAction::Help,
            "go" => {
                let loc = tag
                    .location
                    .as_deref()
                    .and_then(Location::parse)
                    .unwrap_or(Location::MainHall);
                PlayerAction::Go(loc)
            }
            "talk" => PlayerAction::Talk(tag.npc.clone()),
            "buy_food" => PlayerAction::BuyFood,
            "sleep" => PlayerAction::Sleep,
            "tip_sally" => PlayerAction::TipSally,
            "charm_sally" => PlayerAction::CharmSally,
            "persuade_groll" => PlayerAction::PersuadeGroll,
            "challenge_dewr" => PlayerAction::ChallengeDewr,
            "give_password" => PlayerAction::GivePassword,
            "impress_dremis" => PlayerAction::ImpressDremis,
            "fight" => PlayerAction::Fight(npc),
            "accept_sword" => PlayerAction::AcceptSwordTrade,
            "pay_scam" | "pay" => PlayerAction::PayScam,
            "call_out_scam" | "refuse" => PlayerAction::CallOutScam,
            "leave" => PlayerAction::Leave,
            other => PlayerAction::Unknown(other.to_string()),
        }
    }
}

/// Rules-first keyword parser for tavern actions.
pub fn parse_action(input: &str) -> PlayerAction {
    let raw = input.trim();
    if raw.is_empty() {
        return PlayerAction::Unknown(raw.to_string());
    }
    let lower = raw.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let joined = words.join(" ");

    if matches_any(&joined, &["help", "?"]) || joined == "commands" {
        return PlayerAction::Help;
    }
    if matches_any(&joined, &["look", "l", "examine", "where am i"]) {
        return PlayerAction::Look;
    }
    if matches_any(
        &joined,
        &["inventory", "inv", "i", "status", "stats", "gold", "hp"],
    ) {
        return PlayerAction::Inventory;
    }
    if joined.starts_with("go ")
        || joined.starts_with("move ")
        || joined.starts_with("enter ")
        || joined.starts_with("walk ")
    {
        let rest = words[1..].join(" ");
        if rest.is_empty() && joined.starts_with("enter") {
            return PlayerAction::Go(Location::MainHall);
        }
        if let Some(loc) = Location::parse(&rest) {
            return PlayerAction::Go(loc);
        }
    }
    if matches_any(&joined, &["enter", "enter tavern", "go in", "go inside", "head inside"]) {
        return PlayerAction::Go(Location::MainHall);
    }
    if let Some(loc) = Location::parse(&joined) {
        // bare location name
        if words.len() <= 3 {
            return PlayerAction::Go(loc);
        }
    }
    if joined.contains("upstairs") {
        return PlayerAction::Go(Location::Upstairs);
    }
    if joined.contains("kitchen") {
        return PlayerAction::Go(Location::Kitchen);
    }
    if joined.contains("storage") {
        return PlayerAction::Go(Location::StorageRoom);
    }
    if joined.contains("main hall") || joined == "hall" {
        return PlayerAction::Go(Location::MainHall);
    }
    if joined == "outside" || joined.contains("go outside") {
        return PlayerAction::Go(Location::Outside);
    }

    if joined.starts_with("talk")
        || joined.starts_with("speak")
        || joined.starts_with("ask")
        || joined.starts_with("say")
        || joined.starts_with("hello")
        || joined.starts_with("hi ")
        || joined == "hi"
    {
        return PlayerAction::Talk(extract_npc_name(&joined));
    }

    if joined.contains("buy food")
        || joined.contains("order food")
        || joined == "food"
        || joined.split_whitespace().any(|w| w == "eat" || w == "food" || w == "hungry")
    {
        return PlayerAction::BuyFood;
    }
    if joined.contains("sleep") || joined.contains("rent room") || joined.contains("rent a room")
    {
        return PlayerAction::Sleep;
    }
    if joined.contains("tip sally") || joined == "tip" || joined.starts_with("tip ") {
        return PlayerAction::TipSally;
    }
    if joined.contains("charm") || joined.contains("seduce") || joined.contains("flirt") {
        return PlayerAction::CharmSally;
    }
    if joined.contains("password")
        && (joined.contains("give")
            || joined.contains("tell")
            || joined.contains("say")
            || joined.contains("dremis"))
    {
        return PlayerAction::GivePassword;
    }
    if joined.contains("impress") {
        return PlayerAction::ImpressDremis;
    }
    if joined.contains("persuade")
        || joined.contains("ask groll")
        || (joined.contains("groll") && joined.contains("password"))
        || joined.contains("ask for password")
    {
        return PlayerAction::PersuadeGroll;
    }
    if joined.contains("challenge") || (joined.contains("back down") && joined.contains("dewr")) {
        return PlayerAction::ChallengeDewr;
    }
    if joined.contains("scam")
        || joined.contains("call out")
        || joined.contains("see through")
        || joined == "refuse"
        || joined.contains("refuse")
        || joined.contains("not paying")
        || joined.contains("don't pay")
        || joined.contains("dont pay")
        || joined.contains("fake fee")
        || (joined.contains("door tax") && (joined.contains("no") || joined.contains("lie")))
    {
        return PlayerAction::CallOutScam;
    }
    if joined.contains("pay dewr")
        || joined.contains("pay fee")
        || joined.contains("pay tax")
        || joined.contains("pay scam")
        || joined == "pay"
        || joined.contains("pay the")
        || joined.contains("give gold") && joined.contains("dewr")
    {
        return PlayerAction::PayScam;
    }
    if (joined.contains("accept") && joined.contains("sword"))
        || joined.contains("buy sword")
        || (joined.contains("trade") && joined.contains("sword"))
    {
        return PlayerAction::AcceptSwordTrade;
    }
    if joined.starts_with("fight")
        || joined.starts_with("attack")
        || joined.starts_with("kill")
        || joined == "combat"
    {
        return PlayerAction::Fight(extract_npc(&joined));
    }
    if matches_any(
        &joined,
        &[
            "leave",
            "exit",
            "quit tavern",
            "leave tavern",
            "leave area",
            "walk away",
            "flee area",
        ],
    ) || joined.starts_with("leave ")
    {
        return PlayerAction::Leave;
    }

    // Soft talk targeting by name alone in a sentence
    if let Some(name) = extract_npc_name(&joined) {
        if words.len() >= 2 {
            return PlayerAction::Talk(Some(name));
        }
    }

    PlayerAction::Unknown(raw.to_string())
}

fn matches_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay == *n)
}

fn extract_npc(text: &str) -> Option<NpcId> {
    for key in crate::slang::NPC_KEYS {
        if crate::slang::mentions_npc(text, key) {
            return NpcId::parse(key);
        }
    }
    None
}

fn extract_npc_name(text: &str) -> Option<String> {
    extract_npc(text).map(|n| n.display_name().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_verbs() {
        assert_eq!(parse_action("look"), PlayerAction::Look);
        assert_eq!(
            parse_action("go upstairs"),
            PlayerAction::Go(Location::Upstairs)
        );
        assert_eq!(
            parse_action("talk to dewr"),
            PlayerAction::Talk(Some("dewr".into()))
        );
        assert_eq!(parse_action("leave tavern"), PlayerAction::Leave);
        assert_eq!(parse_action("charm sally"), PlayerAction::CharmSally);
        assert_eq!(parse_action("enter"), PlayerAction::Go(Location::MainHall));
        assert_eq!(parse_action("go outside"), PlayerAction::Go(Location::Outside));
        assert_eq!(parse_action("pay"), PlayerAction::PayScam);
        assert_eq!(parse_action("refuse"), PlayerAction::CallOutScam);
        assert!(parse_action("I politely decline your ridiculous fee").is_unknown());
    }

    #[test]
    fn action_tag_maps() {
        use crate::organs::ActionTag;
        let tag = ActionTag {
            action: "call_out_scam".into(),
            npc: Some("dewr".into()),
            location: None,
            confidence: Some(4),
            reason: Some("test".into()),
        };
        assert_eq!(
            PlayerAction::from_action_tag(&tag),
            PlayerAction::CallOutScam
        );
    }
}
