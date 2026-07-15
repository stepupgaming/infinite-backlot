//! `backlot-core`: data model, structured protocol, validation, deterministic
//! direction, and episode packaging for Infinite Backlot.
//!
//! This crate is engine-agnostic (no Bevy dependency) so it can be unit-tested
//! and reused by both the runtime and offline tooling.

pub mod asr;
pub mod author;
pub mod avatar;
pub mod config;
pub mod director;
pub mod error;
pub mod motion;
pub mod navigation;
pub mod package;
pub mod protocol;
pub mod render;
pub mod rng;
pub mod schema;
pub mod stage;
pub mod story;
pub mod timeline;
pub mod tts;
pub mod validation;
pub mod world;
pub mod world_modules;

pub use author::{AuthorSource, DeterministicAuthor, EpisodeAuthor, PlannedEpisode};
pub use avatar::{
    character_pose, part_corners, CameraTargetRole, HumanoidRig, PerformanceState, Pose, RigWorld,
    SemanticJoint, Xform,
};
pub use config::{Config, DirectorConfig, LlmConfig, RenderQuality, RuntimeConfig};
pub use director::{DeterministicDirector, Director, DirectorContext};
pub use error::{CoreError, Result};
pub use package::{
    CameraShot, Caption, Diagnostics, DialogueLine, EpisodeMetrics, EpisodePackage, GemmyManifest,
    TimedEvent,
};
pub use protocol::{
    BeatCommand, BeatOutline, CameraIntent, CentralGoal, CompletionCondition, EpisodePlan,
    ExpectedStateChange, PersistentChange, WorldDigest,
};
pub use render::{produce_episode, ProduceConfig, ProduceReport};
pub use rng::{serial_id, SeededRng};
pub use story::{apply_persistent_changes, repetition_report, WorldDelta};
pub use tts::{line_key, EspeakTts, EstimatingTts, Tts, TtsResult};
pub use validation::{
    ResolvedAction, ResolvedBeat, ValidatedPlan, ValidationError, KNOWN_BEAT_TYPES,
};
pub use world::{
    build_default_world, Character, Location, Prop, Relationship, StoryThread, WorldState,
};

/// Convenience: build the default apartment-building world.
pub fn default_world() -> WorldState {
    build_default_world()
}
