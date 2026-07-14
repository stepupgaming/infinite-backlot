//! Offline motion authoring and deterministic runtime clip preparation.

pub mod bvh;
pub mod compiler;
pub mod library;
pub mod processing;
pub mod retarget;
pub mod soma;

pub use compiler::{
    classify_transition, summarize_clip, MotionSegment, MotionSource, PoseSummary,
    TimedInteractionEvent, TransitionDecision, TransitionPolicy,
};
pub use library::{ClipApproval, MotionLibrary, MotionManifest, ProcessedMotionClip};
pub use processing::{process_clip, MotionProcessingConfig, MotionValidation};
pub use retarget::{RetargetJoint, RetargetMap};
pub use soma::{semantic_alias, SomaJoint, SOMA77};
