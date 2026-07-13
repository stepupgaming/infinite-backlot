//! Application state machine, shared components and resources.

use backlot_core::author::{PlanAuthorship, PlannedEpisode};
use backlot_core::config::Config;
use backlot_core::package::{CameraShot, Caption, DialogueLine, EpisodeMetrics, TimedEvent};
use backlot_core::validation::ValidatedPlan;
use backlot_core::world::WorldState;
use backlot_llm::LlmMetrics;
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};

/// Explicit application states (PRD §13).
#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Boot,
    AssetLoading,
    Idle,
    EpisodeSelecting,
    EpisodePlanning,
    PlanValidation,
    Rehearsing,
    Repairing,
    EpisodeReady,
    Rendering,
    Reviewing,
    Committing,
    ErrorRecovery,
    Shutdown,
}

// ---------- Scene components ----------

#[derive(Component)]
pub struct CharacterAvatar {
    pub id: String,
    pub display: String,
    pub color: Color,
    pub speed: f32,
    pub nav_target: Option<Vec3>,
    pub speaking_until: f32,
    pub emote: String,
}

#[derive(Component)]
pub struct PropMarker {
    pub id: String,
}

#[derive(Component)]
pub struct FlickerLight {
    pub base_intensity: f32,
    pub active: bool,
    pub phase: f32,
}

#[derive(Component)]
pub struct MainCamera;

#[derive(Component)]
pub struct SpeechIndicator;

/// Camera rig state: the director sets desired transform; the system eases toward it.
#[derive(Component)]
pub struct CameraRig {
    pub intent: String,
    pub desired_pos: Vec3,
    pub desired_look: Vec3,
    pub current_look: Vec3,
    pub anchors: Vec<(Vec3, Vec3)>,
}

/// Index of scene entity positions for navigation + camera targeting.
#[derive(Resource, Default)]
pub struct SceneIndex {
    pub characters: HashMap<String, Entity>,
    pub props: HashMap<String, Entity>,
    pub marks: HashMap<String, Vec3>,
    pub anchors: Vec<(Vec3, Vec3)>,
}

// ---------- Author worker communication ----------

pub struct AuthorMsg {
    pub planned: Result<PlannedEpisode, String>,
    pub auth: Option<PlanAuthorship>,
    pub metrics: Option<LlmMetrics>,
}

#[derive(Resource)]
pub struct AuthorHandle {
    pub tx: mpsc::Sender<DirectorContextMsg>,
    pub rx: Arc<Mutex<mpsc::Receiver<AuthorMsg>>>,
    pub pending: bool,
    pub metrics: Option<Arc<Mutex<LlmMetrics>>>,
    pub using_llm: bool,
}

/// A request envelope carrying the director context.
pub struct DirectorContextMsg {
    pub world: WorldState,
    pub episode_number: u64,
    pub seed: u64,
    pub target_duration: f32,
    pub recent_summaries: Vec<String>,
    pub tone: Vec<String>,
}

impl DirectorContextMsg {
    pub fn to_context(&self) -> backlot_core::director::DirectorContext {
        backlot_core::director::DirectorContext {
            world: self.world.clone(),
            episode_number: self.episode_number,
            seed: self.seed,
            target_duration: self.target_duration,
            recent_summaries: self.recent_summaries.clone(),
            tone: self.tone.clone(),
        }
    }
}

// ---------- Current episode + player state ----------

#[derive(Resource, Default)]
pub struct CurrentEpisode {
    pub planned: Option<PlannedEpisode>,
    pub validated: Option<ValidatedPlan>,
    pub auth: Option<PlanAuthorship>,
    pub world_before: Option<WorldState>,
    pub world_after: Option<WorldState>,
    pub episode_id: String,
    pub episode_number: u64,
    pub approved: bool,
}

#[derive(Resource, Default)]
pub struct Player {
    pub active: bool,
    pub render_pass: bool,
    pub beat_index: usize,
    pub beat_elapsed: f32,
    pub action_cursor: usize,
    pub action_fired: Vec<bool>,
    pub schedule: Vec<(f32, backlot_core::validation::ResolvedAction)>,
    pub beat_duration: f32,
    pub initialized_beat: Option<usize>,
    pub finished: bool,
    pub since_event: f32,
    pub quality: f32,
}

/// Current on-screen caption (rendered to console + a caption bar).
#[derive(Resource, Default)]
pub struct ActiveCaption {
    pub text: String,
    pub speaker: String,
    pub until: f32,
    pub active: bool,
}

#[derive(Resource, Default)]
pub struct EpisodeClock {
    pub elapsed: f32,
    pub scale: f32,
}

#[derive(Resource, Default)]
pub struct RehearsalLog {
    pub events: Vec<TimedEvent>,
    pub dialogue: Vec<DialogueLine>,
    pub captions: Vec<Caption>,
    pub camera: Vec<CameraShot>,
    pub hook_time: Option<f32>,
    pub objective_time: Option<f32>,
    pub dead_air_max: f32,
    pub visual_changes: u32,
    pub story_changes: u32,
    pub repairs: u32,
    pub validation_errors: Vec<String>,
}

#[derive(Resource)]
pub struct RunControl {
    pub config: Config,
    pub episodes_to_run: u32,
    pub episodes_done: u32,
    pub last_summary: Option<String>,
    pub recent_summaries: Vec<String>,
    pub auto: bool,
    pub replaying: bool,
}

impl From<&Config> for RunControl {
    fn from(config: &Config) -> Self {
        Self {
            config: config.clone(),
            episodes_to_run: config.runtime.episodes_to_run,
            episodes_done: 0,
            last_summary: None,
            recent_summaries: vec![],
            auto: config.runtime.episodes_to_run > 0,
            replaying: false,
        }
    }
}

/// Clone of the canonical (persistent) world; mutated on commit.
#[derive(Resource)]
pub struct CanonicalWorld(pub WorldState);

/// Shared episode metrics accumulator for the current episode.
#[derive(Resource, Default)]
pub struct CurrentMetrics(pub EpisodeMetrics);
