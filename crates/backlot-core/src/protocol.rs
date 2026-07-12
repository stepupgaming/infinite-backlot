//! Structured LLM protocol.
//!
//! All model interaction uses strict structured output. Free-form prose is only
//! allowed inside explicitly narrative fields (`description`, `text`, `payoff`,
//! reasoning notes). Everything else is a bounded token from a known vocabulary
//! so that invalid commands can never reach the Bevy world未经 validation.

use crate::world::WorldState;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Vocabulary (the bounded "tools" the director may use)
// ---------------------------------------------------------------------------

pub const KNOWN_ACTIONS: &[&str] = &[
    // Movement
    "move_to", "enter_room", "exit_room", "approach", "retreat_from", "follow",
    "flee_to", "turn_toward", "look_at", "sit_at", "stand_at",
    // Object interaction
    "pick_up", "put_down", "give", "take", "inspect", "open", "close", "activate",
    "deactivate", "hide_object", "reveal_object", "carry", "drop", "throw_safe",
    "knock_on", "conceal_object",
    // Character performance
    "speak", "react", "gesture", "pause", "interrupt", "laugh", "sigh", "whisper",
    "shout", "point_at", "conceal_emotion", "display_emotion", "write_note",
    // World events
    "flicker_lights", "cut_power", "ring_alarm", "open_elevator", "close_elevator",
    "spawn_authorized_prop", "move_authorized_prop", "change_room_state",
    "play_environment_effect", "trigger_safe_physics_event",
    // Narrative state
    "add_fact", "remove_false_belief", "create_rumor", "resolve_thread",
    "create_thread", "change_relationship", "assign_secret", "schedule_future_event",
    "change_location_condition",
];

pub const KNOWN_CAMERA_INTENTS: &[&str] = &[
    "establish", "follow", "conversation", "speaker_closeup", "reaction", "reveal",
    "insert_object", "comedic_wide", "tension_push", "over_the_shoulder",
    "group_coverage", "exit_transition", "cliffhanger_hold",
];

pub const KNOWN_EMOTIONS: &[&str] = &[
    "neutral", "happy", "sad", "angry", "fear", "suspicion", "confusion",
    "frustration", "surprise", "serene", "strained", "eager",
];

pub const KNOWN_WORLD_EVENTS: &[&str] = &[
    "flicker_lights", "cut_power", "ring_alarm", "open_elevator", "close_elevator",
    "play_environment_effect", "trigger_safe_physics_event",
];

pub fn is_known_action(t: &str) -> bool {
    KNOWN_ACTIONS.contains(&t)
}
pub fn is_known_camera_intent(t: &str) -> bool {
    KNOWN_CAMERA_INTENTS.contains(&t)
}

// ---------------------------------------------------------------------------
// Episode plan (Section 10.2)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EpisodePlan {
    pub episode_title: String,
    pub logline: String,
    #[serde(default)]
    pub tone: Vec<String>,
    pub target_duration_seconds: f32,
    pub active_characters: Vec<String>,
    pub primary_location: String,
    pub central_goal: CentralGoal,
    pub beats: Vec<BeatOutline>,
    pub payoff: String,
    #[serde(default)]
    pub persistent_changes: Vec<PersistentChange>,
    /// Free-form reasoning notes (allowed prose).
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CentralGoal {
    pub character: String,
    pub goal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BeatOutline {
    pub id: String,
    #[serde(rename = "type")]
    pub beat_type: String,
    pub target_start_second: f32,
    pub description: String,
    #[serde(default)]
    pub required_entities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PersistentChange {
    /// add_fact | remove_fact | add_belief | change_relationship |
    /// resolve_thread | create_thread | change_location_condition ...
    pub operation: String,
    pub target: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub amount: Option<f32>,
}

// ---------------------------------------------------------------------------
// Beat command (Section 10.3)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BeatCommand {
    pub beat_id: String,
    pub dramatic_purpose: String,
    pub actions: Vec<ActionCommand>,
    pub camera_intent: CameraIntent,
    #[serde(default)]
    pub expected_state_changes: Vec<ExpectedStateChange>,
    pub completion_condition: CompletionCondition,
    #[serde(default)]
    pub fallback: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActionCommand {
    pub actor: String,
    pub action: String,
    #[serde(default)]
    pub target: Option<String>,
    /// Required for `speak` and similar performance actions.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub intensity: Option<f32>,
    /// Override the default estimated duration (seconds).
    #[serde(default)]
    pub duration_override: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CameraIntent {
    pub r#type: String,
    pub subject: String,
    #[serde(default)]
    pub reaction_subject: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExpectedStateChange {
    pub target: String,
    pub field: String,
    pub operation: String, // increase | decrease | set | add
    #[serde(default)]
    pub amount: Option<f32>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompletionCondition {
    pub r#type: String, // dialogue_finished | arrival | timer | event_done
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub seconds: Option<f32>,
}

// ---------------------------------------------------------------------------
// World digest (Section 24.1) — compact semantic context sent to the model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorldDigest {
    pub location: DigestLocation,
    pub characters: Vec<DigestCharacter>,
    pub threads: Vec<DigestThread>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DigestLocation {
    pub id: String,
    pub description: String,
    pub available_interactions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DigestCharacter {
    pub id: String,
    pub role: String,
    pub mood: String,
    pub goal: Option<String>,
    #[serde(default)]
    pub knows: Vec<String>,
    #[serde(default)]
    pub does_not_know: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DigestThread {
    pub id: String,
    pub summary: String,
    pub importance: f32,
}

impl WorldDigest {
    /// Build a compact digest for the active location + characters.
    pub fn for_episode(world: &WorldState, location_id: &str, characters: &[String]) -> WorldDigest {
        let loc = world.location(location_id);
        let location = match loc {
            Some(l) => DigestLocation {
                id: l.id.clone(),
                description: l.description.clone(),
                available_interactions: l.available_interactions.clone(),
            },
            None => DigestLocation {
                id: location_id.into(),
                description: String::new(),
                available_interactions: vec![],
            },
        };
        let characters = characters
            .iter()
            .filter_map(|id| world.character(id))
            .map(|c| DigestCharacter {
                id: c.id.clone(),
                role: c.role.clone(),
                mood: c.emotion.first().cloned().unwrap_or_else(|| "neutral".into()),
                goal: c.current_goal.clone(),
                knows: c.known_facts.clone(),
                does_not_know: c.believed_facts.clone(),
            })
            .collect();
        let threads = world
            .threads
            .values()
            .map(|t| DigestThread {
                id: t.id.clone(),
                summary: t.summary.clone(),
                importance: t.importance,
            })
            .collect();
        WorldDigest {
            location,
            characters,
            threads,
        }
    }
}
