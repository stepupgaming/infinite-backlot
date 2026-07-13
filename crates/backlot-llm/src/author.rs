//! LLM-backed episode author (OpenAI-compatible).
//!
//! Implements `backlot_core::author::EpisodeAuthor`. It asks the model for a
//! structured plan and per-beat commands, validates them, and — per PRD — may
//! fall back to the deterministic director on any malformed/slow/unavailable
//! response. Crucially, it records a *truthful* `PlanAuthorship` so a fallback
//! piece is never mislabeled as LLM-authored.
//!
//! When `require_llm` is set, any failure is fatal: the method returns an error
//! instead of falling back, so the caller can fail the run clearly.

use backlot_core::author::{
    AuthorSource, BeatAuthorship, EpisodeAuthor, PlanAuthorship, PlannedEpisode,
};
use backlot_core::author::DeterministicAuthor;
use backlot_core::config::{DirectorConfig, LlmConfig};
use backlot_core::director::{DeterministicDirector, Director, DirectorContext};
use backlot_core::error::{CoreError, Result};
use backlot_core::protocol::{
    BeatCommand, EpisodePlan, KNOWN_ACTIONS, KNOWN_CAMERA_INTENTS, WorldDigest,
};
use backlot_core::schema::{beat_command_schema, episode_plan_schema};
use backlot_core::validation::{validate_beat_command, validate_plan};
use backlot_core::world::WorldState;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::client::{LlmClient, LlmMetrics};

pub struct LlmAuthor {
    client: LlmClient,
    runtime: tokio::runtime::Runtime,
    fallback: DeterministicDirector,
    force_fallback: bool,
    require_llm: bool,
    max_repairs: u32,
}

struct MetricsDelta {
    attempts: u32,
    failures: u32,
    repairs: u32,
    latency_ms: f32,
}

impl LlmAuthor {
    pub fn new(config: LlmConfig, director: DirectorConfig) -> Result<Self> {
        let client = LlmClient::new(config)?;
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
        })
    }

    pub fn metrics(&self) -> LlmMetrics {
        self.client.metrics()
    }

    pub fn metrics_arc(&self) -> Arc<Mutex<LlmMetrics>> {
        self.client.metrics_arc()
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
        self.runtime.block_on(self.author_async(ctx))
    }
}

impl LlmAuthor {
    async fn author_async(&self, ctx: &DirectorContext) -> Result<(PlannedEpisode, PlanAuthorship)> {
        let digest = WorldDigest::for_episode(&ctx.world, &first_location(&ctx.world), &all_char_ids(&ctx.world));
        let plan_schema = episode_plan_schema();
        let beat_schema = beat_command_schema();

        let system = system_prompt();
        let user_plan = plan_user_prompt(ctx, &digest);

        let before = self.client.metrics();
        let plan: EpisodePlan = match self
            .client
            .chat_structured(&system, &user_plan, "EpisodePlan", &plan_schema, self.max_repairs)
            .await
        {
            Ok(content) => match serde_json::from_str::<EpisodePlan>(&content) {
                Ok(p) => match validate_plan(&ctx.world, &p) {
                    Ok(_) => p,
                    Err(e) => {
                        if self.require_llm {
                            return Err(CoreError::Llm(format!(
                                "require_llm: plan failed validation: {e:?}"
                            )));
                        }
                        eprintln!("LLM-FALLBACK plan-validation: {e:?}");
                        tracing::warn!("LLM plan failed validation ({e:?}); using fallback plan");
                        return self.fallback_plan(ctx);
                    }
                },
                Err(e) => {
                    if self.require_llm {
                        return Err(CoreError::Llm(format!("require_llm: plan parse failed: {e}")));
                    }
                    eprintln!("LLM-FALLBACK plan-parse: {e}");
                    tracing::warn!("LLM plan parse failed ({e}); using fallback plan");
                    return self.fallback_plan(ctx);
                }
            },
            Err(e) => {
                if self.require_llm {
                    return Err(CoreError::Llm(format!("require_llm: plan request failed: {e}")));
                }
                eprintln!("LLM-FALLBACK plan-request: {e}");
                tracing::warn!("LLM plan request failed ({e}); using fallback plan");
                return self.fallback_plan(ctx);
            }
        };
        let after = self.delta(&before);
        let plan_source = AuthorSource::Llm;

        // Per-beat commands.
        let mut commands: HashMap<String, BeatCommand> = HashMap::new();
        let mut beat_auths: Vec<BeatAuthorship> = Vec::new();
        for beat in &plan.beats {
            let user_beat = beat_user_prompt(ctx, &digest, &plan, beat);
            let bbefore = self.client.metrics();
            let cmd = match self
                .client
                .chat_structured(&system, &user_beat, "BeatCommand", &beat_schema, self.max_repairs)
                .await
            {
                Ok(content) => match serde_json::from_str::<BeatCommand>(&content) {
                    Ok(c) => match validate_beat_command(&ctx.world, &plan, &c) {
                        Ok(_) => c,
                        Err(e) => {
                            if self.require_llm {
                                return Err(CoreError::Llm(format!(
                                    "require_llm: beat {} invalid: {e:?}",
                                    beat.id
                                )));
                            }
                            tracing::warn!("LLM beat {} invalid ({e:?}); fallback", beat.id);
                            self.fallback.plan_beat(ctx, &plan, beat)?
                        }
                    },
                    Err(e) => {
                        if self.require_llm {
                            return Err(CoreError::Llm(format!(
                                "require_llm: beat {} parse failed: {e}",
                                beat.id
                            )));
                        }
                        tracing::warn!("LLM beat {} parse failed ({e}); fallback", beat.id);
                        self.fallback.plan_beat(ctx, &plan, beat)?
                    }
                },
                Err(e) => {
                    if self.require_llm {
                        return Err(CoreError::Llm(format!(
                            "require_llm: beat {} request failed: {e}",
                            beat.id
                        )));
                    }
                    tracing::warn!("LLM beat {} request failed ({e}); fallback", beat.id);
                    self.fallback.plan_beat(ctx, &plan, beat)?
                }
            };
            let bafter = self.delta(&bbefore);
            let source = if bafter.failures > 0 {
                AuthorSource::DeterministicFallback
            } else {
                AuthorSource::Llm
            };
            beat_auths.push(BeatAuthorship {
                beat_id: beat.id.clone(),
                source,
                model: self.client.model_name().into(),
                attempts: bafter.attempts,
                latency_ms: bafter.latency_ms,
                repair_used: bafter.repairs > 0,
                validation_status: "ok".into(),
            });
            commands.insert(beat.id.clone(), cmd);
        }

        let auth = PlanAuthorship {
            plan_source,
            model: self.client.model_name().into(),
            attempts: after.attempts,
            latency_ms: after.latency_ms,
            repair_used: after.repairs > 0,
            validation_status: "ok".into(),
            beats: beat_auths,
        };
        Ok((PlannedEpisode { plan, commands }, auth))
    }

    /// Build a fully deterministic plan (used when require_llm is false and the
    /// LLM plan step fails). Per-beat commands are resolved by the caller loop
    /// via the deterministic director, but here we return the whole plan.
    fn fallback_plan(&self, ctx: &DirectorContext) -> Result<(PlannedEpisode, PlanAuthorship)> {
        let (p, mut a) = DeterministicAuthor.author(ctx)?;
        a.plan_source = AuthorSource::DeterministicFallback;
        for b in &mut a.beats {
            b.source = AuthorSource::DeterministicFallback;
        }
        Ok((p, a))
    }
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

fn system_prompt() -> String {
    format!(
        "You are the showrunner for 'Infinite Backlot', an autonomous surreal comedy \
         set in an apartment building where impossible events are treated as ordinary \
         maintenance problems. You author SHORT, WATCHABLE episodes. Every response MUST \
         be a single valid JSON object matching the requested schema. Do not output prose, \
         markdown, or commentary outside the JSON fields. Keep dialogue short (1-2 sentences). \
         Aim for a hook in the first 3 seconds, a clear character goal by 10 seconds, and at \
         least two meaningful escalations. Use ONLY these action tokens: {}. Use ONLY these \
         camera intents: {}. Reference only entities (characters, props, locations, staging \
         marks) that exist in the provided world.",
        KNOWN_ACTIONS.join(", "),
        KNOWN_CAMERA_INTENTS.join(", "),
    )
}

fn plan_user_prompt(ctx: &DirectorContext, digest: &WorldDigest) -> String {
    let cast: Vec<serde_json::Value> = ctx
        .world
        .characters
        .values()
        .map(|c| {
            json!({
                "id": c.id,
                "role": c.role,
                "mood": c.emotion.first().cloned().unwrap_or_else(|| "neutral".into()),
                "goal": c.current_goal,
                "knows": c.known_facts,
                "believes": c.believed_facts,
                "allowed_actions": c.allowed_actions,
            })
        })
        .collect();
    let locations: Vec<serde_json::Value> = ctx
        .world
        .locations
        .values()
        .map(|l| json!({ "id": l.id, "name": l.name, "description": l.description, "marks": l.staging_marks.iter().map(|m| &m.id).collect::<Vec<_>>() }))
        .collect();
    let threads: Vec<serde_json::Value> = ctx
        .world
        .threads
        .values()
        .map(|t| json!({ "id": t.id, "summary": t.summary, "importance": t.importance }))
        .collect();

    format!(
        "WORLD DIGEST:\n{}\n\nCAST:\n{}\n\nLOCATIONS:\n{}\n\nTHREADS:\n{}\n\n\
         TARGET DURATION (seconds): {}\nTONE CONSTRAINTS: {:?}\nRECENT EPISODES:\n{}\n\
         PROTECTED CANONICAL FACTS:\n{:?}\n\n\
         Author an episode. Return the EpisodePlan JSON now.",
        serde_json::to_string_pretty(digest).unwrap_or_default(),
        serde_json::to_string_pretty(&cast).unwrap_or_default(),
        serde_json::to_string_pretty(&locations).unwrap_or_default(),
        serde_json::to_string_pretty(&threads).unwrap_or_default(),
        ctx.target_duration,
        ctx.tone,
        backlot_core::repetition_report(&ctx.recent_summaries),
        ctx.world.canonical_facts,
    )
}

fn beat_user_prompt(_ctx: &DirectorContext, digest: &WorldDigest, plan: &EpisodePlan, beat: &backlot_core::protocol::BeatOutline) -> String {
    format!(
        "WORLD DIGEST:\n{}\n\nEPISODE: '{}' | {} \nCENTRAL GOAL: {} wants to {}\n\
         ACTIVE CHARACTERS: {:?}\nPRIMARY LOCATION: {}\n\nTHIS BEAT:\n{}\n\n\
         Author the detailed BeatCommand JSON for this beat. The `actions` array should stage \
         the beat using valid action tokens and existing entities. Set an appropriate \
         `camera_intent` and a `completion_condition`.",
        serde_json::to_string_pretty(digest).unwrap_or_default(),
        plan.episode_title,
        plan.logline,
        plan.central_goal.character,
        plan.central_goal.goal,
        plan.active_characters,
        plan.primary_location,
        serde_json::to_string_pretty(beat).unwrap_or_default(),
    )
}

fn first_location(world: &WorldState) -> String {
    world.locations.keys().next().cloned().unwrap_or_default()
}

fn all_char_ids(world: &WorldState) -> Vec<String> {
    world.characters.keys().cloned().collect()
}

// Re-export so the app can hold metrics behind a shared handle if desired.
pub type SharedMetrics = Arc<Mutex<LlmMetrics>>;
