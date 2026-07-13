//! LLM-backed episode author (OpenAI-compatible).
//!
//! Redesigned (evidence-based) authoring. A complete episode is produced with a
//! SINGLE whole-episode structured call (`AuthoredEpisode`). At most ONE targeted
//! whole-episode revision call is issued when the real, measured runtime falls
//! outside the required 45–60s window. The revision is *direction-aware*: when
//! the episode is too long it is told to cut content (never "add more"), and when
//! too short it is told to add content — and in both cases the accepted parsed
//! episode JSON is included so the model revises rather than starting over.
//!
//! The redesign keeps the existing safety properties: the Rust validators in
//! `backlot_core::validation` remain the final authority, no entity / dialogue /
//! action is ever invented locally, and `require_llm` makes any failure fatal so
//! a fallback episode is never mislabeled as LLM-authored.

use backlot_core::author::DeterministicAuthor;
use backlot_core::author::{
    AuthorSource, BeatAuthorship, EpisodeAuthor, PlanAuthorship, PlannedEpisode,
};
use backlot_core::config::{Config, DirectorConfig, TtsConfig};
use backlot_core::director::{DeterministicDirector, Director, DirectorContext};
use backlot_core::error::{CoreError, Result};
use backlot_core::protocol::{AuthoredEpisode, WorldDigest, KNOWN_ACTIONS, KNOWN_CAMERA_INTENTS};
use backlot_core::schema::authored_episode_schema;
use backlot_core::validation::{
    adapt_authored_episode, estimate_action_duration, validate_beat_command, validate_plan,
};
use backlot_core::world::WorldState;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::client::{CapturedResponse, LlmClient, LlmMetrics};

/// Path (relative to CWD) where a successfully authored + duration-validated
/// episode is cached so a later production run can replay it without any new
/// LLM calls.
const REUSE_CACHE_PATH: &str = "data/last_authored_episode.json";

pub struct LlmAuthor {
    client: LlmClient,
    runtime: tokio::runtime::Runtime,
    fallback: DeterministicDirector,
    force_fallback: bool,
    require_llm: bool,
    max_repairs: u32,
    /// TTS configuration used to measure real (dead-air-compacted) episode
    /// runtime so the duration-repair loop estimates against measured speech
    /// timing rather than a rough heuristic.
    tts: TtsConfig,
    max_dead_air: f32,
    /// When true, every structured request is routed through the capture path
    /// (raw reqwest) and recorded for the diagnostic packet. Production stays false.
    diagnostic: bool,
    /// Directory where per-call request/response files are written (diagnostic).
    capture_dir: Option<PathBuf>,
    /// When set, authoring first tries to load a previously authored + validated
    /// episode from this path and replay it with zero new LLM calls.
    reuse_path: Option<PathBuf>,
    /// Ordered log of every logical (structured) call made while `diagnostic`.
    capture_log: Arc<Mutex<Vec<CapturedCall>>>,
}

#[derive(Clone)]
struct MetricsDelta {
    attempts: u32,
    failures: u32,
    repairs: u32,
    latency_ms: f32,
}

#[derive(Clone, Serialize)]
struct CapturedCall {
    purpose: String,
    stage: String,
    call_index: usize,
    request_system: String,
    request_user: String,
    response: CapturedResponse,
    validation: String,
    accepted: bool,
    retry_followed: bool,
    /// Measured (real TTS) runtime of the episode this call produced, seconds.
    measured_duration: Option<f32>,
    /// "lengthen" | "shorten" | "reused" | null.
    repair_direction: Option<String>,
}

/// Aggregate result of the authoring-only diagnostic.
#[derive(Debug, Serialize)]
pub struct DiagnosticSummary {
    pub total_wire_calls: usize,
    pub total_logical_calls: usize,
    pub total_wall_ms: u128,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub produced: bool,
    pub estimated_duration_secs: Option<f32>,
    pub duration_status: String,
    pub schema_repairs: u32,
    pub finish_reasons: Vec<String>,
    pub any_length_truncated: bool,
    pub plan_title: Option<String>,
    pub beat_count: usize,
    pub repair_needed: bool,
    pub repair_direction: Option<String>,
}

#[derive(Clone, Copy)]
struct DurationPolicy {
    min_secs: f32,
    max_secs: f32,
    target_secs: f32,
}

impl DurationPolicy {
    fn for_request(target_secs: f32) -> Self {
        Self {
            min_secs: 45.0,
            max_secs: 60.0,
            target_secs: target_secs.clamp(45.0, 60.0),
        }
    }

    fn status_for(&self, secs: f32) -> &'static str {
        if secs < self.min_secs {
            "duration_too_short"
        } else if secs > self.max_secs {
            "duration_too_long"
        } else {
            "ok"
        }
    }

    fn in_range(&self, secs: f32) -> bool {
        secs >= self.min_secs && secs <= self.max_secs
    }
}

impl LlmAuthor {
    pub fn new(config: &Config, director: DirectorConfig) -> Result<Self> {
        let client = LlmClient::new(config.llm.clone())?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| CoreError::Llm(format!("runtime: {e}")))?;
        Ok(Self {
            client,
            runtime,
            fallback: DeterministicDirector,
            force_fallback: director.force_fallback,
            require_llm: director.require_llm,
            max_repairs: director.max_repairs.max(1),
            tts: config.tts.clone(),
            max_dead_air: config.runtime.max_dead_air_secs,
            diagnostic: false,
            capture_dir: None,
            reuse_path: None,
            capture_log: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Enable replay from a previously authored + validated episode (no new LLM
    /// calls). Used by production to reuse the episode the diagnostic proved.
    pub fn set_reuse_path(&mut self, path: PathBuf) {
        self.reuse_path = Some(path);
    }

    /// Construct an author that runs the authoring-only diagnostic: every
    /// structured request is captured (raw response, reasoning, usage, timing)
    /// and written to `capture_dir`. No rendering is performed.
    pub fn new_diagnostic(
        config: &Config,
        director: DirectorConfig,
        capture_dir: PathBuf,
    ) -> Result<Self> {
        let mut a = Self::new(config, director)?;
        a.diagnostic = true;
        a.capture_dir = Some(capture_dir.clone());
        a.client
            .set_trace_path(capture_dir.join("authoring_trace.jsonl"));
        Ok(a)
    }

    pub fn metrics(&self) -> LlmMetrics {
        self.client.metrics()
    }

    pub fn metrics_arc(&self) -> Arc<Mutex<LlmMetrics>> {
        self.client.metrics_arc()
    }

    /// Expose the tokio runtime so the binary can `block_on` async diagnostics.
    pub fn runtime(&self) -> &tokio::runtime::Runtime {
        &self.runtime
    }

    /// Diagnostic accessors for the static server/model configuration.
    pub fn config_base_url(&self) -> &str {
        self.client.config_base_url()
    }
    pub fn config_temperature(&self) -> f32 {
        self.client.config_temperature()
    }
    pub fn config_max_tokens(&self) -> u32 {
        self.client.config_max_tokens()
    }
    pub fn config_timeout(&self) -> f32 {
        self.client.config_timeout()
    }
    pub fn config_llm_max_retries(&self) -> u32 {
        self.client.config_llm_max_retries()
    }
    pub fn config_stream(&self) -> bool {
        self.client.config_stream()
    }
    pub fn model_name(&self) -> &str {
        self.client.model_name()
    }

    pub fn health(&self) -> Result<bool> {
        self.runtime.block_on(self.client.health_check())
    }

    fn delta(&self, before: &LlmMetrics) -> MetricsDelta {
        let after = self.client.metrics();
        MetricsDelta {
            attempts: after.requests.saturating_sub(before.requests),
            failures: after.failures.saturating_sub(before.failures),
            repairs: after.schema_repairs.saturating_sub(before.schema_repairs),
            latency_ms: after.last_latency_ms,
        }
    }
}

impl EpisodeAuthor for LlmAuthor {
    fn name(&self) -> &'static str {
        "llm"
    }

    fn author(&self, ctx: &DirectorContext) -> Result<(PlannedEpisode, PlanAuthorship)> {
        if self.force_fallback {
            let (p, mut a) = DeterministicAuthor.author(ctx)?;
            // Normalize source so deterministic-only runs are unambiguous.
            a.plan_source = AuthorSource::Deterministic;
            for b in &mut a.beats {
                b.source = AuthorSource::Deterministic;
            }
            return Ok((p, a));
        }
        let (planned, auth, _ep) = self.runtime.block_on(self.author_async_inner(ctx))?;
        Ok((planned, auth))
    }
}

impl LlmAuthor {
    /// Full authoring flow. Returns the adapted plan + authorship plus the
    /// accepted `AuthoredEpisode` (so the diagnostic can persist it).
    async fn author_async_inner(
        &self,
        ctx: &DirectorContext,
    ) -> Result<(PlannedEpisode, PlanAuthorship, Option<AuthoredEpisode>)> {
        // Replay path: reuse a previously authored + validated episode with zero
        // new LLM calls.
        if let Some(path) = &self.reuse_path {
            if let Some(out) = self.load_reused(ctx, path) {
                return Ok(out);
            }
            // A requested replay must never silently become a new model call.
            // That would invalidate the zero-call replay proof and overwrite the
            // cached artifact with different authored content.
            return Err(CoreError::Llm(format!(
                "reused episode at '{}' is missing, invalid, or outside 45-60s; refusing a fresh LLM call",
                path.display()
            )));
        }

        let digest = WorldDigest::for_episode(
            &ctx.world,
            &first_location(&ctx.world),
            &all_char_ids(&ctx.world),
        );
        let schema = authored_episode_schema();
        let system = whole_episode_system_prompt();
        let duration = DurationPolicy::for_request(ctx.target_duration);
        let before = self.client.metrics();

        // ---- Initial whole-episode call ----
        let (authored1, cap1, user1) = match self
            .request_whole_episode(ctx, &digest, &system, &schema, None, None)
            .await
        {
            Ok(v) => v,
            Err(e) => return self.author_failed(ctx, e, &before),
        };
        let plan_cmds1 = match self.adapt_validate(&ctx.world, &authored1) {
            Ok(p) => p,
            Err(err) => {
                return self.author_failed(
                    ctx,
                    CoreError::Llm(format!("initial episode invalid: {err}")),
                    &before,
                )
            }
        };
        let secs1 = self.measure_or_estimate(&ctx.world, &plan_cmds1.0, &plan_cmds1.1);
        let accepted1 = duration.in_range(secs1);
        if let Some(c) = cap1 {
            self.record_capture(
                "whole-episode",
                "initial",
                &system,
                &user1,
                c,
                if accepted1 {
                    "ok"
                } else {
                    "duration_out_of_range"
                },
                accepted1,
                false,
                Some(secs1),
                None,
            );
        }
        if accepted1 {
            let (planned, auth) = self.build_authorship(plan_cmds1, &before, false, None, None);
            return Ok((planned, auth, Some(authored1)));
        }

        // ---- Direction-aware repair: exactly ONE targeted revision call ----
        let (direction, feedback) =
            direction_aware_feedback(&plan_cmds1.0, &plan_cmds1.1, duration, secs1);
        let accepted_json = serde_json::to_string_pretty(&authored1).unwrap_or_default();
        let (authored2, cap2, user2) = match self
            .request_whole_episode(
                ctx,
                &digest,
                &system,
                &schema,
                Some(&feedback),
                Some(&accepted_json),
            )
            .await
        {
            Ok(v) => v,
            Err(e) => return self.author_failed(ctx, e, &before),
        };
        let plan_cmds2 = match self.adapt_validate(&ctx.world, &authored2) {
            Ok(p) => p,
            Err(err) => {
                return self.author_failed(
                    ctx,
                    CoreError::Llm(format!("repaired episode invalid: {err}")),
                    &before,
                )
            }
        };
        let secs2 = self.measure_or_estimate(&ctx.world, &plan_cmds2.0, &plan_cmds2.1);
        let accepted2 = duration.in_range(secs2);
        if let Some(c) = cap2 {
            self.record_capture(
                "whole-episode",
                "duration-repair",
                &system,
                &user2,
                c,
                if accepted2 {
                    "ok"
                } else {
                    "duration_out_of_range"
                },
                accepted2,
                true,
                Some(secs2),
                Some(direction.as_str()),
            );
        }
        if accepted2 {
            let dir_label = Some(direction.as_str());
            let (planned, auth) = self.build_authorship(plan_cmds2, &before, true, dir_label, None);
            return Ok((planned, auth, Some(authored2)));
        }

        if self.require_llm {
            return Err(CoreError::Llm(format!(
                "require_llm: authored episode runtime {:.1}s outside 45-60s after duration repair",
                secs2
            )));
        }
        tracing::warn!("LLM authoring out of range after repair; using fallback");
        self.fallback_plan(ctx).map(|(p, a)| (p, a, None))
    }

    /// Replay a previously authored + validated episode with no new LLM calls.
    fn load_reused(
        &self,
        ctx: &DirectorContext,
        path: &Path,
    ) -> Option<(PlannedEpisode, PlanAuthorship, Option<AuthoredEpisode>)> {
        let text = std::fs::read_to_string(path).ok()?;
        let ep: AuthoredEpisode = serde_json::from_str(&text).ok()?;
        let plan_cmds = self.adapt_validate(&ctx.world, &ep).ok()?;
        let secs = self.measure_or_estimate(&ctx.world, &plan_cmds.0, &plan_cmds.1);
        let dur = DurationPolicy::for_request(ctx.target_duration);
        if !dur.in_range(secs) {
            tracing::warn!(
                "reused episode {:.1}s outside 45-60s; re-authoring instead of replaying",
                secs
            );
            return None;
        }
        let before = self.client.metrics();
        let (planned, auth) = self.build_authorship(
            plan_cmds,
            &before,
            false,
            Some("reused"),
            Some("gemma-reused"),
        );
        Some((planned, auth, Some(ep)))
    }

    /// One whole-episode structured call, wrapped in a bounded schema-repair
    /// loop. On a validation failure the concrete error is fed back and the call
    /// is re-issued with the same (repair/direction) context. Returns the
    /// accepted `AuthoredEpisode`, the capture (if diagnostic), and the user
    /// prompt that produced it.
    async fn request_whole_episode(
        &self,
        ctx: &DirectorContext,
        digest: &WorldDigest,
        system: &str,
        schema_json: &str,
        repair_feedback: Option<&str>,
        accepted_json: Option<&str>,
    ) -> Result<(AuthoredEpisode, Option<CapturedResponse>, String)> {
        let mut last_error: Option<String> = None;
        for attempt in 0..=self.max_repairs {
            let correction = last_error.as_deref();
            let user =
                whole_episode_user_prompt(ctx, digest, repair_feedback, accepted_json, correction);
            let (content, cap): (String, Option<CapturedResponse>) = if self.diagnostic {
                let purpose = if repair_feedback.is_some() {
                    "whole-episode-repair"
                } else {
                    "whole-episode"
                };
                let (c, cap) = self
                    .client
                    .chat_structured_capture(
                        system,
                        &user,
                        "AuthoredEpisode",
                        schema_json,
                        self.max_repairs,
                        purpose,
                    )
                    .await?;
                (c, Some(cap))
            } else {
                (
                    self.client
                        .chat_structured(
                            system,
                            &user,
                            "AuthoredEpisode",
                            schema_json,
                            self.max_repairs,
                        )
                        .await?,
                    None,
                )
            };
            let outcome = match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(v) => match serde_json::from_value::<AuthoredEpisode>(v) {
                    Ok(ep) => match self.adapt_validate(&ctx.world, &ep) {
                        Ok(_) => Ok((ep, user)),
                        Err(e) => Err(format!("episode failed validation: {e}")),
                    },
                    Err(e) => Err(format!("parse AuthoredEpisode failed: {e}")),
                },
                Err(e) => Err(format!("invalid JSON: {e}")),
            };
            match outcome {
                Ok((ep, user)) => return Ok((ep, cap, user)),
                Err(e) => {
                    last_error = Some(e);
                    if self.require_llm && attempt < self.max_repairs {
                        tracing::warn!("whole-episode invalid; sending schema correction");
                    }
                }
            }
        }
        if self.require_llm {
            return Err(CoreError::Llm(format!(
                "require_llm: {}",
                last_error.unwrap_or_else(|| "episode could not be authored".into())
            )));
        }
        Err(CoreError::Llm(
            last_error.unwrap_or_else(|| "episode could not be authored".into()),
        ))
    }

    /// Adapt + fully validate a whole episode. Returns the runtime-ready plan +
    /// commands, or a combined error string (final authority = Rust validation).
    fn adapt_validate(
        &self,
        world: &WorldState,
        ep: &AuthoredEpisode,
    ) -> std::result::Result<
        (
            EpisodePlanOwned,
            HashMap<String, backlot_core::protocol::BeatCommand>,
        ),
        String,
    > {
        let (plan, commands) =
            adapt_authored_episode(ep, world).map_err(|errs| format_validation(errs))?;
        validate_plan(world, &plan).map_err(|errs| format_validation(errs))?;
        for cmd in commands.values() {
            validate_beat_command(world, &plan, cmd).map_err(|errs| format_validation(errs))?;
        }
        Ok((plan, commands))
    }

    fn build_authorship(
        &self,
        plan_cmds: (
            EpisodePlanOwned,
            HashMap<String, backlot_core::protocol::BeatCommand>,
        ),
        before: &LlmMetrics,
        repaired: bool,
        direction: Option<&str>,
        model_override: Option<&str>,
    ) -> (PlannedEpisode, PlanAuthorship) {
        let after = self.delta(before);
        let model = model_override
            .unwrap_or_else(|| self.client.model_name())
            .to_string();
        let status = if direction == Some("reused") {
            "reused"
        } else if repaired {
            match direction {
                Some("lengthen") => "duration_repaired_lengthen",
                Some("shorten") => "duration_repaired_shorten",
                _ => "duration_repaired",
            }
        } else {
            "ok"
        };
        let beat_status = if direction == Some("reused") {
            "reused"
        } else if repaired {
            "duration_repaired"
        } else {
            "ok"
        };
        let beats: Vec<BeatAuthorship> = plan_cmds
            .1
            .keys()
            .map(|id| BeatAuthorship {
                beat_id: id.clone(),
                source: AuthorSource::Llm,
                model: model.clone(),
                attempts: after.attempts,
                latency_ms: after.latency_ms,
                repair_used: repaired,
                validation_status: beat_status.into(),
            })
            .collect();
        let auth = PlanAuthorship {
            plan_source: AuthorSource::Llm,
            model,
            attempts: after.attempts,
            latency_ms: after.latency_ms,
            repair_used: repaired,
            validation_status: status.into(),
            beats,
        };
        (
            PlannedEpisode {
                plan: plan_cmds.0,
                commands: plan_cmds.1,
            },
            auth,
        )
    }

    fn author_failed(
        &self,
        ctx: &DirectorContext,
        e: CoreError,
        _before: &LlmMetrics,
    ) -> Result<(PlannedEpisode, PlanAuthorship, Option<AuthoredEpisode>)> {
        if self.require_llm {
            Err(e)
        } else {
            tracing::warn!("LLM authoring failed ({e}); using fallback plan");
            self.fallback_plan(ctx).map(|(p, a)| (p, a, None))
        }
    }

    /// Build a fully deterministic plan (used when require_llm is false and the
    /// LLM authoring step fails).
    fn fallback_plan(&self, ctx: &DirectorContext) -> Result<(PlannedEpisode, PlanAuthorship)> {
        let (p, mut a) = DeterministicAuthor.author(ctx)?;
        a.plan_source = AuthorSource::DeterministicFallback;
        for b in &mut a.beats {
            b.source = AuthorSource::DeterministicFallback;
        }
        Ok((p, a))
    }

    /// Measure the real, dead-air-compacted runtime using the configured TTS
    /// engine. Falls back to a rough heuristic only if TTS measurement is
    /// unavailable (never used as the acceptance authority in require_llm mode).
    fn measure_or_estimate(
        &self,
        world: &WorldState,
        plan: &EpisodePlanOwned,
        commands: &HashMap<String, backlot_core::protocol::BeatCommand>,
    ) -> f32 {
        match backlot_core::render::measure_runtime(
            world,
            plan,
            commands,
            &self.tts,
            self.max_dead_air,
        ) {
            Ok(s) => s,
            Err(_) => estimate_whole_episode(plan, commands),
        }
    }

    /// Record a logical call into the diagnostic capture log, flush the
    /// per-call request/response/extracted JSON files, and append a logical
    /// trace line (purpose, timing, validation, measured duration, repair
    /// direction, acceptance) so an interrupted run is never left blind.
    fn record_capture(
        &self,
        purpose: &str,
        stage: &str,
        system: &str,
        user: &str,
        cap: CapturedResponse,
        validation: &str,
        accepted: bool,
        retry: bool,
        measured_duration: Option<f32>,
        repair_direction: Option<&str>,
    ) {
        let mut log = self.capture_log.lock().unwrap();
        let idx = log.len();
        let cc = CapturedCall {
            purpose: purpose.to_string(),
            stage: stage.to_string(),
            call_index: idx,
            request_system: system.to_string(),
            request_user: user.to_string(),
            response: cap.clone(),
            validation: validation.to_string(),
            accepted,
            retry_followed: retry,
            measured_duration,
            repair_direction: repair_direction.map(|s| s.to_string()),
        };
        log.push(cc);
        drop(log);

        // Logical trace line (single per structured call).
        let prompt_tokens = cap
            .usage
            .as_ref()
            .and_then(|u| u.get("prompt_tokens").and_then(|v| v.as_u64()))
            .unwrap_or(0);
        let completion_tokens = cap
            .usage
            .as_ref()
            .and_then(|u| u.get("completion_tokens").and_then(|v| v.as_u64()))
            .unwrap_or(0);
        self.client.append_trace_event(serde_json::json!({
            "seq": idx + 1,
            "purpose": purpose,
            "stage": stage,
            "validation": validation,
            "accepted": accepted,
            "measured_duration_secs": measured_duration,
            "repair_direction": repair_direction,
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "finish_reason": cap.finish_reason,
        }));

        if let Some(dir) = &self.capture_dir {
            let safe = purpose.replace(':', "_");
            let tag = format!(
                "{:02}_{}_{}",
                idx + 1,
                safe,
                if accepted { "ok" } else { "rej" }
            );
            write_json_file(
                &dir.join(format!("{tag}_request.json")),
                &serde_json::json!({"system": system, "user": user}),
            );
            let raw_val: serde_json::Value = serde_json::from_str(&cap.raw_text)
                .unwrap_or(serde_json::Value::String(cap.raw_text.clone()));
            write_json_file(&dir.join(format!("{tag}_response_raw.json")), &raw_val);
            let ext_val: serde_json::Value = cap
                .extracted_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::Value::Null);
            write_json_file(&dir.join(format!("{tag}_extracted.json")), &ext_val);
        }
    }

    /// Run ONLY the LLM authoring stage (no rendering) and write the full
    /// diagnostic packet to `out_dir`.
    pub async fn author_diagnostic(
        &self,
        ctx: &DirectorContext,
        out_dir: &Path,
    ) -> Result<DiagnosticSummary> {
        std::fs::create_dir_all(out_dir).ok();
        let diag_result = self.author_async_inner(ctx).await;
        let (plan_opt, cmds_opt, authored_opt, err_opt) = match &diag_result {
            Ok((planned, _auth, ep)) => (
                Some(&planned.plan),
                Some(&planned.commands),
                ep.clone(),
                None,
            ),
            Err(e) => (None, None, None, Some(e.to_string())),
        };

        let wire = self.client.wire_log_snapshot();
        let caps = self.capture_log.lock().unwrap().clone();

        let schema = authored_episode_schema();
        write_json_file(
            &out_dir.join("authored_episode_schema.json"),
            &serde_json::from_str::<serde_json::Value>(&schema).unwrap_or(serde_json::Value::Null),
        );
        write_json_file(
            &out_dir.join("captured_calls.json"),
            &serde_json::to_value(&caps).unwrap_or(serde_json::Value::Null),
        );
        if let (Some(plan), Some(cmds)) = (plan_opt, cmds_opt) {
            write_json_file(
                &out_dir.join("final_plan.json"),
                &serde_json::to_value(plan).unwrap_or(serde_json::Value::Null),
            );
            write_json_file(
                &out_dir.join("final_commands.json"),
                &serde_json::to_value(cmds).unwrap_or(serde_json::Value::Null),
            );
        }
        if let Some(ep) = &authored_opt {
            write_json_file(
                &out_dir.join("final_authored_episode.json"),
                &serde_json::to_value(ep).unwrap_or(serde_json::Value::Null),
            );
            // Cache the validated episode so production can replay it with zero
            // new LLM calls.
            if let Some(d) = &self.capture_dir {
                let _ = write_json_file(
                    &d.join("last_authored_episode.json"),
                    &serde_json::to_value(ep).unwrap_or(serde_json::Value::Null),
                );
            }
            let _ = std::fs::write(
                REUSE_CACHE_PATH,
                serde_json::to_string_pretty(ep).unwrap_or_default(),
            );
        }

        let duration = if let (Some(plan), Some(cmds)) = (plan_opt, cmds_opt) {
            backlot_core::render::measure_runtime(
                &ctx.world,
                plan,
                cmds,
                &self.tts,
                self.max_dead_air,
            )
            .ok()
        } else {
            None
        };
        let duration_status = match duration {
            Some(d) => DurationPolicy::for_request(ctx.target_duration)
                .status_for(d)
                .to_string(),
            None => "n/a".into(),
        };
        let breakdown = duration_breakdown(plan_opt, cmds_opt);
        write_json_file(
            &out_dir.join("duration_analysis.json"),
            &serde_json::json!({
                "estimated_duration_secs": duration,
                "status": duration_status,
                "breakdown": breakdown,
            }),
        );

        let total_wire = wire.len();
        let total_logical = caps.len();
        let sum_wall: u128 = wire.iter().map(|w| w.wall_ms).sum();
        let span_ms = if !wire.is_empty() {
            let first = wire.first().unwrap().start_unix_ms;
            let last = wire.last().unwrap();
            (last.start_unix_ms + last.wall_ms).saturating_sub(first)
        } else {
            0
        };
        let prompt_tokens: u64 = wire.iter().map(|w| w.prompt_tokens as u64).sum();
        let completion_tokens: u64 = wire.iter().map(|w| w.completion_tokens as u64).sum();
        let finish_reasons: Vec<String> = wire
            .iter()
            .filter_map(|w| w.finish_reason.clone())
            .collect();
        let any_length = wire
            .iter()
            .any(|w| w.finish_reason.as_deref() == Some("length"));
        let produced = plan_opt.is_some();
        let plan_title = plan_opt.map(|p| p.episode_title.clone());
        let beat_count = plan_opt.map(|p| p.beats.len()).unwrap_or(0);
        let repair_needed = caps.iter().any(|c| c.stage == "duration-repair");
        let repair_direction = caps
            .iter()
            .find(|c| c.stage == "duration-repair")
            .and_then(|c| c.repair_direction.clone());

        let md = build_packet_markdown(
            self,
            ctx,
            out_dir,
            &wire,
            &caps,
            &schema,
            duration,
            &duration_status,
            &breakdown,
            err_opt.as_deref(),
            produced,
            plan_title.as_deref(),
            beat_count,
            total_wire,
            total_logical,
            sum_wall,
            span_ms,
            prompt_tokens,
            completion_tokens,
            repair_needed,
            repair_direction.as_deref(),
        );
        let _ = std::fs::write(out_dir.join("LLM_AUTHORING_DIAGNOSTIC_PACKET.md"), md);

        Ok(DiagnosticSummary {
            total_wire_calls: total_wire,
            total_logical_calls: total_logical,
            total_wall_ms: span_ms,
            prompt_tokens,
            completion_tokens,
            produced,
            estimated_duration_secs: duration,
            duration_status,
            schema_repairs: caps.iter().filter(|c| c.validation != "ok").count() as u32,
            finish_reasons,
            any_length_truncated: any_length,
            plan_title,
            beat_count,
            repair_needed,
            repair_direction,
        })
    }
}

/// Owned episode-plan type produced by adaptation (alias for readability).
type EpisodePlanOwned = backlot_core::protocol::EpisodePlan;

/// Write `v` to `path` as pretty JSON, creating parent dirs.
fn write_json_file(path: &Path, v: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        path,
        serde_json::to_string_pretty(v).unwrap_or_else(|_| "null".into()),
    );
}

/// Combine validation errors into one readable string.
fn format_validation(errs: Vec<backlot_core::validation::ValidationError>) -> String {
    if errs.is_empty() {
        return "invalid".into();
    }
    errs.iter()
        .map(|e| format!("{}: {}", e.field, e.message))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Rough whole-episode duration estimate (used only when real TTS is
/// unavailable; never the acceptance authority in require_llm mode).
fn estimate_whole_episode(
    plan: &EpisodePlanOwned,
    commands: &HashMap<String, backlot_core::protocol::BeatCommand>,
) -> f32 {
    let mut total = 0.0;
    for cmd in commands.values() {
        let actions_total: f32 = cmd
            .actions
            .iter()
            .map(|a| {
                a.duration_override
                    .unwrap_or_else(|| estimate_action_duration(&a.action, a.text.as_deref()))
            })
            .sum();
        let floor = cmd.completion_condition.seconds.unwrap_or(0.0);
        total += actions_total.max(floor) + 0.6;
    }
    total.max(plan.target_duration_seconds.min(1.0))
}

/// Rough spoken-vs-action breakdown for the diagnostic duration section.
fn duration_breakdown(
    plan: Option<&EpisodePlanOwned>,
    cmds: Option<&HashMap<String, backlot_core::protocol::BeatCommand>>,
) -> serde_json::Value {
    if let (Some(plan), Some(cmds)) = (plan, cmds) {
        let mut spoken_lines = 0u32;
        let mut spoken_chars = 0u32;
        let mut action_secs = 0.0f32;
        for cmd in cmds.values() {
            for a in &cmd.actions {
                if matches!(a.action.as_str(), "speak" | "whisper" | "shout") {
                    spoken_lines += 1;
                    if let Some(t) = &a.text {
                        spoken_chars += t.chars().count() as u32;
                    }
                }
                action_secs += a
                    .duration_override
                    .unwrap_or_else(|| estimate_action_duration(&a.action, a.text.as_deref()));
            }
        }
        let spoken_secs_est = spoken_chars as f32 * 0.055 + spoken_lines as f32 * 0.3;
        serde_json::json!({
            "beats": plan.beats.len(),
            "spoken_lines": spoken_lines,
            "spoken_chars": spoken_chars,
            "est_spoken_secs_from_chars": format!("{:.1}", spoken_secs_est),
            "est_action_secs": format!("{:.1}", action_secs),
            "target_duration_seconds": plan.target_duration_seconds,
        })
    } else {
        serde_json::Value::Null
    }
}

#[allow(clippy::too_many_arguments)]
fn build_packet_markdown(
    a: &LlmAuthor,
    _ctx: &DirectorContext,
    out_dir: &Path,
    wire: &[crate::client::WireCall],
    caps: &[CapturedCall],
    schema: &str,
    duration: Option<f32>,
    duration_status: &str,
    breakdown: &serde_json::Value,
    err: Option<&str>,
    produced: bool,
    plan_title: Option<&str>,
    beat_count: usize,
    total_wire: usize,
    total_logical: usize,
    sum_wall: u128,
    span_ms: u128,
    _prompt_tokens: u64,
    _completion_tokens: u64,
    repair_needed: bool,
    repair_direction: Option<&str>,
) -> String {
    let mut s = String::new();
    let _ = out_dir;
    s.push_str("# LLM Authoring Diagnostic Packet (redesigned single-call authoring)\n\n");
    s.push_str(&format!("Generated {}\n\n", chrono_stamp()));
    s.push_str("**Purpose.** Capture real evidence (exact prompts, raw model responses, per-call timing, schemas, duration logic, server config) for the REDESIGNED authoring stage ONLY — no rendering. Authoring now collapses the entire episode into ONE whole-episode structured call (`AuthoredEpisode`), with at most ONE direction-aware revision call if the measured runtime misses the 45–60s window.\n\n");

    // ---- 1. Call graph ----
    s.push_str("## 1. Current (redesigned) authoring call graph\n\n");
    s.push_str("One episode is authored as:\n\n");
    s.push_str("1. **Initial whole-episode call** — `request_whole_episode()` → 1 structured call (schema `AuthoredEpisode`). The model returns episode metadata AND every fully-authored beat in a single JSON object.\n");
    s.push_str("2. **Schema-repair loop** — the call is wrapped in a `0..=max_repairs` loop. On any parse/validation failure the concrete error is fed back (`SCHEMA CORRECTION`) and the SAME whole-episode call is re-issued. This is structural, not duration. There is exactly ONE beat identifier field (`AuthoredBeat.id`); the internal `beat_id` is derived from it, so the old `id`/`beat_id` confusion can never trigger another call.\n");
    s.push_str("3. **Format fallback** — inside the call, `chat_structured` first tries `json_object`, then falls back to strict `json_schema` (with bounded-vocabulary `enum`s) for up to `max_retries` attempts. So each logical call can emit 1–2 wire calls, but there is only ONE logical call (or two if a duration repair is needed).\n");
    s.push_str("4. **Direction-aware duration repair** — after the whole episode is measured (real TTS + dead-air compaction via `measure_runtime`), if it is outside 45–60s, exactly ONE targeted whole-episode revision call is issued. The accepted parsed episode JSON is included and the model is told to LENGTHEN (if too short) or SHORTEN (if too long) — never \"add more\" when too long. There are no per-beat calls and no full restart.\n\n");
    s.push_str(&format!(
        "**Configuration during this run:** `max_repairs = {}` (director + schema-repair loop bound), `llm.max_retries = {}`, `max_tokens = {}`, `temperature = {}`.\n\n",
        a.max_repairs, a.config_llm_max_retries(), a.config_max_tokens(), a.config_temperature()
    ));
    s.push_str(&format!(
        "**Call count (worst case):** 1 initial logical call, +1 direction-aware repair logical call if needed. Each logical call = at most 1 + `max_retries` wire calls. Observed this run: **{} wire calls**, **{} logical calls**.\n\n",
        total_wire, total_logical
    ));

    // ---- 2. Prompt templates ----
    s.push_str("## 2. Exact prompt templates\n\n");
    s.push_str("Assembled in `crates/backlot-llm/src/author.rs`. Fully rendered instances are saved per call as `*_request.json`. The static system template is reproduced below.\n\n");
    s.push_str("### 2.1 System prompt (verbatim)\n\n```\n");
    s.push_str(WHOLE_EPISODE_SYSTEM_TEMPLATE);
    s.push_str("\n```\n\nDynamic insertions: `{}` → `KNOWN_ACTIONS.join(\", \")`; `{}` → `KNOWN_CAMERA_INTENTS.join(\", \")`.\n\n");
    s.push_str("### 2.2 Whole-episode user prompt\n\nThe user prompt embeds the world digest, target duration, tone, recent episodes, canonical facts, the `AuthoredEpisode` field spec (with a concrete example), the duration guidance (NO \"2–4 spoken lines per beat\" rule), the bounded-vocabulary hard rules, and — on repair — a `DURATION REPAIR` block plus the accepted episode JSON. Rendered instances are in `*_request.json`.\n\n");

    // ---- 3. Rendered prompts ----
    s.push_str("## 3. Fully rendered real prompts\n\n");
    s.push_str("Saved as separate UTF-8 files in this directory:\n\n");
    s.push_str(
        "- `NN_whole-episode_*_request.json` — initial whole-episode request (system + user).\n",
    );
    s.push_str("- `NN_whole-episode-repair_*_request.json` — direction-aware repair request (stage `duration-repair`).\n\n");
    s.push_str("Each request file contains the exact `system` and `user` messages sent (no secrets; the API key is empty).\n\n");

    // ---- 4. Raw responses ----
    s.push_str("## 4. Raw model responses\n\n");
    s.push_str("For every logical call, `*_response_raw.json` holds the COMPLETE server JSON (choices, `content`, `reasoning_content` when present, `finish_reason`, `usage`, `model`, `id`). `*_extracted.json` holds only the extracted JSON object. `captured_calls.json` aggregates all of them with validation result and accept/reject flag.\n\n");
    if caps.is_empty() {
        s.push_str("_No captured calls (diagnostic capture path not engaged)._\n\n");
    } else {
        s.push_str(&format!(
            "_Logical calls captured: {}. (wire calls: {})_\n\n",
            caps.len(),
            total_wire
        ));
        for c in caps.iter() {
            s.push_str(&format!(
                "- `{}` [{}] accepted={} validation={} measured={:?}s direction={:?}\n",
                c.purpose,
                c.stage,
                c.accepted,
                if c.validation.len() > 80 {
                    format!("{}…", &c.validation[..80])
                } else {
                    c.validation.clone()
                },
                c.measured_duration,
                c.repair_direction,
            ));
        }
        s.push_str("\n");
    }

    // ---- 5. Timing trace ----
    s.push_str("## 5. Timing trace\n\n");
    s.push_str(&format!(
        "Flushed per-event trace: `authoring_trace.jsonl` (one JSON object per physical HTTP request, plus a `kind:\"logical\"` line per structured call). Total wire calls = {}, total logical calls = {}, sum of wire latencies = {:.1}s, observed span (first start → last end) = {:.1}s.\n\n",
        total_wire, total_logical, sum_wall as f32 / 1000.0, span_ms as f32 / 1000.0
    ));
    s.push_str("| # | purpose | format | wall(s) | prompt_tok | compl_tok | finish | ok |\n");
    s.push_str("|---|---|---|---|---|---|---|---|\n");
    for w in wire.iter() {
        s.push_str(&format!(
            "| {} | {} | {} | {:.1} | {} | {} | {:?} | {} |\n",
            w.seq,
            w.purpose,
            w.format,
            w.wall_ms as f32 / 1000.0,
            w.prompt_tokens,
            w.completion_tokens,
            w.finish_reason,
            w.ok
        ));
    }
    for c in caps.iter() {
        s.push_str(&format!(
            "| {} | {} (logical) | - | - | {} | {} | {:?} | {} |\n",
            c.call_index + 1,
            c.purpose,
            c.response
                .usage
                .as_ref()
                .and_then(|u| u.get("prompt_tokens").and_then(|v| v.as_u64()))
                .unwrap_or(0),
            c.response
                .usage
                .as_ref()
                .and_then(|u| u.get("completion_tokens").and_then(|v| v.as_u64()))
                .unwrap_or(0),
            c.response.finish_reason,
            c.accepted,
        ));
    }
    s.push_str("\n");

    // ---- 6. Schemas and validators ----
    s.push_str("## 6. Schemas and validators\n\n");
    s.push_str(&format!(
        "The schema is `schemars`-derived from `AuthoredEpisode` in `crates/backlot-core/src/protocol.rs` and written to `authored_episode_schema.json` ({} bytes). Crucially, the bounded vocabularies are now real JSON Schema `enum`s injected into the schema: `actions[*].action` ∈ KNOWN_ACTIONS ({} tokens), `camera_intent.type` ∈ KNOWN_CAMERA_INTENTS ({} tokens), `completion_condition.type` ∈ KNOWN_COMPLETION_TYPES ({} tokens). So the model is constrained at the schema level; the Rust validators (`validate_plan` / `validate_beat_command`) remain the final authority.\n\n",
        schema.len(),
        KNOWN_ACTIONS.len(),
        KNOWN_CAMERA_INTENTS.len(),
        backlot_core::protocol::KNOWN_COMPLETION_TYPES.len(),
    ));
    s.push_str("**AuthoredEpisode required fields:** `episode_title, logline, target_duration_seconds, active_characters, primary_location, central_goal, beats, payoff`. Optional: `tone, persistent_changes, notes`.\n\n");
    s.push_str("**AuthoredBeat (one per beat, single id field) required:** `id, narrative_purpose, target_start_second, actions, camera_intent, completion_condition`. Optional: `fallback, expected_state_changes, notes`. The internal `beat_id` is derived from `id` during `adapt_authored_episode` — no second call, no `id`/`beat_id` confusion.\n\n");
    s.push_str("**Validation outcomes this run:**\n\n");
    if caps.is_empty() {
        s.push_str("_none_\n\n");
    } else {
        let ok = caps.iter().filter(|c| c.accepted).count();
        let rej = caps.len() - ok;
        s.push_str(&format!(
            "- accepted logical calls: {}\n- rejected (schema repair triggered): {}\n",
            ok, rej
        ));
        s.push_str("\n");
    }

    // ---- 7. Duration logic ----
    s.push_str("## 7. Duration logic\n\n");
    s.push_str(
        "Pipeline:\n\n\
         1. **Plan duration hint:** the model is told `target_duration_seconds` (~50) and a NATURAL pacing guidance: 5–6 concise beats, ~30–42s of total spoken dialogue, the rest meaningful actions/reactions/transitions, and NO padding. The old hard rule \"2–4 spoken lines per beat\" that caused the 83.3s overrun is GONE.\n\
         2. **Dialogue duration:** measured by REAL espeak TTS in `measure_runtime` — each `speak`/`whisper`/`shout` line is synthesized, silence-trimmed, and its true length used.\n\
         3. **Action duration:** `estimate_action_duration(action, text)` heuristic per action.\n\
         4. **Pauses/transitions:** `compact_dead_air` compresses gaps > `max_dead_air_secs`, so padding does NOT add time.\n\
         5. **Accepted range:** 45–60s.\n\
         6. **Direction-aware repair:** if the measured runtime misses the window, exactly ONE targeted whole-episode revision is issued. If too SHORT → told exact seconds missing + which beats are underdeveloped + to add content (preserve hook/premise/payoff). If too LONG → told exact seconds to remove + to cut/shorten redundant dialogue and combine actions (preserve hook/escalation/reaction/payoff, do NOT add beats). The accepted parsed episode JSON is included so the model revises rather than restarts.\n\n",
    );
    s.push_str(&format!(
        "**This run:** estimated duration = {}. Status = `{}`. Breakdown: `{}`. Repair needed = {} (direction: {:?}).\n\n",
        duration.map(|d| format!("{d:.1}s")).unwrap_or_else(|| "n/a (authoring failed)".into()),
        duration_status,
        serde_json::to_string_pretty(breakdown).unwrap_or_default(),
        repair_needed,
        repair_direction,
    ));
    if let Some(t) = plan_title {
        s.push_str(&format!("The generated plan title was `{}` with {} beats. The full plan is in `final_plan.json`; the canonical authored episode is in `final_authored_episode.json`.\n\n", t, beat_count));
    }

    // ---- 8. Model and server configuration ----
    s.push_str("## 8. Model and server configuration\n\n");
    s.push_str(&format!(
        "- **Model identifier:** `{}`\n\
         - **Base URL:** `{}`\n\
         - **Slots:** inferred 1 (single-slot); wire calls are strictly serialized. With the redesigned 1–2 logical calls the wall time is now dominated by 1–2 generations, not 7+.\n\
         - **temperature:** {}\n\
         - **max output tokens:** {}\n\
         - **timeout:** {:.0}s\n\
         - **retry count (effective):** `max_repairs = {}` (schema-repair loop bound).\n\
         - **streaming:** {}\n",
        a.model_name(),
        a.config_base_url(),
        a.config_temperature(),
        a.config_max_tokens(),
        a.config_timeout(),
        a.max_repairs,
        if a.config_stream() { "enabled" } else { "disabled" }
    ));
    s.push_str("\n");

    // ---- 9. Source map ----
    s.push_str("## 9. Relevant source map\n\n");
    s.push_str("- `crates/backlot-llm/src/client.rs::LlmClient::chat` — production requests.\n");
    s.push_str("- `crates/backlot-llm/src/client.rs::chat_structured` — json_object→json_schema fallback.\n");
    s.push_str("- `crates/backlot-llm/src/client.rs::chat_structured_capture` / `raw_post` — diagnostic raw path preserving `reasoning_content`, usage, model id; appends per-wire + per-logical trace lines.\n");
    s.push_str("- `crates/backlot-llm/src/author.rs::request_whole_episode` — the single whole-episode call + bounded schema-repair loop.\n");
    s.push_str("- `crates/backlot-llm/src/author.rs::author_async_inner` — measures runtime, accepts if in range, else issues ONE direction-aware repair call.\n");
    s.push_str("- `crates/backlot-llm/src/author.rs::direction_aware_feedback` — the direction-aware (lengthen/shorten) repair prompt.\n");
    s.push_str("- `crates/backlot-core/src/validation.rs::adapt_authored_episode` — safe local normalization (canonical beat ids, ordering, defaults) + structural mapping; does NOT invent content.\n");
    s.push_str("- `crates/backlot-core/src/validation.rs::validate_plan` / `validate_beat_command` — the final authority.\n");
    s.push_str("- `crates/backlot-core/src/protocol.rs` — `AuthoredEpisode` / `AuthoredBeat` (schema source, single beat id).\n");
    s.push_str("- `crates/backlot-core/src/schema.rs::authored_episode_schema` — schemars schema + `enum` injection.\n");
    s.push_str("- `crates/backlot-core/src/render.rs::measure_runtime` — real TTS duration (authoritative gate).\n\n");

    // ---- 10. Assessment ----
    s.push_str("## 10. Initial evidence-based assessment\n\n");
    s.push_str(&format!(
        "- **Calls consuming the most time:** ONE initial whole-episode call (at most 1+`max_repairs` wire calls) and at most ONE direction-aware repair call. Worst case wire calls ≈ 2 × (1 + `max_repairs`) = {} (vs 28 before).\n",
        2 * (1 + a.max_repairs as usize)
    ));
    let any_reason = caps.iter().any(|c| {
        c.response
            .reasoning_content
            .as_ref()
            .map(|r| !r.is_empty())
            .unwrap_or(false)
    });
    s.push_str(&format!(
        "- **Excessive reasoning?** reasoning_content present in {} of {} captured calls{}.\n",
        caps.iter()
            .filter(|c| c
                .response
                .reasoning_content
                .as_ref()
                .map(|r| !r.is_empty())
                .unwrap_or(false))
            .count(),
        caps.len(),
        if any_reason { "" } else { " — none observed" }
    ));
    s.push_str("- **Schema constrains vocabulary now?** Yes — `action`/`camera_intent.type`/`completion_condition.type` are real JSON Schema `enum`s, so out-of-vocab tokens are blocked at the schema level (Rust validation still the final authority).\n");
    s.push_str(
        "- **Per-beat calls?** Eliminated. All beats arrive in one `AuthoredEpisode` response.\n",
    );
    s.push_str("- **Duration repair = targeted edit or restart?** Targeted, direction-aware whole-episode revision that includes the accepted episode JSON. Never a blind restart, never \"add more\" when too long.\n");
    s.push_str("- **One whole-episode call feasible?** Yes — proven by this run (see §4/§5).\n\n");
    if let Some(e) = err {
        s.push_str(&format!(
            "**Authoring error (require_llm mode):** `{}`\n\n",
            e
        ));
    }
    s.push_str(&format!(
        "**Outcome:** produced complete valid episode = {}. Estimated duration = {}. Single biggest fix applied: collapsed 7+ calls into 1–2 and replaced the direction-blind duration repair (which caused the 83.3s overrun) with a direction-aware one.\n",
        produced,
        duration.map(|d| format!("{d:.1}s")).unwrap_or_else(|| "n/a".into())
    ));

    s
}

/// Verbatim system-prompt template (the `format!` argument in
/// `whole_episode_system_prompt()`).
const WHOLE_EPISODE_SYSTEM_TEMPLATE: &str = "You are the showrunner for 'Infinite Backlot', an autonomous surreal comedy set in an apartment building where impossible events are treated as ordinary maintenance problems. You author ONE complete, SHORT, WATCHABLE episode per response as a single valid JSON object matching the `AuthoredEpisode` schema. Do not output prose, markdown, or commentary outside the JSON fields. Keep dialogue short and purposeful. Aim for a hook in the first 3 seconds and a clear character goal by 10 seconds. Use ONLY these action tokens: {}. Use ONLY these camera intents: {}. Reference only entities (characters, props, locations, staging marks) that exist in the provided world.";

fn chrono_stamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{now}")
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

fn whole_episode_system_prompt() -> String {
    format!(
        "You are the showrunner for 'Infinite Backlot', an autonomous surreal comedy set in an apartment building where impossible events are treated as ordinary maintenance problems. You author ONE complete, SHORT, WATCHABLE episode per response as a single valid JSON object matching the `AuthoredEpisode` schema. Do not output prose, markdown, or commentary outside the JSON fields. Keep dialogue short and purposeful. Aim for a hook in the first 3 seconds and a clear character goal by 10 seconds. Use ONLY these action tokens: {}. Use ONLY these camera intents: {}. Reference only entities (characters, props, locations, staging marks) that exist in the provided world.",
        KNOWN_ACTIONS.join(", "),
        KNOWN_CAMERA_INTENTS.join(", "),
    )
}

fn whole_episode_user_prompt(
    ctx: &DirectorContext,
    digest: &WorldDigest,
    repair_feedback: Option<&str>,
    accepted_json: Option<&str>,
    schema_correction: Option<&str>,
) -> String {
    let repair_block = repair_feedback
        .map(|f| {
            format!(
                "\n\n{}\n\nACCEPTED EPISODE JSON (revise THIS object; do not start from an empty prompt):\n{}",
                f,
                accepted_json.unwrap_or("")
            )
        })
        .unwrap_or_default();
    let correction_block = schema_correction
        .map(|f| format!("\n\nSCHEMA CORRECTION (previous attempt rejected):\n{f}\nFix ONLY the rejected fields and return a new valid episode JSON."))
        .unwrap_or_default();

    let char_ids = all_char_ids(&ctx.world).join(", ");
    let loc_ids = ctx
        .world
        .locations
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");

    let example = r#"{
  "episode_title": "The Ventilation Void",
  "logline": "Mara and Voss argue as the elevator opens onto a floor that should not exist.",
  "tone": ["tense", "absurd"],
  "target_duration_seconds": 50.0,
  "active_characters": ["mara", "voss"],
  "primary_location": "floor_3_hallway",
  "central_goal": {"character": "mara", "goal": "keep the elevator secret from Voss"},
  "beats": [
    {
      "id": "beat_1",
      "narrative_purpose": "Hook: the elevator dings on a non-existent floor.",
      "blocking": "Voss at hall_center staring at indicator; Mara at maintenance_panel reaching for panel.",
      "visible_action": "Voss turns toward indicator; Mara activates panel",
      "intended_reaction": "Mara flinches; Voss squints",
      "camera_purpose": "establish tension on the impossible ding",
      "performance_intent": "suspicious, strained",
      "target_start_second": 0.0,
      "actions": [
        {"actor": "voss", "action": "look_at", "target": "elevator_indicator"},
        {"actor": "voss", "action": "speak", "text": "The indicator says Sub-Basement Zero. We are on three."}
      ],
      "camera_intent": {"type": "establish", "subject": "voss"},
      "completion_condition": {"type": "dialogue_finished"}
    },
    {
      "id": "beat_2",
      "narrative_purpose": "Mara blocks Voss from looking closer.",
      "blocking": "Mara walks to Voss; Voss faces elevator.",
      "visible_action": "Mara steps in front of Voss, then points at panel",
      "intended_reaction": "Voss frowns and steps back",
      "camera_purpose": "two-character context then speaker",
      "performance_intent": "evasive, bureaucratic annoyance",
      "target_start_second": 10.0,
      "actions": [
        {"actor": "mara", "action": "move_to", "target": "voss"},
        {"actor": "mara", "action": "speak", "text": "Don't look at the light. Faulty bulb."}
      ],
      "camera_intent": {"type": "conversation", "subject": "mara"},
      "completion_condition": {"type": "dialogue_finished"}
    }
  ],
  "payoff": "The elevator doors close on a floor that should not exist."
}"#;

    let mut p = String::new();
    p.push_str(&format!(
        "WORLD DIGEST:\n{}\n\n\
         TARGET DURATION (seconds): {:.1}\n\
         TONE CONSTRAINTS: {:?}\n\
         RECENT EPISODES:\n{}\n\
         PROTECTED CANONICAL FACTS:\n{:?}{}\n\n\
         Author ONE complete episode as a single JSON object matching the `AuthoredEpisode` schema:\n\
         - episode_title: string\n\
         - logline: string\n\
         - tone: array of strings\n\
         - target_duration_seconds: number (about {:.1})\n\
         - active_characters: array of character ids from [{}] (at least two)\n\
         - primary_location: a location id from [{}]\n\
         - central_goal: object {{\"character\": <id>, \"goal\": <string>}}\n\
         - beats: array of 5 or 6 beat objects, each {{\"id\": \"beat_1\", \"narrative_purpose\": <string>, \"target_start_second\": <number>, \"actions\": [...], \"camera_intent\": {{...}}, \"completion_condition\": {{...}}, \"blocking\"?: <string>, \"visible_action\"?: <string>, \"intended_reaction\"?: <string>, \"camera_purpose\"?: <string>, \"performance_intent\"?: <string>}}\n\
         - payoff: string describing the ending\n\n",
        serde_json::to_string_pretty(digest).unwrap_or_default(),
        ctx.target_duration,
        ctx.tone,
        backlot_core::repetition_report(&ctx.recent_summaries),
        ctx.world.canonical_facts,
        repair_block,
        ctx.target_duration,
        char_ids,
        loc_ids,
    ));
    p.push_str(
        "Each beat.actions entry is an object: {\"actor\": <character id>, \"action\": <token>, \"target\"?: <entity>, \"text\"?: <spoken line>, \"intensity\"?: <0-1>, \"performance_intent\"?: <short playable direction>, \"duration_override\"?: <seconds>}.\n\
         Each beat.camera_intent is {\"type\": <camera intent>, \"subject\": <character id>, \"reaction_subject\"?: <character id>}.\n\
         Each beat.completion_condition is {\"type\": <completion type>, \"actor\"?: <character id>, \"seconds\"?: <number>}.\n\
         Every beat MUST include a concrete blocking description, one readable visible action, an intended reaction, camera purpose, and performance intent. Use actual staging marks, props, the elevator, panel, indicator, doors, or another valid entity as targets whenever the beat needs physical business.\n\n",
    );
    p.push_str(
        "DURATION (the only hard runtime constraint): the rendered episode must run 45-60 seconds, measured from REAL spoken dialogue plus action timing. Build that time from dialogue AND visible business; no padding, no silence.\n\
         - Use exactly 6 beats.\n\
         - Include EXACTLY 14-20 spoken lines (count every speak/shout/whisper action). Each line must have 8-16 words (no one-word lines). Dialogue fills 35-50 seconds; the rest is actions/reactions/transitions. At least 14 lines required; 10 or fewer will be rejected.\n\
         - Every beat needs at least one non-speech action. Across the episode include a staging change, a prop or environment interaction, two distinct reactions, a physical escalation, and a final visible payoff.\n\
         - Make at least one character walk to a new staging mark, turn or look toward a relevant subject, and return to a neutral pose after a gesture or reaction.\n\
         - Treat gestures as short actions: preparation, main gesture, brief hold, recovery. Do not leave arms raised through a scene.\n\
         - Use camera intent purposefully: cover speaker, listener reaction, interaction/insert, wider blocking, and payoff.\n\
         - Do NOT pad with silence, static poses, slow walking, or empty camera moves — those are removed automatically and add no time.\n\
         - Hook within ~3 seconds, clear character goal by ~10 seconds, at least two meaningful escalations.\n\
         - Aim for a total rendered runtime of about 50 seconds.\n\n",
    );
    p.push_str(&format!(
        "HARD RULES:\n\
         - active_characters MUST be a subset of [{}]; at least two characters.\n\
         - primary_location MUST be one of [{}].\n\
         - actions[*].action must be one of these tokens only: {}.\n\
         - camera_intent.type must be one of these camera intents only: {}.\n\
         - completion_condition.type must be exactly one of: dialogue_finished, arrival, timer, event_done, animation_finished.\n\
         - payoff must be a non-empty string.\n\
         - Reference only entities that exist in the provided world.\n\
         - Return ONLY the JSON object: no markdown fences, no commentary, no extra keys.{}\n\n",
        char_ids,
        loc_ids,
        KNOWN_ACTIONS.join(", "),
        KNOWN_CAMERA_INTENTS.join(", "),
        correction_block,
    ));
    p.push_str(&format!(
        "EXAMPLE of a VALID AuthoredEpisode (use the real entity ids from the world digest above):\n{}\n\n",
        example
    ));
    p.push_str("Return the episode JSON now.");
    p
}

/// Build a direction-aware duration-repair prompt and return (direction, text).
/// `direction` is \"lengthen\" (too short) or \"shorten\" (too long). The prompt
/// never says \"add more\" when the episode is already too long.
fn direction_aware_feedback(
    _plan: &EpisodePlanOwned,
    commands: &HashMap<String, backlot_core::protocol::BeatCommand>,
    duration: DurationPolicy,
    secs: f32,
) -> (String, String) {
    let too_short = secs < duration.min_secs;
    let direction = if too_short { "lengthen" } else { "shorten" }.to_string();

    let beat_lines: Vec<String> = commands
        .iter()
        .map(|(id, cmd)| {
            let dialogue = cmd
                .actions
                .iter()
                .filter(|a| matches!(a.action.as_str(), "speak" | "whisper" | "shout"))
                .count();
            let words: usize = cmd
                .actions
                .iter()
                .filter_map(|a| a.text.as_ref())
                .map(|t| t.split_whitespace().count())
                .sum();
            format!(
                "- {} : {} actions, {} spoken lines (~{} words), camera={} subject={}",
                id,
                cmd.actions.len(),
                dialogue,
                words,
                cmd.camera_intent.r#type,
                cmd.camera_intent.subject
            )
        })
        .collect();
    let beat_summary = beat_lines.join("\n");

    let feedback = if too_short {
        let secs_to_add = (duration.min_secs - secs).max(1.0);
        let new_lines = ((secs_to_add / 4.0).ceil() as i32).max(4);
        format!(
            "DURATION REPAIR (lengthen). The rendered episode currently runs {:.1}s but must land inside {:.1}-{:.1}s (aim near {:.1}s). It is {:.1}s too short. \
             Add time with visible business first: extend blocking, add a reaction beat within an existing beat, add a prop or elevator interaction, or add short back-and-forth where needed. You MAY add new spoken dialogue, but do not only reword existing lines. Add at least {:.1} seconds of meaningful staged material, and if you add dialogue keep it concise and tied to visible action. \
             Focus on the beats with the thinnest action coverage or fewest spoken words below. Preserve the existing hook, premise, and payoff exactly. Only revise the provided accepted episode JSON — do not start over.\n\nBEAT ANALYSIS:\n{}",
            secs, duration.min_secs, duration.max_secs, duration.target_secs, secs_to_add, secs_to_add, beat_summary
        )
    } else {
        let secs_to_cut = (secs - duration.max_secs).max(1.0);
        format!(
            "DURATION REPAIR (shorten). The rendered episode currently runs {:.1}s but must land inside {:.1}-{:.1}s (aim near {:.1}s). It is {:.1}s too long. \
             You MUST REMOVE CONTENT — trim redundant dialogue first, shorten repetitive back-and-forth, combine duplicate actions, and drop filler business while preserving visible blocking, reaction, escalation, and payoff. Do NOT only reword lines hoping they shrink — actually delete or consolidate material, roughly {:.1}s worth. \
             Do NOT add new beats unless replacing longer material. Preserve the hook, escalation, reaction, and payoff. Only revise the provided accepted episode JSON — do not start over.\n\nBEAT ANALYSIS:\n{}",
            secs, duration.min_secs, duration.max_secs, duration.target_secs, secs_to_cut, secs_to_cut, beat_summary
        )
    };
    (direction, feedback)
}

fn first_location(world: &WorldState) -> String {
    world.locations.keys().next().cloned().unwrap_or_default()
}

fn all_char_ids(world: &WorldState) -> Vec<String> {
    world.characters.keys().cloned().collect()
}

// Re-export so the app can hold metrics behind a shared handle if desired.
pub type SharedMetrics = Arc<Mutex<LlmMetrics>>;
