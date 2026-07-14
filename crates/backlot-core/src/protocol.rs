//! Structured LLM protocol.
//!
//! All model interaction uses strict structured output. Free-form prose is only
//! allowed inside explicitly narrative fields (`description`, `text`, `payoff`,
//! reasoning notes). Everything else is a bounded token from a known vocabulary
//! so that invalid commands can never reach the Bevy world未经 validation.

use crate::world::WorldState;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const CURRENT_AUTHORED_SCHEMA_VERSION: u32 = 2;

fn legacy_authored_schema_version() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Executable production vocabulary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityRef {
    Character { id: String },
    Group { ids: Vec<String> },
    Prop { id: String },
    Environment { id: String },
    Mark { id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct BlockingCue {
    pub actor: String,
    #[serde(default)]
    pub path: Vec<String>,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub face: Option<EntityRef>,
    #[serde(default = "default_locomotion")]
    pub locomotion: String,
}

fn default_locomotion() -> String {
    "walk".into()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ActionPhases {
    pub anticipation: f32,
    pub execution: f32,
    pub hold: f32,
    pub recovery: f32,
}

impl Default for ActionPhases {
    fn default() -> Self {
        Self {
            anticipation: 0.15,
            execution: 0.45,
            hold: 0.15,
            recovery: 0.25,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct PerformanceCue {
    pub motion: String,
    #[serde(default = "default_intensity")]
    pub intensity: f32,
    #[serde(default)]
    pub phases: ActionPhases,
    #[serde(default)]
    pub gaze: Option<EntityRef>,
    #[serde(default)]
    pub active_hand: Option<String>,
    #[serde(default)]
    pub prop_contact: Option<EntityRef>,
    #[serde(default)]
    pub sync: Option<String>,
}

fn default_intensity() -> f32 {
    0.6
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentEventKind {
    ElevatorDoors,
    ElevatorIndicator,
    ControlPanel,
    InteriorLight,
    ImpossibleFloorReveal,
    LightingShift,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EnvironmentCue {
    pub target: String,
    pub event: EnvironmentEventKind,
    #[serde(default)]
    pub from: Option<f32>,
    #[serde(default)]
    pub to: Option<f32>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub start_offset: f32,
    #[serde(default = "default_cue_duration")]
    pub duration: f32,
    #[serde(default = "default_easing")]
    pub easing: String,
}

fn default_cue_duration() -> f32 {
    0.8
}

fn default_easing() -> String {
    "smoothstep".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SoundCue {
    pub sound: String,
    #[serde(default)]
    pub source: Option<EntityRef>,
    #[serde(default = "default_gain")]
    pub gain: f32,
    #[serde(default)]
    pub start_offset: f32,
}

fn default_gain() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CameraPurpose {
    Speaker,
    Reaction,
    OverTheShoulder,
    Insert,
    Interaction,
    Reveal,
    Payoff,
    Spatial,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct CameraCue {
    pub purpose: CameraPurpose,
    pub subjects: Vec<EntityRef>,
    #[serde(default)]
    pub required_visible_event: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryEmotion {
    Neutral,
    Warm,
    Amused,
    Anxious,
    Urgent,
    Hushed,
    Stunned,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryPace {
    Slow,
    Measured,
    Natural,
    Fast,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct DeliverySpec {
    pub emotion: DeliveryEmotion,
    #[serde(default = "default_intensity")]
    pub energy: f32,
    #[serde(default = "default_delivery_pace")]
    pub pace: DeliveryPace,
    #[serde(default)]
    pub emphasis: Vec<String>,
    #[serde(default)]
    pub pause_style: Option<String>,
    #[serde(default)]
    pub vocal_effort: Option<String>,
}

fn default_delivery_pace() -> DeliveryPace {
    DeliveryPace::Natural
}

// ---------------------------------------------------------------------------
// Vocabulary (the bounded "tools" the director may use)
// ---------------------------------------------------------------------------

pub const KNOWN_ACTIONS: &[&str] = &[
    // Movement
    "move_to",
    "enter_room",
    "exit_room",
    "approach",
    "retreat_from",
    "follow",
    "flee_to",
    "turn_toward",
    "look_at",
    "sit_at",
    "stand_at",
    // Object interaction
    "pick_up",
    "put_down",
    "give",
    "take",
    "inspect",
    "open",
    "close",
    "activate",
    "deactivate",
    "hide_object",
    "reveal_object",
    "carry",
    "drop",
    "throw_safe",
    "knock_on",
    "conceal_object",
    // Character performance
    "speak",
    "react",
    "gesture",
    "pause",
    "interrupt",
    "laugh",
    "sigh",
    "whisper",
    "shout",
    "point_at",
    "conceal_emotion",
    "display_emotion",
    "write_note",
    // World events
    "flicker_lights",
    "cut_power",
    "ring_alarm",
    "open_elevator",
    "close_elevator",
    "spawn_authorized_prop",
    "move_authorized_prop",
    "change_room_state",
    "play_environment_effect",
    "trigger_safe_physics_event",
    // Narrative state
    "add_fact",
    "remove_false_belief",
    "create_rumor",
    "resolve_thread",
    "create_thread",
    "change_relationship",
    "assign_secret",
    "schedule_future_event",
    "change_location_condition",
];

pub const KNOWN_CAMERA_INTENTS: &[&str] = &[
    "establish",
    "follow",
    "conversation",
    "speaker_closeup",
    "reaction",
    "reveal",
    "insert_object",
    "comedic_wide",
    "tension_push",
    "over_the_shoulder",
    "group_coverage",
    "exit_transition",
    "cliffhanger_hold",
];

pub const KNOWN_EMOTIONS: &[&str] = &[
    "neutral",
    "happy",
    "sad",
    "angry",
    "fear",
    "suspicion",
    "confusion",
    "frustration",
    "surprise",
    "serene",
    "strained",
    "eager",
];

pub const KNOWN_WORLD_EVENTS: &[&str] = &[
    "flicker_lights",
    "cut_power",
    "ring_alarm",
    "open_elevator",
    "close_elevator",
    "play_environment_effect",
    "trigger_safe_physics_event",
];

pub const KNOWN_COMPLETION_TYPES: &[&str] = &[
    "dialogue_finished",
    "arrival",
    "timer",
    "event_done",
    "animation_finished",
];

pub fn is_known_action(t: &str) -> bool {
    KNOWN_ACTIONS.contains(&t)
}
pub fn is_known_camera_intent(t: &str) -> bool {
    KNOWN_CAMERA_INTENTS.contains(&t)
}
pub fn is_known_completion(t: &str) -> bool {
    KNOWN_COMPLETION_TYPES.contains(&t)
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
    #[serde(default)]
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
    #[serde(default)]
    pub blocking: Vec<BlockingCue>,
    #[serde(default)]
    pub environment: Vec<EnvironmentCue>,
    #[serde(default)]
    pub sounds: Vec<SoundCue>,
    #[serde(default)]
    pub camera_cue: Option<CameraCue>,
    #[serde(default)]
    pub visible_action: Option<String>,
    #[serde(default)]
    pub intended_reaction: Option<String>,
    #[serde(default)]
    pub performance_intent: Option<String>,
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
    #[serde(default)]
    pub performance: Option<PerformanceCue>,
    #[serde(default)]
    pub delivery: Option<DeliverySpec>,
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

// ---------------------------------------------------------------------------
// Whole-episode authored response (redesigned single-call authoring)
// ---------------------------------------------------------------------------
//
// Instead of asking the model for a `EpisodePlan` and then N separate
// `BeatCommand` calls, the redesigned authoring collapses everything into ONE
// structured response. `AuthoredEpisode` is the model-facing schema: it carries
// the episode metadata AND every fully-authored beat in a single object.
//
// Crucially there is exactly ONE beat identifier field — `AuthoredBeat.id`. The
// runtime derives the internal `beat_id` from it during adaptation, so the old
// `id` vs `beat_id` confusion that used to trigger an extra model call can
// never happen again.

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthoredEpisode {
    #[serde(default = "legacy_authored_schema_version")]
    pub schema_version: u32,
    pub episode_title: String,
    pub logline: String,
    #[serde(default)]
    pub tone: Vec<String>,
    pub target_duration_seconds: f32,
    pub active_characters: Vec<String>,
    pub primary_location: String,
    pub central_goal: CentralGoal,
    pub beats: Vec<AuthoredBeat>,
    pub payoff: String,
    #[serde(default)]
    pub persistent_changes: Vec<PersistentChange>,
    /// Free-form reasoning notes (allowed prose).
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthoredBeat {
    /// The ONLY beat identifier. Internal `beat_id` is derived from this.
    pub id: String,
    /// Narrative purpose / dramatic function of the beat (free text).
    pub narrative_purpose: String,
    /// Approximate start time (seconds). Used only as an ordering hint; the
    /// adaptation phase re-derives a strictly increasing timeline.
    pub target_start_second: f32,
    pub actions: Vec<AuthoredAction>,
    pub camera_intent: AuthoredCameraIntent,
    pub completion_condition: AuthoredCompletion,
    /// Short staging/blocking description so the runtime can preserve what the
    /// beat is supposed to look like even when the action list stays lean.
    #[serde(default)]
    pub blocking: Option<String>,
    /// The most important on-screen business for this beat.
    #[serde(default)]
    pub visible_action: Option<String>,
    /// The important reaction to catch during or after the beat.
    #[serde(default)]
    pub intended_reaction: Option<String>,
    /// Why this camera setup exists (speaker, reaction, prop, payoff, etc.).
    #[serde(default)]
    pub camera_purpose: Option<String>,
    /// Playable acting direction for the beat as a whole.
    #[serde(default)]
    pub performance_intent: Option<String>,
    #[serde(default)]
    pub blocking_cues: Vec<BlockingCue>,
    #[serde(default)]
    pub environment_cues: Vec<EnvironmentCue>,
    #[serde(default)]
    pub sound_cues: Vec<SoundCue>,
    #[serde(default)]
    pub camera_cue: Option<CameraCue>,
    #[serde(default)]
    pub fallback: Option<String>,
    #[serde(default)]
    pub expected_state_changes: Vec<ExpectedStateChange>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthoredAction {
    pub actor: String,
    pub action: String,
    #[serde(default)]
    pub target: Option<String>,
    /// Required for `speak` and similar performance actions.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub intensity: Option<f32>,
    /// Playable acting direction for this action, such as "restrained" or
    /// "startled". It remains optional so cached authored episodes replay.
    #[serde(default)]
    pub performance_intent: Option<String>,
    /// Override the default estimated duration (seconds).
    #[serde(default)]
    pub duration_override: Option<f32>,
    #[serde(default)]
    pub performance: Option<PerformanceCue>,
    #[serde(default)]
    pub delivery: Option<DeliverySpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthoredCameraIntent {
    pub r#type: String,
    pub subject: String,
    #[serde(default)]
    pub reaction_subject: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AuthoredCompletion {
    pub r#type: String,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub seconds: Option<f32>,
}

impl WorldDigest {
    /// Build a compact digest for the active location + characters.
    pub fn for_episode(
        world: &WorldState,
        location_id: &str,
        characters: &[String],
    ) -> WorldDigest {
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
                mood: c
                    .emotion
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "neutral".into()),
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
