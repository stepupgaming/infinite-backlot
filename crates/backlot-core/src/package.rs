//! Episode package serialization.
//!
//! Per PRD §26 every episode is committed as a folder of machine-readable
//! artifacts so it can be replayed, inspected, and exported (e.g. to Gemmy).

use crate::author::PlanAuthorship;
use crate::error::{CoreError, Result};
use crate::protocol::EpisodePlan;
use crate::world::WorldState;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedEvent {
    pub t: f32,
    pub kind: String,
    pub actor: Option<String>,
    pub target: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogueLine {
    pub start: f32,
    pub end: f32,
    pub actor: String,
    pub text: String,
    pub voice_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Caption {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraShot {
    pub start: f32,
    pub end: f32,
    pub intent: String,
    pub subject: String,
    pub position: [f32; 3],
    pub look_at: [f32; 3],
}

/// Content-quality metrics (PRD §27). Stored per episode and aggregated later.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EpisodeMetrics {
    pub hook_latency_secs: f32,
    pub objective_understandable_secs: f32,
    pub dead_air_secs: f32,
    pub avg_shot_duration: f32,
    pub longest_shot_duration: f32,
    pub dialogue_to_action_ratio: f32,
    pub visual_changes_per_min: f32,
    pub story_changes_per_min: f32,
    pub failed_actions: u32,
    pub deterministic_repairs: u32,
    pub character_visibility_pct: f32,
    pub prop_visibility_pct: f32,
    pub caption_safe_pct: f32,
    pub repeated_phrases: u32,
    pub repeated_actions: u32,
    pub payoff_complete: bool,
    pub persistent_consequence: bool,
    pub continuity_violations: u32,
    pub render_defects: u32,
    pub tts_failures: u32,
    pub model_validation_failures: u32,
}

/// Truthful run diagnostics. `director` and `plan_author_source` may differ
/// from a configured label: a fallback-authored episode must never claim `llm`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Diagnostics {
    pub episode_id: String,
    pub generated_at: String,
    /// Configured/recorded director label (truthful: llm | deterministic | deterministic_fallback).
    pub director: String,
    pub llm_requests: u32,
    pub llm_failures: u32,
    pub validation_errors: Vec<String>,
    pub repairs: u32,
    pub metrics: EpisodeMetrics,
    pub issues: Vec<String>,
    // --- truthfulness + verification extensions ---
    /// Whether the run was in REQUIRE-LLM production mode.
    pub require_llm: bool,
    /// Whether ANY content actually came from the LLM.
    pub llm_used: bool,
    /// Truthful source of the plan (llm | deterministic | deterministic_fallback).
    pub plan_author_source: String,
    /// Full per-piece authorship record.
    pub authorship: Option<PlanAuthorship>,
    /// TTS provider id actually used (estimating | espeak | espeak-failed).
    pub tts_provider: String,
    /// Whether real (non-estimated) audio was produced.
    pub tts_real: bool,
    /// Whether the final mix contained real audio.
    pub audio_real: bool,
    /// Whether deterministic frames were captured.
    pub frames_captured: bool,
    /// Whether the MP4 was successfully produced.
    pub mp4_produced: bool,
    /// The exact FFmpeg command used (for reproducibility).
    pub ffmpeg_command: Option<String>,
    /// Whether ffprobe verification passed.
    pub ffprobe_ok: bool,
    /// Whether the final render issued zero LLM requests.
    pub replay_no_llm: bool,
}

/// Manifest for downstream tools (PRD §26.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GemmyManifest {
    pub title: String,
    pub summary: String,
    pub hook_text: String,
    pub duration_secs: f32,
    pub characters: Vec<String>,
    pub transcript: String,
    pub caption_style: String,
    pub render_paths: Vec<String>,
    pub thumbnail_candidates: Vec<String>,
    pub story_tags: Vec<String>,
    pub quality_scores: std::collections::HashMap<String, f32>,
    pub detected_issues: Vec<String>,
    pub canonical: bool,
    pub suggested_posting_caption: String,
    pub suggested_compilation_category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodePackage {
    pub id: String,
    pub title: String,
    pub logline: String,
    pub duration_secs: f32,
    pub canonical: bool,
    pub plan: EpisodePlan,
    pub world_before: WorldState,
    pub world_after: WorldState,
    pub events: Vec<TimedEvent>,
    pub dialogue: Vec<DialogueLine>,
    pub captions: Vec<Caption>,
    pub camera_plan: Vec<CameraShot>,
    pub metrics: EpisodeMetrics,
    pub diagnostics: Diagnostics,
    pub gemmy: GemmyManifest,
    pub report_md: String,
}

impl EpisodePackage {
    /// Write the full package to `<base>/episodes/<id>/`.
    pub fn write(&self, base_dir: &str) -> Result<()> {
        let dir = Path::new(base_dir).join("episodes").join(&self.id);
        std::fs::create_dir_all(&dir.join("output")).map_err(|source| CoreError::Io {
            path: dir.clone(),
            source,
        })?;
        std::fs::create_dir_all(&dir.join("audio")).map_err(|source| CoreError::Io {
            path: dir.clone(),
            source,
        })?;
        std::fs::create_dir_all(&dir.join("frames")).map_err(|source| CoreError::Io {
            path: dir.clone(),
            source,
        })?;

        write_json(&dir.join("episode.json"), &serde_json::json!({
            "id": self.id, "title": self.title, "logline": self.logline,
            "duration_secs": self.duration_secs, "canonical": self.canonical,
        }))?;
        write_json(&dir.join("plan.json"), &self.plan)?;
        write_json(&dir.join("world_before.json"), &self.world_before)?;
        write_json(&dir.join("world_after.json"), &self.world_after)?;
        write_jsonl(&dir.join("events.jsonl"), &self.events)?;
        write_json(&dir.join("dialogue.json"), &self.dialogue)?;
        write_json(&dir.join("captions.json"), &self.captions)?;
        write_json(&dir.join("camera_plan.json"), &self.camera_plan)?;
        write_json(&dir.join("render_manifest.json"), &serde_json::json!({
            "vertical_captioned": format!("output/vertical_captioned.mp4"),
            "vertical_clean": format!("output/vertical_clean.mp4"),
            "horizontal_clean": format!("output/horizontal_clean.mp4"),
            "thumbnail_01": format!("output/thumbnail_01.png"),
            "frames_dir": "frames",
        }))?;
        write_json(&dir.join("diagnostics.json"), &self.diagnostics)?;
        write_json(&dir.join("gemmy_manifest.json"), &self.gemmy)?;
        std::fs::write(dir.join("report.md"), &self.report_md).map_err(|source| CoreError::Io {
            path: dir.join("report.md"),
            source,
        })?;
        Ok(())
    }

    pub fn report(&self) -> String {
        self.report_md.clone()
    }

    pub fn build_report(&mut self) {
        let m = &self.metrics;
        self.report_md = format!(
            "# {title}\n\n**Logline:** {logline}\n\n\
             - **ID:** {id}\n\
             - **Director:** {dir}\n\
             - **Duration:** {dur:.1}s\n\
             - **Canonical:** {can}\n\
             - **Beats:** {beats}\n\
             - **Dialogue lines:** {dl}\n\
             - **Camera shots:** {cs}\n\n\
             ## Quality\n\
             - Hook latency: {hook:.1}s\n\
             - Objective clear by: {obj:.1}s\n\
             - Dead-air: {dead:.1}s\n\
             - Avg shot: {shot:.1}s / Longest: {long:.1}s\n\
             - Visual changes/min: {vpm:.1}\n\
             - Story changes/min: {spm:.1}\n\
             - Failed actions: {fa}\n\
             - Deterministic repairs: {rep}\n\
             - Payoff complete: {pay}\n\
             - Persistent consequence: {pc}\n\n\
             ## Issues\n{issues}\n",
            title = self.title,
            logline = self.logline,
            id = self.id,
            dir = self.diagnostics.director,
            dur = self.duration_secs,
            can = self.canonical,
            beats = self.plan.beats.len(),
            dl = self.dialogue.len(),
            cs = self.camera_plan.len(),
            hook = m.hook_latency_secs,
            obj = m.objective_understandable_secs,
            dead = m.dead_air_secs,
            shot = m.avg_shot_duration,
            long = m.longest_shot_duration,
            vpm = m.visual_changes_per_min,
            spm = m.story_changes_per_min,
            fa = m.failed_actions,
            rep = m.deterministic_repairs,
            pay = m.payoff_complete,
            pc = m.persistent_consequence,
            issues = if self.diagnostics.issues.is_empty() {
                "  - none".into()
            } else {
                self.diagnostics
                    .issues
                    .iter()
                    .map(|i| format!("  - {i}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            },
        );
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let s = serde_json::to_string_pretty(value)?;
    std::fs::write(path, s).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn write_jsonl<T: Serialize>(path: &Path, items: &[T]) -> Result<()> {
    let mut buf = String::new();
    for it in items {
        buf.push_str(&serde_json::to_string(it)?);
        buf.push('\n');
    }
    std::fs::write(path, buf).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// Convenience constructor producing an empty-but-valid package shell.
pub fn empty_package(id: &str, plan: &EpisodePlan, world: &WorldState) -> EpisodePackage {
    EpisodePackage {
        id: id.into(),
        title: plan.episode_title.clone(),
        logline: plan.logline.clone(),
        duration_secs: plan.target_duration_seconds,
        canonical: false,
        plan: plan.clone(),
        world_before: world.clone(),
        world_after: world.clone(),
        events: vec![],
        dialogue: vec![],
        captions: vec![],
        camera_plan: vec![],
        metrics: EpisodeMetrics::default(),
        diagnostics: Diagnostics {
            episode_id: id.into(),
            generated_at: Utc::now().to_rfc3339(),
            director: "unknown".into(),
            llm_requests: 0,
            llm_failures: 0,
            validation_errors: vec![],
            repairs: 0,
            metrics: EpisodeMetrics::default(),
            issues: vec![],
            ..Default::default()
        },
        gemmy: GemmyManifest {
            title: plan.episode_title.clone(),
            summary: plan.logline.clone(),
            hook_text: String::new(),
            duration_secs: plan.target_duration_seconds,
            characters: plan.active_characters.clone(),
            transcript: String::new(),
            caption_style: "backlot-default".into(),
            render_paths: vec![],
            thumbnail_candidates: vec![],
            story_tags: plan.tone.clone(),
            quality_scores: Default::default(),
            detected_issues: vec![],
            canonical: false,
            suggested_posting_caption: String::new(),
            suggested_compilation_category: "shorts".into(),
        },
        report_md: String::new(),
    }
}
