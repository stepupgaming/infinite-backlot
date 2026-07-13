//! Validation & conversion of structured LLM output into safe internal commands.
//!
//! Per PRD §10.4 every response must be: parsed → schema-validated →
//! semantically validated → capability-checked → continuity-checked → converted
//! to internal commands. Invalid commands never enter the Bevy world.

use crate::protocol::*;
use crate::world::WorldState;
use std::collections::HashSet;

pub const KNOWN_BEAT_TYPES: &[&str] = &[
    "hook", "situation", "goal", "complication", "escalation", "reveal", "reversal",
    "payoff", "consequence",
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

pub fn validate_plan(world: &WorldState, plan: &EpisodePlan) -> Result<ValidatedPlan, Vec<ValidationError>> {
    let mut errs = Vec::new();

    if plan.active_characters.is_empty() {
        errs.push(ValidationError::new("active_characters", "no active characters"));
    }
    let _active: HashSet<&String> = plan.active_characters.iter().collect();
    for c in &plan.active_characters {
        if world.character(c).is_none() {
            errs.push(ValidationError::new("active_characters", &format!("unknown character '{c}'")));
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
            errs.push(ValidationError::new("beats", &format!("duplicate beat id '{}'", b.id)));
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

// ---------- Beat command validation ----------

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
        errs.push(ValidationError::new(
            "camera_intent.subject",
            &format!("unknown subject '{}'", cmd.camera_intent.subject),
        ));
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
                errs.push(ValidationError::new(
                    "actions.target",
                    &format!("unknown target '{t}'"),
                ));
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
        "inspect" | "look_at" | "point_at" | "turn_toward" | "open" | "close"
        | "activate" | "deactivate" | "knock_on" | "pick_up" | "put_down" | "give"
        | "take" | "hide_object" | "reveal_object" | "conceal_object" | "carry"
        | "drop" | "throw_safe" | "sit_at" | "stand_at" => 1.3,
        "react" | "gesture" | "laugh" | "sigh" | "interrupt" | "display_emotion"
        | "conceal_emotion" | "write_note" | "pause" => 1.0,
        "flicker_lights" | "cut_power" | "ring_alarm" | "open_elevator"
        | "close_elevator" | "play_environment_effect" | "trigger_safe_physics_event"
        | "spawn_authorized_prop" | "move_authorized_prop" | "change_room_state"
        | "change_location_condition" => 1.6,
        "add_fact" | "remove_false_belief" | "create_rumor" | "resolve_thread"
        | "create_thread" | "change_relationship" | "assign_secret"
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
    matches!(c, "dialogue_finished" | "arrival" | "timer" | "event_done" | "animation_finished")
}
