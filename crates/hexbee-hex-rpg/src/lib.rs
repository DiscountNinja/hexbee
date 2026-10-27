//! Tavern RPG hex module.

mod bridge;
mod cards;
mod checks;
mod combat;
mod game;
mod module;
mod organs;
mod parse;
mod rounds;
mod rules;
mod scene;
mod secrets;
mod social;
mod prices;
mod bind;
mod direction;
mod clarify;
mod lexicon;
mod moves;
mod skills;
mod slang;
mod state;
mod weapons;
mod world_doc;
mod worlds;

pub use bridge::{
    default_world_from_campaign, is_default_player_name, needs_chargen, sync_campaign_to_world,
    sync_world_to_campaign,
};
pub use cards::{load_builtin_cards, CharacterCard};
pub use checks::{dc_for, resolve_d20_check, CheckKind, CheckResult};
pub use combat::{resolve_attack, Combatant};
pub use game::{GameEvent, SceneEnd, TavernGame};
pub use module::{bundled_campaign_dir, TavernHex};
pub use organs::cast::finalize_npc_line;
pub use parse::{parse_action, PlayerAction};
pub use rules::{match_rules, RuleHit, TurnMode};
pub use lexicon::{Lexicon, LexiconKind, LexiconRoute};
pub use secrets::SecretsCatalog;
pub use worlds::{apply_canon, apply_check_success, CanonPatch, Situation, SituationKnob};
pub use social::{detect_talk_check, is_pure_banter, is_off_book_social, wants_social_outcome};
pub use scene::{
    AmbientNpc, SceneCoordinator, SceneSnapshot, TalkFocus, TalkMiss, TalkRequest,
};
pub use skills::{
    ability_mod, Ability, AbilityScores, BuildPreset, CharacterBuild, Skill, PROFICIENCY_BONUS,
    SKILL_PICKS, STANDARD_ARRAY,
};
pub use state::{Location, NpcId, PlayerState, WorldState};
pub use world_doc::{is_room_survey_line, is_soft_search_line, SoftFlag, WorldDoc};
pub use weapons::{weapon_damage_range, Weapon};

