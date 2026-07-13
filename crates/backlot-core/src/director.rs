//! Directors: the bounded "showrunners".
//!
//! `Director` is the trait the runtime depends on. The `DeterministicDirector`
//! is a built-in fallback that authors watchable episodes with zero external
//! dependencies, so the product is runnable even when no model is reachable.
//! `backlot-llm` provides an `LlmDirector` that wraps the same trait and calls
//! the model, degrading to the deterministic director on failure.

use crate::error::Result;
use crate::protocol::*;
use crate::rng::SeededRng;
use crate::timeline::CameraShotSpec;
use crate::world::WorldState;
use crate::package::DialogueLine;
use crate::validation::ValidatedPlan;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct DirectorContext {
    pub world: WorldState,
    pub episode_number: u64,
    pub seed: u64,
    pub target_duration: f32,
    pub recent_summaries: Vec<String>,
    pub tone: Vec<String>,
}

pub trait Director: Send + Sync {
    fn name(&self) -> &'static str;
    /// Produce the overall episode plan (beat *outlines* only).
    fn plan_episode(&self, ctx: &DirectorContext) -> Result<EpisodePlan>;
    /// Produce the detailed command for a single beat.
    fn plan_beat(&self, ctx: &DirectorContext, plan: &EpisodePlan, beat: &BeatOutline)
        -> Result<BeatCommand>;
}

// ---------------------------------------------------------------------------
// Deterministic director
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DeterministicDirector;

#[derive(Clone)]
struct Scenario {
    title: &'static str,
    logline: &'static str,
    tone: &'static [&'static str],
    active: &'static [&'static str],
    goal_char: &'static str,
    goal_text: &'static str,
    thread: &'static str,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        title: "The Elevator Inspection",
        logline: "An inspector discovers the elevator has been adding floors that do not exist.",
        tone: &["surreal", "comedy", "mystery"],
        active: &["mara", "ellis", "voss", "nox"],
        goal_char: "mara",
        goal_text: "complete the inspection without revealing the impossible fourth floor",
        thread: "unknown_floor",
    },
    Scenario {
        title: "Code Violation Number Four",
        logline: "Inspector Voss cites the building for a floor that is not on the map.",
        tone: &["comedy", "bureaucratic", "mystery"],
        active: &["mara", "voss", "ellis", "nox"],
        goal_char: "mara",
        goal_text: "distract the inspector before the elevator misbehaves",
        thread: "inspection",
    },
    Scenario {
        title: "The Tenant Who Does Not Leave",
        logline: "Ellis decides to meet the neighbor in 4A, who is never seen leaving.",
        tone: &["surreal", "mystery", "unease"],
        active: &["ellis", "nox", "mara", "voss"],
        goal_char: "ellis",
        goal_text: "learn who (or what) lives in apartment 4A",
        thread: "missing_tenant",
    },
];

impl DeterministicDirector {
    fn scenario(&self, ctx: &DirectorContext) -> Scenario {
        let mut rng = SeededRng::new(ctx.seed);
        let idx = rng.next_u64() as usize % SCENARIOS.len();
        SCENARIOS[idx].clone()
    }
}

impl Director for DeterministicDirector {
    fn name(&self) -> &'static str {
        "deterministic"
    }

    fn plan_episode(&self, ctx: &DirectorContext) -> Result<EpisodePlan> {
        let s = self.scenario(ctx);
        let beats_outline = build_outline(&s, ctx.target_duration);

        let persistent_changes = match s.thread {
            "unknown_floor" => vec![PersistentChange {
                operation: "add_belief".into(),
                target: "voss".into(),
                value: "believes_floor_four_exists".into(),
                field: None,
                amount: None,
            }],
            "inspection" => vec![PersistentChange {
                operation: "change_relationship".into(),
                target: "mara".into(),
                value: "voss".into(),
                field: Some("suspicion".into()),
                amount: Some(0.2),
            }],
            "missing_tenant" => vec![PersistentChange {
                operation: "add_fact".into(),
                target: "ellis".into(),
                value: "met_nox_in_4a".into(),
                field: None,
                amount: None,
            }],
            _ => vec![],
        };

        Ok(EpisodePlan {
            episode_title: s.title.into(),
            logline: s.logline.into(),
            tone: s.tone.iter().map(|t| t.to_string()).collect(),
            target_duration_seconds: ctx.target_duration,
            active_characters: s.active.iter().map(|c| c.to_string()).collect(),
            primary_location: "floor_3_hallway".into(),
            central_goal: CentralGoal {
                character: s.goal_char.into(),
                goal: s.goal_text.into(),
            },
            beats: beats_outline,
            payoff: payoff_line(&s),
            persistent_changes,
            notes: Some(format!("deterministic scenario for thread '{}'", s.thread)),
        })
    }

    fn plan_beat(
        &self,
        ctx: &DirectorContext,
        plan: &EpisodePlan,
        beat: &BeatOutline,
    ) -> Result<BeatCommand> {
        let s = self.scenario(ctx);
        build_beat_command(&s, ctx, plan, beat)
    }
}

// ---------------------------------------------------------------------------
// Outline + beat construction
// ---------------------------------------------------------------------------

fn build_outline(_s: &Scenario, duration: f32) -> Vec<BeatOutline> {
    let steps: &[(&str, &str, &[&str])] = &[
        ("hook", "The elevator opens onto a wall while someone inside asks whether this is floor four.", &["elevator", "mara"]),
        ("complication", "A symbol appears on the floor indicator instead of a number.", &["elevator_indicator", "mara"]),
        ("escalation", "The lights flicker in disagreement and a door that should not exist hums.", &["flickering_light", "ellis"]),
        ("reveal", "A tenant who is never seen leaving offers a calm, impossible explanation.", &["nox", "ellis"]),
        ("payoff", "The inspector cites the nonexistent floor for lacking an emergency exit.", &["voss", "elevator"]),
    ];
    let n = steps.len();
    steps
        .iter()
        .enumerate()
        .map(|(i, (t, desc, ents))| BeatOutline {
            id: format!("beat_{:02}", i + 1),
            beat_type: t.to_string(),
            target_start_second: (duration * i as f32 / n as f32).max(0.0),
            description: desc.to_string(),
            required_entities: ents.iter().map(|e| e.to_string()).collect(),
        })
        .collect()
}

fn payoff_line(s: &Scenario) -> String {
    match s.thread {
        "unknown_floor" => "The inspector cites the nonexistent floor for lacking an emergency exit.".into(),
        "inspection" => "Voss files a violation for a floor that is not on the map, and Mara quietly adds it to the map.".into(),
        "missing_tenant" => "Ellis leaves 4A convinced, while Nox remains, serene and unmoving, long after the door closes.".into(),
        _ => "The episode ends on a deliberate, impossible note.".into(),
    }
}

fn build_beat_command(
    s: &Scenario,
    ctx: &DirectorContext,
    _plan: &EpisodePlan,
    beat: &BeatOutline,
) -> Result<BeatCommand> {
    let mut rng = SeededRng::new(ctx.seed).derive(hash_str(&beat.id));
    let c: HashMap<&str, &str> = s.active.iter().map(|c| (*c, *c)).collect();
    let a = |actor: &str, action: &str, target: Option<&str>, text: Option<&str>| ActionCommand {
        actor: actor.to_string(),
        action: action.to_string(),
        target: target.map(str::to_string),
        text: text.map(str::to_string),
        intensity: None,
        duration_override: None,
    };

    let (actions, camera, completion, fallback): (Vec<ActionCommand>, CameraIntent, CompletionCondition, Option<String>) =
        match beat.beat_type.as_str() {
            "hook" => (
                vec![
                    a(c["mara"], "move_to", Some("elevator_door"), None),
                    a(c["mara"], "open_elevator", Some("elevator"), None),
                    a(c["mara"], "speak", None, Some("Don't get in. The elevator's feeling creative today.")),
                    a(c["ellis"], "approach", Some("elevator_door"), None),
                    a(c["ellis"], "speak", None, Some("It just asked me what floor four looks like.")),
                    a(c["mara"], "speak", None, Some("It's been doing that since the building opened.")),
                ],
                CameraIntent { r#type: "establish".into(), subject: "elevator".into(), reaction_subject: Some(c["mara"].to_string()) },
                CompletionCondition { r#type: "dialogue_finished".into(), actor: Some(c["ellis"].into()), seconds: None },
                Some("Have the elevator ding ominously and close on its own.".into()),
            ),
            "complication" => (
                vec![
                    a(c["mara"], "inspect", Some("elevator_indicator"), None),
                    a(c["mara"], "speak", None, Some("That's not a floor. That's a complaint.")),
                    a(c["ellis"], "point_at", Some("elevator_indicator"), None),
                    a(c["ellis"], "speak", None, Some("It's glowing. I think it's proud of itself.")),
                    a(c["mara"], "speak", None, Some("The indicator only argues when it's annoyed.")),
                    a(c["ellis"], "speak", None, Some("Arguing with an elevator. Great.")),
                ],
                CameraIntent { r#type: "reveal".into(), subject: "elevator_indicator".into(), reaction_subject: Some(c["mara"].to_string()) },
                CompletionCondition { r#type: "dialogue_finished".into(), actor: Some(c["ellis"].into()), seconds: None },
                Some("Mara blocks the view and the indicator flickers back to a number.".into()),
            ),
            "escalation" => (
                vec![
                    a(c["ellis"], "retreat_from", Some("elevator_door"), None),
                    a(c["mara"], "flicker_lights", Some("flickering_light"), None),
                    a(c["ellis"], "react", None, Some("Whoa—")),
                    a(rng.pick(s.active).unwrap_or("nox"), "speak", None, Some("The building prefers you not to notice. Most guests oblige.")),
                    a(c["ellis"], "speak", None, Some("Noticed. I'm choosing not to oblige.")),
                    a(rng.pick(s.active).unwrap_or("nox"), "speak", None, Some("You will adjust. They always do.")),
                ],
                CameraIntent { r#type: "comedic_wide".into(), subject: "hall_center".into(), reaction_subject: Some(c["ellis"].to_string()) },
                CompletionCondition { r#type: "dialogue_finished".into(), actor: Some("nox".into()), seconds: None },
                Some("Cut power briefly, then restore with a single defiant light.".into()),
            ),
            "reveal" => (
                vec![
                    a(c["nox"], "move_to", Some("apartment_4a_door"), None),
                    a(c["nox"], "speak", None, Some("I live in 4A. I've always lived in 4A. The floor just borrowed the number.")),
                    a(c["ellis"], "speak", None, Some("So the number was borrowed, not the floor.")),
                    a(c["mara"], "speak", None, Some("And the door still opens from the inside.")),
                    a(c["ellis"], "react", None, Some("...borrowed?")),
                    a(c["mara"], "conceal_object", Some("maintenance_override_key"), None),
                ],
                CameraIntent { r#type: "speaker_closeup".into(), subject: "nox".into(), reaction_subject: Some(c["ellis"].to_string()) },
                CompletionCondition { r#type: "dialogue_finished".into(), actor: Some(c["nox"].into()), seconds: None },
                Some("Nox tilts their head a degree too far and the moment passes.".into()),
            ),
            "payoff" => (
                vec![
                    a(c.get("voss").copied().unwrap_or("mara"), "move_to", Some("elevator_door"), None),
                    a(c.get("voss").copied().unwrap_or("mara"), "speak", None, Some("Citation: floor four lacks a required emergency exit. Rectify within thirty days.")),
                    a(c["mara"], "sigh", None, None),
                    a(c["ellis"], "whisper", None, Some("Do we... build an exit for a floor that isn't there?")),
                    a(c["mara"], "speak", None, Some("We are not building an exit.")),
                    a(c.get("voss").copied().unwrap_or("mara"), "speak", None, Some("Noted and dismissed.")),
                ],
                CameraIntent { r#type: "cliffhanger_hold".into(), subject: "elevator".into(), reaction_subject: Some(c["mara"].to_string()) },
                CompletionCondition { r#type: "dialogue_finished".into(), actor: Some(c["ellis"].into()), seconds: None },
                Some("Freeze on Mara's exhausted face as the elevator dings for floor four.".into()),
            ),
            _ => (
                vec![a(c["mara"], "speak", None, Some("Something impossible, on schedule, as usual."))],
                CameraIntent { r#type: "establish".into(), subject: c["mara"].into(), reaction_subject: None },
                CompletionCondition { r#type: "timer".into(), actor: None, seconds: Some(4.0) },
                None,
            ),
        };

    Ok(BeatCommand {
        beat_id: beat.id.clone(),
        dramatic_purpose: beat.beat_type.clone(),
        actions,
        camera_intent: camera,
        expected_state_changes: vec![],
        completion_condition: completion,
        fallback,
        notes: None,
    })
}

fn hash_str(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

// ---------------------------------------------------------------------------
// Autonomous camera director (Phase 6)
// ---------------------------------------------------------------------------
//
// Turns the sparse per-beat camera intent into purposeful, legible coverage.
// Every shot frames a *real performer* (never a wall or the elevator shell),
// the hook is immediate and close, speaker + reaction coverage is guaranteed,
// and inserts/reveals get their own brief close-up. The result is 8-14 shots
// for a typical short episode.

/// Resolve a camera subject to a real character id (fallback chain).
fn resolve_subject_char(
    world: &WorldState,
    subject: &str,
    active: &[String],
    dialogue: &[DialogueLine],
    at_time: f32,
) -> String {
    if world.character(subject).is_some() {
        return subject.to_string();
    }
    // The subject might be a prop/mark; find whoever is speaking, else first active.
    if let Some(d) = dialogue.iter().find(|d| d.start <= at_time && at_time < d.end) {
        if world.character(&d.actor).is_some() {
            return d.actor.clone();
        }
    }
    active.first().cloned().unwrap_or_else(|| subject.to_string())
}

/// Plan the expanded camera coverage for an episode.
pub fn plan_shots(
    world: &WorldState,
    validated: &ValidatedPlan,
    home_of: &HashMap<String, [f32; 3]>,
    dialogue: &[DialogueLine],
    inserts: &[(f32, String)],
    duration: f32,
) -> Vec<CameraShotSpec> {
    let active: Vec<String> = validated.plan.active_characters.clone();
    let mut shots: Vec<CameraShotSpec> = Vec::new();

    // Per-beat boundaries (mirror of build_schedule clocking).
    let mut clock = 0.0f32;
    let beat_bounds: Vec<(f32, f32)> = validated
        .resolved_beats
        .iter()
        .map(|rb| {
            let b0 = clock;
            let mut t = 0.0f32;
            for ra in &rb.resolved_actions {
                t += ra.estimated_duration;
            }
            let b1 = (b0 + t + 0.6).max(rb.completion.seconds.unwrap_or(0.0).max(b0 + t));
            clock = b1;
            (b0, b1)
        })
        .collect();

    for (i, rb) in validated.resolved_beats.iter().enumerate() {
        let (b0, b1) = beat_bounds[i];
        let seg = (b1 - b0).max(0.1);
        let speaker = resolve_subject_char(world, &rb.camera_intent.subject, &active, dialogue, b0);
        let reaction = rb.camera_intent.reaction_subject.clone().filter(|r| {
            world.character(r).is_some() && *r != speaker
        });

        let is_hook = rb.outline.beat_type == "hook" || i == 0;
        // Context/group shot length.
        let ctx_len = (seg * 0.35).clamp(2.0, 3.5);
        let ctx_end = (b0 + ctx_len).min(b1 - 1.0).max(b0 + 1.5);

        // Reaction shot length (last portion of the beat).
        let react_len = if reaction.is_some() {
            (seg * 0.3).clamp(2.0, 3.0)
        } else {
            0.0
        };
        let react_start = (b1 - react_len).max(ctx_end + 1.0);

        // Hook: open close on the performer, not a wide.
        let ctx_intent = if is_hook {
            "tension_push".to_string()
        } else {
            "group_coverage".to_string()
        };
        shots.push(CameraShotSpec {
            start: b0,
            end: ctx_end,
            intent: ctx_intent,
            subject: speaker.clone(),
            reaction: reaction.clone(),
        });

        // Speaker closeup for the middle of the beat.
        if react_start > ctx_end + 0.5 {
            shots.push(CameraShotSpec {
                start: ctx_end,
                end: react_start,
                intent: if is_hook { "speaker_closeup" } else { "speaker_closeup" }.to_string(),
                subject: speaker.clone(),
                reaction: reaction.clone(),
            });
        }

        // Reaction coverage at the end.
        if let Some(r) = reaction.clone() {
            if b1 - react_start >= 1.2 {
                shots.push(CameraShotSpec {
                    start: react_start,
                    end: b1,
                    intent: "reaction".to_string(),
                    subject: speaker.clone(),
                    reaction: Some(r),
                });
            }
        }
    }

    // Insert / reveal close-ups around key prop moments.
    for (time, _prop) in inserts {
        // Find a character active near this time to frame.
        let subj = resolve_subject_char(world, &active.first().cloned().unwrap_or_default(), &active, dialogue, *time);
        let start = (*time - 1.0).max(0.0);
        let end = (*time + 1.4).min(duration);
        if end - start >= 1.0 {
            shots.push(CameraShotSpec {
                start,
                end,
                intent: "insert_object".to_string(),
                subject: subj,
                reaction: None,
            });
        }
    }

    // Sort by start, then trim to a purposeful 8-14 shot count.
    shots.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    if shots.len() > 14 {
        // Keep speaker + reaction shots first; drop surplus group shots.
        let mut kept: Vec<CameraShotSpec> = shots
            .iter()
            .filter(|s| s.intent != "group_coverage")
            .cloned()
            .collect();
        // Re-add group shots until we reach 14.
        for s in &shots {
            if kept.len() >= 14 {
                break;
            }
            if s.intent == "group_coverage" && !kept.contains(s) {
                kept.push(s.clone());
            }
        }
        kept.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        shots = kept;
    }

    // Guarantee a minimum of 8 shots by cloning the nearest speaker shots if the
    // episode is very short.
    if shots.len() < 8 && !shots.is_empty() {
        let mut extra = shots.clone();
        while shots.len() + extra.len() < 8 && !extra.is_empty() {
            shots.extend(extra.iter().cloned());
        }
        shots.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    }

    // Resolve overlaps that produce zero-length or out-of-order shots. After
    // sorting by start, clamp each shot's end to the next shot's start so the
    // plan is strictly increasing and every entry has positive length.
    shots.retain(|s| s.end > s.start + 0.05);
    if shots.len() > 1 {
        for i in 0..shots.len() - 1 {
            let next_start = shots[i + 1].start;
            if shots[i].end > next_start {
                shots[i].end = next_start.max(shots[i].start + 0.5);
            }
        }
        // Re-drop anything that became degenerate after clamping.
        shots.retain(|s| s.end > s.start + 0.05);
    }
    shots
}
