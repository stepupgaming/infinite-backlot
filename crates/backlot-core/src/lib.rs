//! `backlot-core`: data model, structured protocol, validation, deterministic
//! direction, and episode packaging for Infinite Backlot.
//!
//! This crate is engine-agnostic (no Bevy dependency) so it can be unit-tested
//! and reused by both the runtime and offline tooling.

pub mod author;
pub mod avatar;
pub mod config;
pub mod timeline;
pub mod director;
pub mod error;
pub mod package;
pub mod protocol;
pub mod render;
pub mod rng;
pub mod schema;
pub mod story;
pub mod tts;
pub mod validation;
pub mod world;

pub use avatar::{
    character_pose, part_corners, CameraTargetRole, HumanoidRig, PerformanceState, Pose,
    RigWorld, SemanticJoint, Xform,
};
pub use config::{Config, DirectorConfig, LlmConfig, RuntimeConfig};
pub use author::{AuthorSource, DeterministicAuthor, EpisodeAuthor, PlannedEpisode};
pub use director::{DeterministicDirector, Director, DirectorContext};
pub use error::{CoreError, Result};
pub use package::{
    Caption, CameraShot, Diagnostics, DialogueLine, EpisodeMetrics, EpisodePackage, GemmyManifest,
    TimedEvent,
};
pub use protocol::{
    BeatCommand, BeatOutline, CameraIntent, CentralGoal, CompletionCondition, EpisodePlan,
    ExpectedStateChange, PersistentChange, WorldDigest,
};
pub use render::{ProduceConfig, ProduceReport, produce_episode};
pub use rng::{serial_id, SeededRng};
pub use story::{apply_persistent_changes, repetition_report, WorldDelta};
pub use tts::{line_key, EspeakTts, EstimatingTts, Tts, TtsResult};
pub use validation::{ResolvedAction, ResolvedBeat, ValidatedPlan, ValidationError, KNOWN_BEAT_TYPES};
pub use world::{build_default_world, Character, Location, Prop, Relationship, StoryThread, WorldState};

/// Convenience: build the default apartment-building world.
pub fn default_world() -> WorldState {
    build_default_world()
}
