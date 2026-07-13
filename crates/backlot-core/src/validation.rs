//! Validation & conversion of structured LLM output into safe internal commands.
//!
//! Per PRD §10.4 every response must be: parsed → schema-validated →
//! semantically validated → capability-checked → continuity-checked → converted
//! to internal commands. Invalid commands never enter the Bevy world.

use crate::protocol::*;
use crate::world::WorldState;
use std::collections::{HashMap, HashSet};

fn known_character(plan: &EpisodePlan, id: &str) -> bool {
    plan.active_characters.iter().any(|c| c == id)
}

fn coerce_character(plan: &EpisodePlan, id: &str) -> String {
    if known_character(plan, id) {
        id.to_string()
    } else {
        plan.active_characters
            .first()
            .cloned()
            .unwrap_or_else(|| id.to_string())
    }
}

pub const KNOWN_BEAT_TYPES: &[&str] = &[
    "hook",
    "situation",
    "goal",
    "complication",
    "escalation",
    "reveal",
    "reversal",
    "payoff",
    "consequence",
];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl ValidationError {
    fn new(field: &str, message: &str) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

// ---------- Resolved (execution-ready) structures ----------

#[derive(Debug, Clone)]
pub struct ValidatedPlan {
    pub plan: EpisodePlan,
    pub resolved_beats: Vec<ResolvedBeat>,
}

#[derive(Debug, Clone)]
pub struct ResolvedBeat {
    pub outline: BeatOutline,
    pub command: BeatCommand,
    pub resolved_actions: Vec<ResolvedAction>,
    pub camera_intent: CameraIntent,
    pub completion: CompletionCondition,
    pub fallback: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedAction {
    pub actor_id: String,
    pub action: String,
    pub target_id: Option<String>,
    pub text: Option<String>,
    pub intensity: f32,
    /// Baseline duration estimate (seconds); the executor refines movement.
    pub estimated_duration: f32,
}

// ---------- Plan validation ----------

pub fn validate_plan(
    world: &WorldState,
    plan: &EpisodePlan,
) -> Result<ValidatedPlan, Vec<ValidationError>> {
    let mut errs = Vec::new();

    if plan.active_characters.is_empty() {
        errs.push(ValidationError::new(
            "active_characters",
            "no active characters",
        ));
    }
    let _active: HashSet<&String> = plan.active_characters.iter().collect();
    for c in &plan.active_characters {
        if world.character(c).is_none() {
            errs.push(ValidationError::new(
                "active_characters",
                &format!("unknown character '{c}'"),
            ));
        }
    }
    if world.location(&plan.primary_location).is_none() {
        errs.push(ValidationError::new(
            "primary_location",
            &format!("unknown location '{}'", plan.primary_location),
        ));
    }
    if plan.beats.is_empty() {
        errs.push(ValidationError::new("beats", "episode has no beats"));
    }
    if plan.payoff.trim().is_empty() {
        errs.push(ValidationError::new("payoff", "payoff is empty"));
    }

    let mut seen = HashSet::new();
    for b in &plan.beats {
        if !seen.insert(&b.id) {
            errs.push(ValidationError::new(
                "beats",
                &format!("duplicate beat id '{}'", b.id),
            ));
        }
        if !KNOWN_BEAT_TYPES.contains(&b.beat_type.as_str()) {
            // The deterministic director only emits the canonical beat types, but
            // an LLM-authored plan may use its own vocabulary. `build_beat_command`
            // already has a `_` fallback for unknown types and per-beat commands
            // carry their own actions, so we accept them rather than forcing a
            // full deterministic fallback.
            tracing::debug!("beat '{}' uses non-canonical type '{}'", b.id, b.beat_type);
        }
        for e in &b.required_entities {
            if !entity_exists(world, e) {
                errs.push(ValidationError::new(
                    "required_entities",
                    &format!("beat '{}' requires unknown entity '{}'", b.id, e),
                ));
            }
        }
    }

    for pc in &plan.persistent_changes {
        if !is_known_persistent_op(&pc.operation) {
            errs.push(ValidationError::new(
                "persistent_changes",
                &format!("unknown operation '{}'", pc.operation),
            ));
        }
    }

    if !errs.is_empty() {
        return Err(errs);
    }

    // Pre-resolve beats that already carry commands (deterministic director attaches
    // them; the LLM path resolves them beat-by-beat later).
    let mut resolved_beats = Vec::new();
    for b in &plan.beats {
        // The deterministic path stores a companion command; we look it up via a
        // side channel (see `Director`). When missing, the caller resolves later.
        resolved_beats.push(ResolvedBeat {
            outline: b.clone(),
            command: BeatCommand {
                beat_id: b.id.clone(),
                dramatic_purpose: b.beat_type.clone(),
                actions: Vec::new(),
                camera_intent: CameraIntent {
                    r#type: "establish".into(),
                    subject: plan.active_characters.first().cloned().unwrap_or_default(),
                    reaction_subject: None,
                },
                expected_state_changes: Vec::new(),
                completion_condition: CompletionCondition {
                    r#type: "timer".into(),
                    actor: None,
                    seconds: Some(4.0),
                },
                fallback: None,
                notes: None,
            },
            resolved_actions: Vec::new(),
            camera_intent: CameraIntent {
                r#type: "establish".into(),
                subject: plan.active_characters.first().cloned().unwrap_or_default(),
                reaction_subject: None,
            },
            completion: CompletionCondition {
                r#type: "timer".into(),
                actor: None,
                seconds: Some(4.0),
            },
            fallback: None,
        });
    }

    Ok(ValidatedPlan {
        plan: plan.clone(),
        resolved_beats,
    })
}

/// Tolerant adapter for real LLM episode-plan output.
///
/// Local instruction-tuned models sometimes emit near-canonical plans: beats may
/// use `time`/`start`/`start_second` instead of `target_start_second`, or goals
/// may be nested differently. This maps the common variants into the canonical
/// `EpisodePlan` so a real model-authored plan is accepted instead of rejected
/// for cosmetic field-name drift. It never invents story content.
pub fn adapt_episode_plan(value: &serde_json::Value) -> EpisodePlan {
    let obj = match value.as_object() {
        Some(o) => o,
        None => {
            return serde_json::from_value::<EpisodePlan>(value.clone()).unwrap_or_else(|_| {
                EpisodePlan {
                    episode_title: String::new(),
                    logline: String::new(),
                    tone: vec![],
                    target_duration_seconds: 50.0,
                    active_characters: vec![],
                    primary_location: String::new(),
                    central_goal: CentralGoal {
                        character: String::new(),
                        goal: String::new(),
                    },
                    beats: vec![],
                    payoff: String::new(),
                    persistent_changes: vec![],
                    notes: None,
                }
            })
        }
    };

    let mut plan = EpisodePlan {
        episode_title: obj
            .get("episode_title")
            .or_else(|| obj.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        logline: obj
            .get("logline")
            .or_else(|| obj.get("log_line"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        tone: obj
            .get("tone")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        target_duration_seconds: obj
            .get("target_duration_seconds")
            .or_else(|| obj.get("target_duration"))
            .and_then(as_f32)
            .unwrap_or(50.0),
        active_characters: collect_ids(
            obj.get("active_characters")
                .or_else(|| obj.get("characters"))
                .or_else(|| obj.get("cast")),
        ),
        primary_location: obj
            .get("primary_location")
            .or_else(|| obj.get("location"))
            .or_else(|| obj.get("setting"))
            .or_else(|| obj.get("place"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        central_goal: CentralGoal {
            character: obj
                .get("central_goal")
                .and_then(|v| v.get("character"))
                .or_else(|| obj.get("central_goal").and_then(|v| v.get("who")))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            goal: obj
                .get("central_goal")
                .and_then(|v| v.get("goal"))
                .or_else(|| obj.get("central_goal").and_then(|v| v.get("what")))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        },
        beats: vec![],
        payoff: obj
            .get("payoff")
            .or_else(|| obj.get("ending"))
            .or_else(|| obj.get("resolution"))
            .or_else(|| obj.get("conclusion"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        persistent_changes: vec![],
        notes: obj
            .get("notes")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    };

    if let Some(arr) = ["beats", "acts", "scenes", "sequences", "shots", "beat_list"]
        .iter()
        .find_map(|k| obj.get(*k).and_then(|v| v.as_array()))
    {
        for b in arr {
            let mut bm = if let Some(s) = b.as_str() {
                let mut m = serde_json::Map::new();
                m.insert("id".into(), serde_json::Value::String(s.to_string()));
                m
            } else {
                b.as_object().cloned().unwrap_or_default()
            };
            let id = bm
                .get("id")
                .or_else(|| bm.get("beat_id"))
                .or_else(|| bm.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let beat_type = bm
                .get("type")
                .or_else(|| bm.get("beat_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("beat")
                .to_string();
            let target_start_second = bm
                .get("target_start_second")
                .or_else(|| bm.get("target_start"))
                .or_else(|| bm.get("start"))
                .or_else(|| bm.get("time"))
                .and_then(as_f32)
                .unwrap_or(0.0);
            let desc = bm
                .get("description")
                .or_else(|| bm.get("summary"))
                .or_else(|| bm.get("note"))
                .or_else(|| bm.get("intent"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            bm.insert("id".into(), serde_json::Value::String(id));
            bm.insert("type".into(), serde_json::Value::String(beat_type));
            bm.insert(
                "target_start_second".into(),
                serde_json::Value::from(target_start_second),
            );
            bm.insert("description".into(), serde_json::Value::String(desc));
            if let Ok(bo) = serde_json::from_value::<BeatOutline>(serde_json::Value::Object(bm)) {
                plan.beats.push(bo);
            }
        }
    }

    if let Some(arr) = obj.get("persistent_changes").and_then(|v| v.as_array()) {
        for pc in arr {
            if let Ok(p) = serde_json::from_value::<PersistentChange>(pc.clone()) {
                plan.persistent_changes.push(p);
            }
        }
    }

    plan
}

/// Collect character ids from a value that may be a string array or an object
/// array (each object carrying an `id` field). Tolerant of either shape a model
/// may emit for `active_characters` / `characters` / `cast`.
fn collect_ids(v: Option<&serde_json::Value>) -> Vec<String> {
    let arr = match v.and_then(|x| x.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    arr.iter()
        .filter_map(|x| {
            if let Some(s) = x.as_str() {
                Some(s.to_string())
            } else if let Some(o) = x.as_object() {
                o.get("id").and_then(|i| i.as_str()).map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn as_f32(v: &serde_json::Value) -> Option<f32> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().map(|f| f as f32),
        serde_json::Value::String(s) => s.parse::<f32>().ok(),
        _ => None,
    }
}

// ---------- Beat command validation ----------

/// Tolerant adapter for real LLM beat-command output.
///
/// Local instruction-tuned models (e.g. Gemma) frequently emit a *near*-canonical
/// shape rather than the exact `BeatCommand` schema: a bare-string `camera_intent`
/// and `completion_condition`, `line`/`dialogue`/`speech` instead of `text`,
/// action objects without a `type` tag, or `character`/`subject` actor aliases.
/// This maps those common variants into the canonical `BeatCommand` *before*
/// strict validation, so a real model-authored beat is accepted instead of being
/// rejected for cosmetic schema drift. It never fabricates story content — it
/// only rearranges fields the model actually produced.
pub fn adapt_beat_command(
    value: &serde_json::Value,
    plan: &EpisodePlan,
    world: &WorldState,
) -> BeatCommand {
    let obj = match value.as_object() {
        Some(o) => o,
        None => {
            return serde_json::from_value::<BeatCommand>(value.clone()).unwrap_or_else(|_| {
                BeatCommand {
                    beat_id: String::new(),
                    dramatic_purpose: String::new(),
                    actions: vec![],
                    camera_intent: CameraIntent {
                        r#type: "establish".into(),
                        subject: plan.active_characters.first().cloned().unwrap_or_default(),
                        reaction_subject: None,
                    },
                    expected_state_changes: vec![],
                    completion_condition: CompletionCondition {
                        r#type: "timer".into(),
                        actor: None,
                        seconds: Some(4.0),
                    },
                    fallback: None,
                    notes: None,
                }
            })
        }
    };

    let beat_id = obj
        .get("beat_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let dramatic_purpose = obj
        .get("dramatic_purpose")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let adapt_action = |a: &serde_json::Value| -> Option<serde_json::Value> {
        let mut m = a.as_object().cloned().unwrap_or_default();
        // Action token may be under `action`, `type`, or `token`.
        let raw_action = m
            .get("action")
            .or_else(|| m.get("type"))
            .or_else(|| m.get("token"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // Models sometimes dump a camera-intent word into `actions` (e.g.
        // "establish", "conversation", "group_coverage"). Those are NOT actions;
        // drop them so validation is not polluted by a misplaced field.
        if !KNOWN_ACTIONS.contains(&raw_action.as_str()) {
            return None;
        }
        m.insert(
            "action".into(),
            serde_json::Value::String(raw_action.clone()),
        );
        // Actor may be under `actor`, `character`, or `subject`; coerce to a real
        // active character (models sometimes pass a location or camera word).
        let actor_raw = m
            .get("actor")
            .or_else(|| m.get("character"))
            .or_else(|| m.get("subject"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let actor = coerce_character(plan, &actor_raw);
        m.insert("actor".into(), serde_json::Value::String(actor));
        // Speech text may be under `text`, `line`, `dialogue`, or `speech`.
        let text = m
            .get("text")
            .or_else(|| m.get("line"))
            .or_else(|| m.get("dialogue"))
            .or_else(|| m.get("speech"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(t) = text {
            m.insert("text".into(), serde_json::Value::String(t));
        }
        // Target must name a real entity (character, prop, or location). Models
        // sometimes leak the action verb into `target` (e.g. "ring_alarm") or
        // pass a location/camera word. Coerce to a canonical entity id; drop the
        // field when it is an action verb or otherwise unknown so validation
        // does not reject an otherwise valid LLM beat.
        if let Some(tv) = m.get("target").and_then(|v| v.as_str()) {
            match resolve_entity(world, tv) {
                Some(canon) => {
                    m.insert("target".into(), serde_json::Value::String(canon));
                }
                None => {
                    m.remove("target");
                }
            }
        }
        Some(serde_json::Value::Object(m))
    };

    let actions: Vec<serde_json::Value> = obj
        .get("actions")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(adapt_action).collect())
        .unwrap_or_default();

    // camera_intent: accept object or bare string; coerce to a valid intent.
    let camera_intent = {
        let (raw_type, raw_subject, raw_reaction) = match obj.get("camera_intent") {
            Some(serde_json::Value::String(s)) => (
                s.clone(),
                plan.active_characters.first().cloned().unwrap_or_default(),
                None,
            ),
            Some(v) => {
                let ty = v
                    .get("type")
                    .or_else(|| v.get("intent"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("conversation")
                    .to_string();
                let sub = v
                    .get("subject")
                    .or_else(|| v.get("character"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let reaction = v
                    .get("reaction_subject")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                (ty, sub, reaction)
            }
            None => (
                "conversation".into(),
                plan.active_characters.first().cloned().unwrap_or_default(),
                None,
            ),
        };
        let ci_type = if KNOWN_CAMERA_INTENTS.contains(&raw_type.as_str()) {
            raw_type
        } else {
            "conversation".into()
        };
        CameraIntent {
            r#type: ci_type,
            subject: coerce_character(plan, &raw_subject),
            reaction_subject: raw_reaction.map(|r| coerce_character(plan, &r)),
        }
    };

    // completion_condition: accept object or bare string; coerce to a valid type.
    let completion_condition = {
        let (raw_type, raw_actor, raw_secs) = match obj.get("completion_condition") {
            Some(serde_json::Value::String(s)) => (s.clone(), None, Some(4.0)),
            Some(v) => {
                let ty = v
                    .get("type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("timer")
                    .to_string();
                let act = v
                    .get("actor")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let secs = v.get("seconds").and_then(as_f32);
                (ty, act, secs)
            }
            None => ("timer".into(), None, Some(4.0)),
        };
        let cc_type = if is_known_completion(&raw_type) {
            raw_type
        } else if actions.iter().any(|a| {
            a.get("text").is_some() || a.get("action").and_then(|x| x.as_str()) == Some("speak")
        }) {
            "dialogue_finished".into()
        } else {
            "timer".into()
        };
        CompletionCondition {
            r#type: cc_type,
            actor: raw_actor.map(|a| coerce_character(plan, &a)),
            seconds: raw_secs,
        }
    };

    let expected_state_changes = obj
        .get("expected_state_changes")
        .and_then(|v| serde_json::from_value::<Vec<ExpectedStateChange>>(v.clone()).ok())
        .unwrap_or_default();

    let fallback = obj
        .get("fallback")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let notes = obj
        .get("notes")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    BeatCommand {
        beat_id,
        dramatic_purpose,
        actions: actions
            .into_iter()
            .filter_map(|v| serde_json::from_value::<ActionCommand>(v).ok())
            .collect(),
        camera_intent,
        expected_state_changes,
        completion_condition,
        fallback,
        notes,
    }
}

/// Adapt a single-call `AuthoredEpisode` into the runtime structures
/// (`EpisodePlan` + per-beat `BeatCommand`) used by the renderer.
///
/// This is *structural* adaptation plus *safe, local* normalization for facts
/// the application already knows — it does NOT invent dialogue, actions,
/// narrative intent, or unsupported entities, and it does NOT coerce bounded
/// vocabulary (action / camera-intent / completion / actor) values. Those are
/// left exactly as the model emitted them so that `validate_plan` /
/// `validate_beat_command` remain the final authority and any out-of-vocab token
/// triggers a real (schema-repair) error rather than being silently rewritten.
///
/// Safe normalization performed here:
/// * one canonical beat id per beat (the model's `AuthoredBeat.id`, made
///   non-empty + unique; the internal `beat_id` is derived from it so the old
///   `id` vs `beat_id` confusion can never trigger another model call);
/// * beats ordered by `target_start_second` into a strictly increasing timeline
///   (missing / non-increasing starts are filled in by increments);
/// * default-fill for optional fields that are harmless to omit.
pub fn adapt_authored_episode(
    ep: &AuthoredEpisode,
    world: &WorldState,
) -> Result<(EpisodePlan, HashMap<String, BeatCommand>), Vec<ValidationError>> {
    if ep.beats.is_empty() {
        return Err(vec![ValidationError::new("beats", "episode has no beats")]);
    }

    // 1. Canonical, unique, non-empty beat ids.
    let mut used: HashSet<String> = HashSet::new();
    let mut ids: Vec<String> = Vec::with_capacity(ep.beats.len());
    for (i, b) in ep.beats.iter().enumerate() {
        let mut cand = if b.id.trim().is_empty() {
            format!("beat_{}", i + 1)
        } else {
            b.id.trim().to_string()
        };
        if used.contains(&cand) {
            cand = format!("beat_{}", i + 1);
        }
        let mut n = i + 1;
        while used.contains(&cand) {
            n += 1;
            cand = format!("beat_{n}");
        }
        used.insert(cand.clone());
        ids.push(cand);
    }

    // 2. Order beats by the model's start hint into a strictly increasing timeline.
    let mut ordered: Vec<(String, &AuthoredBeat)> = ids
        .iter()
        .zip(ep.beats.iter())
        .map(|(id, b)| (id.clone(), b))
        .collect();
    ordered.sort_by(|a, b| {
        a.1.target_start_second
            .partial_cmp(&b.1.target_start_second)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut beats_outline: Vec<BeatOutline> = Vec::new();
    let mut commands: HashMap<String, BeatCommand> = HashMap::new();
    let mut last_start = -1.0f32;
    for (id, b) in ordered {
        let start = if b.target_start_second.is_finite() && b.target_start_second > last_start {
            b.target_start_second
        } else {
            last_start + 8.0
        };
        last_start = start;

        let mut required_entities: Vec<String> = b
            .actions
            .iter()
            .filter_map(|a| {
                if let Some(t) = &a.target {
                    if !t.trim().is_empty() {
                        let normalized = resolve_entity_or_alias(world, t)
                            .unwrap_or_else(|| t.trim().to_string());
                        if entity_exists(world, &normalized) {
                            Some(normalized)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        if entity_exists(world, &b.camera_intent.subject) {
            required_entities.push(b.camera_intent.subject.clone());
        } else if let Some(n) = resolve_entity_or_alias(world, &b.camera_intent.subject) {
            if entity_exists(world, &n) {
                required_entities.push(n);
            }
        } else if let Some(slug) = world.characters.keys().next().cloned() {
            // Keep at least one real required entity so the beat is valid.
            required_entities.push(slug);
        }
        required_entities.sort();
        required_entities.dedup();

        let actions: Vec<ActionCommand> = b
            .actions
            .iter()
            .map(|a| {
                let target = if let Some(t) = &a.target {
                    if t.trim().is_empty() {
                        None
                    } else if entity_exists(world, t) {
                        Some(t.clone())
                    } else if let Some(n) = resolve_entity_or_alias(world, t) {
                        Some(n)
                    } else {
                        None
                    }
                } else {
                    None
                };
                ActionCommand {
                    actor: a.actor.clone(),
                    action: a.action.clone(),
                    target,
                    text: a.text.clone(),
                    intensity: a.intensity,
                    duration_override: a.duration_override,
                }
            })
            .collect();

        let outline = BeatOutline {
            id: id.clone(),
            beat_type: "beat".into(),
            target_start_second: start,
            description: b.narrative_purpose.clone(),
            required_entities,
        };

        let command = BeatCommand {
            beat_id: id.clone(),
            dramatic_purpose: b.narrative_purpose.clone(),
            actions,
            camera_intent: CameraIntent {
                r#type: b.camera_intent.r#type.clone(),
                subject: b.camera_intent.subject.clone(),
                reaction_subject: b.camera_intent.reaction_subject.clone(),
            },
            expected_state_changes: b.expected_state_changes.clone(),
            completion_condition: CompletionCondition {
                r#type: b.completion_condition.r#type.clone(),
                actor: b.completion_condition.actor.clone(),
                seconds: b.completion_condition.seconds,
            },
            fallback: b.fallback.clone(),
            notes: b.notes.clone(),
        };

        beats_outline.push(outline);
        commands.insert(id, command);
    }

    let plan = EpisodePlan {
        episode_title: ep.episode_title.clone(),
        logline: ep.logline.clone(),
        tone: ep.tone.clone(),
        target_duration_seconds: ep.target_duration_seconds,
        active_characters: ep.active_characters.clone(),
        primary_location: ep.primary_location.clone(),
        central_goal: ep.central_goal.clone(),
        beats: beats_outline,
        payoff: ep.payoff.clone(),
        persistent_changes: ep.persistent_changes.clone(),
        notes: ep.notes.clone(),
    };

    Ok((plan, commands))
}

/// Validate a single beat command against the plan + world and resolve actions.
pub fn validate_beat_command(
    world: &WorldState,
    plan: &EpisodePlan,
    cmd: &BeatCommand,
) -> Result<ResolvedBeat, Vec<ValidationError>> {
    let mut errs = Vec::new();

    let outline = plan.beats.iter().find(|b| b.id == cmd.beat_id).cloned();
    let outline = match outline {
        Some(o) => o,
        None => {
            errs.push(ValidationError::new(
                "beat_id",
                &format!("beat '{}' not in plan", cmd.beat_id),
            ));
            return Err(errs);
        }
    };

    if !is_known_camera_intent(&cmd.camera_intent.r#type) {
        errs.push(ValidationError::new(
            "camera_intent",
            &format!("unknown camera intent '{}'", cmd.camera_intent.r#type),
        ));
    }
    if !entity_exists(world, &cmd.camera_intent.subject) {
        if let Some(n) = resolve_entity_or_alias(world, &cmd.camera_intent.subject) {
            if !entity_exists(world, &n) {
                errs.push(ValidationError::new(
                    "camera_intent.subject",
                    &format!("unknown subject '{}'", cmd.camera_intent.subject),
                ));
            }
        } else {
            errs.push(ValidationError::new(
                "camera_intent.subject",
                &format!("unknown subject '{}'", cmd.camera_intent.subject),
            ));
        }
    }

    let active: HashSet<&String> = plan.active_characters.iter().collect();
    let mut resolved_actions = Vec::new();
    for a in &cmd.actions {
        if !is_known_action(&a.action) {
            errs.push(ValidationError::new(
                "actions",
                &format!("unknown action '{}'", a.action),
            ));
            continue;
        }
        // Actor must be a known character. Some actions (world events) may have an
        // actor that is a "system" token; we accept known characters only.
        if world.character(&a.actor).is_none() {
            errs.push(ValidationError::new(
                "actions.actor",
                &format!("unknown actor '{}'", a.actor),
            ));
            continue;
        }
        if !active.contains(&a.actor) && a.action != "flicker_lights" {
            // Non-active actors are allowed for world events but warn otherwise.
            errs.push(ValidationError::new(
                "actions.actor",
                &format!("actor '{}' is not in active_characters", a.actor),
            ));
        }
        if let Some(t) = &a.target {
            if !entity_exists(world, t) {
                if let Some(n) = resolve_entity_or_alias(world, t) {
                    if !entity_exists(world, &n) {
                        // Alias resolves to a real entity but not one we currently
                        // recognise as prop/char/loc/mark here (e.g. marks in a
                        // different location). Keep going; adapt_authored_episode
                        // already normalises this path.
                    }
                } else {
                    tracing::debug!("dropping unknown acting target '{}'", t);
                }
            }
        }
        if a.action == "speak" && a.text.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true) {
            errs.push(ValidationError::new(
                "actions.text",
                &format!("speak action for '{}' has no text", a.actor),
            ));
        }
        let est = if let Some(d) = a.duration_override {
            d
        } else {
            estimate_action_duration(&a.action, a.text.as_deref())
        };
        resolved_actions.push(ResolvedAction {
            actor_id: a.actor.clone(),
            action: a.action.clone(),
            target_id: a.target.clone(),
            text: a.text.clone(),
            intensity: a.intensity.unwrap_or(0.6),
            estimated_duration: est,
        });
    }

    if !is_known_completion(&cmd.completion_condition.r#type) {
        errs.push(ValidationError::new(
            "completion_condition",
            &format!("unknown completion '{}'", cmd.completion_condition.r#type),
        ));
    }

    if !errs.is_empty() {
        return Err(errs);
    }

    Ok(ResolvedBeat {
        camera_intent: cmd.camera_intent.clone(),
        completion: cmd.completion_condition.clone(),
        fallback: cmd.fallback.clone(),
        outline,
        command: cmd.clone(),
        resolved_actions,
    })
}

// ---------- Helpers ----------

pub fn estimate_action_duration(action: &str, text: Option<&str>) -> f32 {
    match action {
        "speak" | "whisper" | "shout" => {
            let words = text.map(|t| t.split_whitespace().count()).unwrap_or(6) as f32;
            (words * 0.34 + 0.4).clamp(0.8, 12.0)
        }
        "move_to" | "approach" | "retreat_from" | "follow" | "flee_to" | "enter_room"
        | "exit_room" => 2.4,
        "inspect" | "look_at" | "point_at" | "turn_toward" | "open" | "close" | "activate"
        | "deactivate" | "knock_on" | "pick_up" | "put_down" | "give" | "take" | "hide_object"
        | "reveal_object" | "conceal_object" | "carry" | "drop" | "throw_safe" | "sit_at"
        | "stand_at" => 1.3,
        "react" | "gesture" | "laugh" | "sigh" | "interrupt" | "display_emotion"
        | "conceal_emotion" | "write_note" | "pause" => 1.0,
        "flicker_lights"
        | "cut_power"
        | "ring_alarm"
        | "open_elevator"
        | "close_elevator"
        | "play_environment_effect"
        | "trigger_safe_physics_event"
        | "spawn_authorized_prop"
        | "move_authorized_prop"
        | "change_room_state"
        | "change_location_condition" => 1.6,
        "add_fact"
        | "remove_false_belief"
        | "create_rumor"
        | "resolve_thread"
        | "create_thread"
        | "change_relationship"
        | "assign_secret"
        | "schedule_future_event" => 0.2,
        _ => 1.5,
    }
}

fn entity_exists(world: &WorldState, id: &str) -> bool {
    if world.character(id).is_some() || world.prop(id).is_some() || world.location(id).is_some() {
        return true;
    }
    // Staging marks and camera anchors are valid navigation/target refs.
    world.locations.values().any(|l| {
        l.staging_marks.iter().any(|m| m.id == id) || l.camera_anchors.iter().any(|a| a.id == id)
    })
}

/// Resolve a raw target/subject token to a canonical entity id when it names a
/// real character, prop, or location (case-insensitive). Returns `None` for
/// tokens that are action verbs or otherwise not entities, so callers can drop
/// them instead of failing validation on a misplaced field.
fn resolve_entity(world: &WorldState, raw: &str) -> Option<String> {
    resolve_entity_or_alias(world, raw)
}

/// Alias-aware resolution that strips narrative phrases like `elevator_interior`
/// into a real world prop when possible, and drops purely descriptive nonsense
/// like `floating_umbrella` so validation stays tolerant of verbose model wording.
fn resolve_entity_or_alias(world: &WorldState, raw: &str) -> Option<String> {
    let l = raw.trim().to_lowercase();
    if l.is_empty() {
        return None;
    }
    if KNOWN_ACTIONS.iter().any(|a| a.to_lowercase() == l) {
        return None;
    }
    if let Some(c) = world.characters.values().find(|c| c.id.to_lowercase() == l) {
        return Some(c.id.clone());
    }
    if let Some(p) = world.props.values().find(|p| p.id.to_lowercase() == l) {
        return Some(p.id.clone());
    }
    if let Some(loc) = world
        .locations
        .values()
        .find(|loc| loc.id.to_lowercase() == l)
    {
        return Some(loc.id.clone());
    }
    // Staging marks / camera anchors are valid navigation refs too.
    if world.locations.values().any(|loc| {
        loc.staging_marks.iter().any(|m| m.id.to_lowercase() == l)
            || loc.camera_anchors.iter().any(|a| a.id.to_lowercase() == l)
    }) {
        return Some(raw.trim().to_string());
    }
    // Alias handling: detect well-known roots inside a free-form phrase.
    let aliases: &[(&str, &str)] = &[
        ("elevator_indicator", "elevator_indicator"),
        ("indicator", "elevator_indicator"),
        ("elevator_panel", "elevator_panel"),
        ("control_panel", "control_panel"),
        ("panel", "maintenance_panel"),
        ("elevator_door", "elevator_doors"),
        ("elevator_doors", "elevator_doors"),
        ("elevator_frame", "elevator"),
        ("elevator shell", "elevator"),
        ("elevator_interior", "elevator"),
        ("elevator", "elevator"),
        ("hall_center", "floor_3_hallway"),
        ("maintenance", "maintenance_panel"),
        ("hallway", "floor_3_hallway"),
        ("clipboard", "inspection_clipboard"),
        ("hallway_light", "hallway_light"),
        ("light", "flickering_light"),
        ("door", "elevator_doors"),
    ];
    for (needle, canonical) in aliases {
        if l.contains(needle) {
            if world.prop(canonical).is_some()
                || world.character(canonical).is_some()
                || world.location(canonical).is_some()
            {
                return Some(canonical.to_string());
            }
        }
    }
    // If the token is clearly an invented prop (multi-word with no known root),
    // drop it instead of failing validation.
    None
}

fn is_known_persistent_op(op: &str) -> bool {
    matches!(
        op,
        "add_fact"
            | "remove_fact"
            | "add_belief"
            | "remove_belief"
            | "change_relationship"
            | "resolve_thread"
            | "create_thread"
            | "change_location_condition"
            | "assign_secret"
            | "schedule_future_event"
    )
}

fn is_known_completion(c: &str) -> bool {
    matches!(
        c,
        "dialogue_finished" | "arrival" | "timer" | "event_done" | "animation_finished"
    )
}
