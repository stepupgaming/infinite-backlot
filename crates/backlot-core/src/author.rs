//! Unified authoring contract.
//!
//! Both the built-in deterministic director and the LLM-backed director
//! implement `EpisodeAuthor`, producing a fully-resolved `PlannedEpisode`
//! (plan + per-beat commands) **plus** a truthful `PlanAuthorship` record that
//! states, for the plan and for every beat, which implementation actually
//! supplied the result. This is what prevents a fallback-authored episode from
//! being mislabeled as LLM-authored in the diagnostics.

use crate::director::{DeterministicDirector, Director, DirectorContext};
use crate::error::Result;
use crate::protocol::{BeatCommand, EpisodePlan};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PlannedEpisode {
    pub plan: EpisodePlan,
    /// Beat id -> detailed command.
    pub commands: HashMap<String, BeatCommand>,
}

/// Which implementation actually produced a piece of content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorSource {
    /// The configured OpenAI-compatible LLM produced this piece.
    Llm,
    /// The deterministic director produced this piece because the LLM failed
    /// (a silent fallback). Never claim `Llm` when this is the source.
    DeterministicFallback,
    /// The run was configured to use the deterministic director only.
    Deterministic,
}

impl AuthorSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthorSource::Llm => "llm",
            AuthorSource::DeterministicFallback => "deterministic_fallback",
            AuthorSource::Deterministic => "deterministic",
        }
    }
}

/// Per-beat authorship record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatAuthorship {
    pub beat_id: String,
    pub source: AuthorSource,
    pub model: String,
    pub attempts: u32,
    pub latency_ms: f32,
    pub repair_used: bool,
    pub validation_status: String,
}

/// Whole-plan authorship record (truthful source for plan + every beat).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanAuthorship {
    pub plan_source: AuthorSource,
    pub model: String,
    pub attempts: u32,
    pub latency_ms: f32,
    pub repair_used: bool,
    pub validation_status: String,
    pub beats: Vec<BeatAuthorship>,
}

impl PlanAuthorship {
    /// True only if BOTH the plan and every beat came from the LLM.
    pub fn all_llm(&self) -> bool {
        self.plan_source == AuthorSource::Llm
            && self.beats.iter().all(|b| b.source == AuthorSource::Llm)
    }
}

pub trait EpisodeAuthor: Send + Sync {
    fn name(&self) -> &'static str;
    /// Author a complete episode (plan + every beat command) for `ctx`, returning
    /// the plan together with a truthful authorship record.
    fn author(&self, ctx: &DirectorContext) -> Result<(PlannedEpisode, PlanAuthorship)>;
}

/// Wraps the deterministic director as an `EpisodeAuthor`.
pub struct DeterministicAuthor;

fn deterministic_beat_auth(beat_id: &str) -> BeatAuthorship {
    BeatAuthorship {
        beat_id: beat_id.into(),
        source: AuthorSource::Deterministic,
        model: "deterministic".into(),
        attempts: 1,
        latency_ms: 0.0,
        repair_used: false,
        validation_status: "ok".into(),
    }
}

impl EpisodeAuthor for DeterministicAuthor {
    fn name(&self) -> &'static str {
        "deterministic"
    }

    fn author(&self, ctx: &DirectorContext) -> Result<(PlannedEpisode, PlanAuthorship)> {
        let d = DeterministicDirector;
        let plan = d.plan_episode(ctx)?;
        let mut commands = HashMap::new();
        let mut beats = Vec::new();
        for b in &plan.beats {
            let cmd = d.plan_beat(ctx, &plan, b)?;
            commands.insert(b.id.clone(), cmd);
            beats.push(deterministic_beat_auth(&b.id));
        }
        let auth = PlanAuthorship {
            plan_source: AuthorSource::Deterministic,
            model: "deterministic".into(),
            attempts: 1,
            latency_ms: 0.0,
            repair_used: false,
            validation_status: "ok".into(),
            beats,
        };
        Ok((PlannedEpisode { plan, commands }, auth))
    }
}
