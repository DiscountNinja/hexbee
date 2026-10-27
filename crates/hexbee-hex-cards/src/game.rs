//! Cards table lobby and turn loop.

use hexbee_provider::LlmClient;

use crate::ai::{choose_move, expand_legal_for_ai, heuristic_discard, heuristic_pass};
use crate::card::Card;
use crate::deck::SeatId;
use crate::moves::{IllegalMove, Move};
use crate::player::SeatKind;
use crate::rules::{DrawPokerGame, HeartsGame, TableGame, TwentyOneGame};
use crate::table::{GameKind, TableState};

pub enum GameEvent {
    Lines(Vec<String>),
}

pub struct CardsGame {
    pub table: TableState,
    engine: Option<Box<dyn TableGame>>,
    pub over: bool,
}

impl CardsGame {
    pub fn new(seed: Option<u64>) -> Self {
        Self {
            table: TableState::new(seed),
            engine: None,
            over: false,
        }
    }

    pub fn intro(&self) -> String {
        "🃏 Cards table — four seats (you + 3 AI). Type 21, hearts, or poker. `menu` / `quit` anytime."
            .into()
    }

    pub fn status_line(&self) -> String {
        match self.table.game {
            Some(g) => format!(
                "{} │ {} │ turn: {}",
                g.display(),
                self.table.scores_line(),
                self.table.player(self.table.to_act).name
            ),
            None => format!("Lobby │ {}", self.table.scores_line()),
        }
    }

    pub async fn handle_line(
        &mut self,
        input: &str,
        llm: Option<&dyn LlmClient>,
    ) -> Vec<GameEvent> {
        let lower = input.trim().to_lowercase();
        if lower.is_empty() {
            return vec![];
        }
        if matches!(lower.as_str(), "quit" | "exit" | "leave") {
            self.over = true;
            return vec![GameEvent::Lines(vec![
                "Leaving the cards table.".into(),
            ])];
        }
        if matches!(lower.as_str(), "menu" | "lobby" | "games") {
            self.engine = None;
            self.table.game = None;
            return vec![GameEvent::Lines(vec![
                "Lobby — choose 21, hearts, or poker.".into(),
                self.table.scores_line(),
            ])];
        }
        if matches!(lower.as_str(), "score" | "scores") {
            return vec![GameEvent::Lines(vec![self.table.scores_line()])];
        }
        if matches!(lower.as_str(), "help" | "?") {
            return vec![GameEvent::Lines(help_lines(self))];
        }

        if self.engine.is_none() {
            if let Some(kind) = GameKind::parse(&lower) {
                let mut lines = self.start_game(kind);
                lines.extend(self.run_auto(llm).await);
                return vec![GameEvent::Lines(lines)];
            }
            return vec![GameEvent::Lines(vec![
                "Pick a game: 21 · hearts · poker".into(),
            ])];
        }

        // Deal next hand
        if matches!(lower.as_str(), "deal" | "next" | "hand") {
            if let Some(engine) = self.engine.as_mut() {
                let mut lines = engine.start_hand(&mut self.table);
                lines.extend(self.run_auto(llm).await);
                return vec![GameEvent::Lines(lines)];
            }
        }

        let seat = self.table.to_act;
        if self.table.player(seat).kind != SeatKind::Human {
            // Nudge auto-play
            let mut lines = self.run_auto(llm).await;
            if lines.is_empty() {
                lines.push("Waiting on AI…".into());
            }
            return vec![GameEvent::Lines(lines)];
        }

        match parse_human_move(input, self) {
            Ok(mv) => {
                let mut lines = match self.apply_move(seat, mv) {
                    Ok(msgs) => msgs,
                    Err(err) => vec![err.display()],
                };
                lines.extend(self.run_auto(llm).await);
                vec![GameEvent::Lines(lines)]
            }
            Err(msg) => vec![GameEvent::Lines(vec![msg])],
        }
    }

    fn start_game(&mut self, kind: GameKind) -> Vec<String> {
        self.table.setup_players_for(kind);
        self.table.game = Some(kind);
        let mut engine: Box<dyn TableGame> = match kind {
            GameKind::TwentyOne => Box::new(TwentyOneGame::new()),
            GameKind::Hearts => Box::new(HeartsGame::new()),
            GameKind::DrawPoker => Box::new(DrawPokerGame::new()),
        };
        let mut lines = vec![format!("Starting {}.", kind.display())];
        lines.extend(engine.start_hand(&mut self.table));
        self.engine = Some(engine);
        lines
    }

    fn apply_move(&mut self, seat: SeatId, mv: Move) -> Result<Vec<String>, IllegalMove> {
        let engine = self.engine.as_mut().ok_or_else(|| {
            IllegalMove::new("no game", vec![])
        })?;
        let legal = engine.legal_moves(&self.table, seat);
        // Allow human pass/discard with concrete cards even if legal is sentinel
        let ok = legal_accepts(&legal, &mv);
        if !ok {
            return Err(IllegalMove::new(
                format!("cannot {} now", mv.label()),
                legal,
            ));
        }
        let effect = engine.apply(&mut self.table, seat, mv)?;
        let mut lines = effect.messages;
        if effect.hand_over || engine.is_hand_over(&self.table) {
            let deltas = engine.settle(&mut self.table);
            for d in &deltas {
                lines.push(format!(
                    "{} {:+} → {}",
                    self.table.player(d.seat).name,
                    d.delta,
                    self.table.score_of(d.seat)
                ));
            }
            lines.push(self.table.scores_line());
            lines.push("Type `deal` for next hand, `menu` for lobby.".into());
        } else {
            lines.extend(engine.render_public(&self.table));
            if self.table.player(self.table.to_act).kind == SeatKind::Human {
                lines.push(engine.prompt_hint(&self.table, self.table.to_act));
            }
        }
        Ok(lines)
    }

    async fn run_auto(&mut self, llm: Option<&dyn LlmClient>) -> Vec<String> {
        let mut lines = Vec::new();
        for _ in 0..64 {
            if self.engine.is_none() {
                break;
            }
            let hand_over = self
                .engine
                .as_ref()
                .map(|e| e.is_hand_over(&self.table))
                .unwrap_or(true);
            if hand_over {
                break;
            }
            let seat = self.table.to_act;
            if self.table.player(seat).kind == SeatKind::Human {
                if let Some(engine) = self.engine.as_ref() {
                    lines.push(engine.prompt_hint(&self.table, seat));
                }
                break;
            }
            let (legal, kind) = {
                let engine = self.engine.as_ref().unwrap();
                let legal = engine.legal_moves(&self.table, seat);
                (legal, self.table.game)
            };
            let legal = expand_legal_for_ai(kind, &self.table, seat, legal);
            if legal.is_empty() {
                lines.push(format!(
                    "(no legal moves for {})",
                    self.table.player(seat).name
                ));
                break;
            }
            let mv = {
                let engine = self.engine.as_ref().unwrap();
                choose_move(engine.as_ref(), &self.table, seat, &legal, llm)
                    .await
                    .unwrap_or_else(|| legal[0].clone())
            };
            match self.apply_move(seat, mv) {
                Ok(msgs) => lines.extend(msgs),
                Err(err) => {
                    // Force fallback apply of first expanded legal
                    lines.push(format!("AI retry: {}", err.reason));
                    let fb = match kind {
                        Some(GameKind::Hearts) if matches!(legal[0], Move::PassCards(_)) => {
                            heuristic_pass(&self.table, seat)
                        }
                        Some(GameKind::DrawPoker) if matches!(legal[0], Move::Discard(_)) => {
                            heuristic_discard(&self.table, seat)
                        }
                        _ => legal[0].clone(),
                    };
                    match self.apply_move(seat, fb) {
                        Ok(msgs) => lines.extend(msgs),
                        Err(e2) => {
                            lines.push(e2.display());
                            break;
                        }
                    }
                }
            }
        }
        lines
    }

    pub fn sync_runtime(&self) -> serde_json::Value {
        let game = self.table.game.map(|g| g.id());
        serde_json::json!({
            "game": game,
            "scores": self.table.scores,
        })
    }

    pub fn load_scores(&mut self, runtime: &serde_json::Value) {
        if let Some(obj) = runtime.get("scores").and_then(|s| s.as_object()) {
            for (k, v) in obj {
                if let Some(n) = v.as_i64() {
                    self.table.scores.insert(k.clone(), n as i32);
                }
            }
        }
    }
}

fn help_lines(game: &CardsGame) -> Vec<String> {
    let mut v = vec![
        "Commands: 21 | hearts | poker | deal | menu | scores | quit".into(),
        "21: hit / stand".into(),
        "Hearts: pass AS KH QD · play AS".into(),
        "Poker: check / call / fold / bet 10 / raise 20 · discard AS KH · pat".into(),
    ];
    if let Some(engine) = game.engine.as_ref() {
        if game.table.player(game.table.to_act).kind == SeatKind::Human {
            v.push(engine.prompt_hint(&game.table, game.table.to_act));
        }
    }
    v
}

fn legal_accepts(legal: &[Move], mv: &Move) -> bool {
    if legal.is_empty() {
        return false;
    }
    for m in legal {
        match (m, mv) {
            (Move::PassCards(s), Move::PassCards(c)) if s.is_empty() && c.len() == 3 => {
                return true;
            }
            (Move::Discard(s), Move::Discard(c)) if s.is_empty() && c.len() <= 3 => return true,
            (Move::Discard(s), Move::Stand) if s.is_empty() => return true,
            (a, b) if a == b => return true,
            (Move::Hit, Move::Hit) | (Move::Stand, Move::Stand) => return true,
            (Move::Check, Move::Check)
            | (Move::Call, Move::Call)
            | (Move::Fold, Move::Fold) => return true,
            (Move::Bet(_), Move::Bet(_)) | (Move::Raise(_), Move::Raise(_)) => return true,
            (Move::PlayCard(a), Move::PlayCard(b)) if a == b => return true,
            _ => {}
        }
    }
    // Exact play card in list
    legal.iter().any(|m| m == mv)
}

fn parse_human_move(input: &str, game: &CardsGame) -> Result<Move, String> {
    let lower = input.trim().to_lowercase();
    let engine = game.engine.as_ref().ok_or_else(|| "no game".to_string())?;
    let seat = game.table.to_act;
    let legal = engine.legal_moves(&game.table, seat);

    if matches!(lower.as_str(), "hit" | "h") {
        return Ok(Move::Hit);
    }
    if matches!(lower.as_str(), "stand" | "stay" | "s") {
        return Ok(Move::Stand);
    }
    if matches!(lower.as_str(), "check") {
        return Ok(Move::Check);
    }
    if matches!(lower.as_str(), "call") {
        return Ok(Move::Call);
    }
    if matches!(lower.as_str(), "fold") {
        return Ok(Move::Fold);
    }
    if matches!(lower.as_str(), "pat" | "stand pat") {
        return Ok(Move::Discard(vec![]));
    }
    if let Some(rest) = lower.strip_prefix("bet ") {
        let n: u32 = rest
            .trim()
            .parse()
            .map_err(|_| "bet <amount>".to_string())?;
        return Ok(Move::Bet(n));
    }
    if let Some(rest) = lower.strip_prefix("raise ") {
        let n: u32 = rest
            .trim()
            .parse()
            .map_err(|_| "raise <amount>".to_string())?;
        return Ok(Move::Raise(n));
    }
    if let Some(rest) = lower
        .strip_prefix("pass ")
        .or_else(|| lower.strip_prefix("passcards "))
    {
        let cards = parse_card_list(rest)?;
        return Ok(Move::PassCards(cards));
    }
    if let Some(rest) = lower.strip_prefix("discard ") {
        let cards = parse_card_list(rest)?;
        return Ok(Move::Discard(cards));
    }
    if let Some(rest) = lower
        .strip_prefix("play ")
        .or_else(|| lower.strip_prefix("card "))
    {
        let card = Card::parse(rest).ok_or_else(|| format!("bad card: {rest}"))?;
        return Ok(Move::PlayCard(card));
    }
    // Bare card for hearts
    if let Some(card) = Card::parse(input.trim()) {
        return Ok(Move::PlayCard(card));
    }

    let hint = if legal.is_empty() {
        "no moves".into()
    } else {
        legal
            .iter()
            .take(6)
            .map(|m| m.label())
            .collect::<Vec<_>>()
            .join(" · ")
    };
    Err(format!("Could not parse move. Try: {hint}"))
}

fn parse_card_list(s: &str) -> Result<Vec<Card>, String> {
    let mut cards = Vec::new();
    for part in s.split(|c: char| c.is_whitespace() || c == ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let c = Card::parse(part).ok_or_else(|| format!("bad card: {part}"))?;
        cards.push(c);
    }
    if cards.is_empty() {
        return Err("need cards".into());
    }
    Ok(cards)
}
