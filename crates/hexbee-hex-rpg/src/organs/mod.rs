// Scaffold organs — Director/Actor/Judge/Narrator helpers exist; live path uses purpose + dialogue.
mod actor;
mod director;
mod judge;
// mod narrator;

mod gm;
mod purpose;

pub use gm::GmOrgan;
pub use purpose::{purpose_classify, purpose_heuristic, PurposeResult};

pub mod interpret;
pub use interpret::{heuristic_interpret_plan, wants_leave_command, ActionPlan, ActionTag};
pub mod dialogue;
pub use dialogue::{
    generate_action_narration, generate_npc_line, generate_scene_narration,
    generate_scene_transition, template_scene_transition, ActionNarrationRequest,
    DialogueRequest, SceneRequest, SceneTransitionRequest,
};
pub mod competency;
pub mod cast;
pub use cast::{
    finalize_npc_line_with_prior, format_narrator_line, format_npc_display,
    generate_ambient_cast, npc_display_icon, parse_staged_line, propose_soft_detail, speech_to_npc,
    split_player_line,
};
