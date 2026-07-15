//! Shared, engine-agnostic episode timeline.
//!
//! This is the **single authoritative representation** of an episode's committed
//! motion. Both the offline CPU renderer (`render.rs`) and the real Bevy renderer
//! (`backlot-app`) consume the same `Schedule` and `evaluate_at`, so there is no
//! second, divergent interpretation of the world. The Bevy renderer merely draws
//! the `FrameState` this module produces; it never re-derives scene state.

use crate::avatar::{character_pose, CameraTargetRole, HumanoidRig, PerformanceState, Pose, Xform};
use crate::package::{Caption, DialogueLine, TimedEvent};
use crate::protocol::{ActionPhases, EnvironmentEventKind, PerformanceCue, SoundCue};
use crate::validation::ValidatedPlan;
use crate::world::WorldState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ===========================================================================
// Schedule (deterministic timeline)
// ===========================================================================

#[derive(Debug, Clone)]
pub struct ScheduledAction {
    pub actor: String,
    pub action: String,
    pub target: Option<String>,
    /// Authoritative world-space destination captured by the occupancy planner.
    /// Dynamic near-actor fallback slots are not part of the static set manifest,
    /// so resolving only by slot name during playback would silently drop them.
    pub target_position: Option<[f32; 3]>,
    pub text: Option<String>,
    pub start: f32,
    pub dur: f32,
    pub performance: Option<PerformanceCue>,
}

#[derive(Debug, Clone)]
pub struct CharTrack {
    pub id: String,
    pub home: [f32; 3],
    pub actions: Vec<ScheduledAction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraShotSpec {
    pub start: f32,
    pub end: f32,
    pub intent: String,
    pub subject: String,
    pub reaction: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PropAttach {
    pub prop: String,
    pub char_id: String,
    pub start: f32,
    pub end: f32,
}

#[derive(Debug, Clone)]
pub struct ScheduledEnvironmentCue {
    pub target: String,
    pub event: EnvironmentEventKind,
    pub start: f32,
    pub duration: f32,
    pub from: Option<f32>,
    pub to: Option<f32>,
    pub value: Option<String>,
    pub easing: String,
}

#[derive(Debug, Clone)]
pub struct ScheduledSoundCue {
    pub cue: SoundCue,
    pub start: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovementResolution {
    pub actor: String,
    pub action: String,
    pub requested_destination: Option<String>,
    pub resolved_destination: Option<String>,
    pub start: f32,
    pub end: f32,
    pub path: Vec<[f32; 3]>,
    pub executed: bool,
    pub unresolved_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableBlockingAction {
    pub actor: String,
    pub starting_slot: String,
    pub destination_slot: String,
    pub travel_path: Vec<[f32; 3]>,
    pub facing_target: Option<String>,
    pub arrival_time: f32,
    pub action_after_arrival: Option<String>,
    pub required_camera_visible_moment: f32,
}

#[derive(Debug, Clone)]
pub struct Schedule {
    pub duration: f32,
    pub characters: Vec<CharTrack>,
    pub camera_shots: Vec<CameraShotSpec>,
    pub dialogue: Vec<DialogueLine>,
    pub captions: Vec<Caption>,
    pub events: Vec<TimedEvent>,
    pub flicker: Vec<(f32, f32)>,
    pub prop_attach: Vec<PropAttach>,
    /// Insert markers (prop reveals, elevator indicators) for the director.
    pub inserts: Vec<(f32, String)>,
    pub environment: Vec<ScheduledEnvironmentCue>,
    pub sounds: Vec<ScheduledSoundCue>,
    pub movement_resolutions: Vec<MovementResolution>,
    pub blocking_plan: Vec<ExecutableBlockingAction>,
}

#[derive(Debug, Clone)]
pub enum ActionKind {
    Move,
    Speak,
    React,
    Gesture,
    Point,
    Look,
    Listen,
    Interact,
    Environment,
    Narrative,
    Unknown,
}

pub fn action_kind(a: &str) -> ActionKind {
    match a {
        "move_to" | "approach" | "retreat_from" | "follow" | "flee_to" | "enter_room"
        | "exit_room" => ActionKind::Move,
        "speak" | "whisper" | "shout" => ActionKind::Speak,
        "react" | "laugh" | "sigh" | "display_emotion" | "conceal_emotion" | "interrupt" => {
            ActionKind::React
        }
        "gesture" => ActionKind::Gesture,
        "point_at" => ActionKind::Point,
        "look_at" | "turn_toward" => ActionKind::Look,
        "pause" => ActionKind::Listen,
        "pick_up" | "put_down" | "give" | "take" | "inspect" | "open" | "close" | "activate"
        | "deactivate" | "hide_object" | "reveal_object" | "carry" | "drop" | "throw_safe"
        | "knock_on" | "conceal_object" | "sit_at" | "stand_at" | "write_note" => {
            ActionKind::Interact
        }
        "flicker_lights"
        | "cut_power"
        | "ring_alarm"
        | "open_elevator"
        | "close_elevator"
        | "spawn_authorized_prop"
        | "move_authorized_prop"
        | "change_room_state"
        | "play_environment_effect"
        | "trigger_safe_physics_event"
        | "change_location_condition" => ActionKind::Environment,
        "add_fact"
        | "remove_false_belief"
        | "create_rumor"
        | "resolve_thread"
        | "create_thread"
        | "change_relationship"
        | "assign_secret"
        | "schedule_future_event" => ActionKind::Narrative,
        _ => ActionKind::Unknown,
    }
}

pub fn action_phase_weight(phases: ActionPhases, normalized_time: f32) -> f32 {
    let total =
        (phases.anticipation + phases.execution + phases.hold + phases.recovery).max(0.0001);
    let t = normalized_time.clamp(0.0, 1.0) * total;
    let a = phases.anticipation.max(0.0);
    let x = phases.execution.max(0.0);
    let h = phases.hold.max(0.0);
    let r = phases.recovery.max(0.0);
    if a > 0.0 && t < a {
        return smoothstep(t / a);
    }
    if t < a + x + h {
        return 1.0;
    }
    if r > 0.0 && t < a + x + h + r {
        return 1.0 - smoothstep((t - a - x - h) / r);
    }
    0.0
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Resolve a target id to a static world position (marks, character homes, or
/// prop home marks). Deterministic and resolution-independent.
pub fn resolve_pos(
    target: &str,
    world: &WorldState,
    home_of: &HashMap<String, [f32; 3]>,
) -> Option<[f32; 3]> {
    if let Some(position) = crate::stage::slot_position(target) {
        return Some(position);
    }
    if let Some(position) = crate::stage::feature_position(target) {
        return Some(position);
    }
    // staging mark
    for l in world.locations.values() {
        if let Some(m) = l.staging_marks.iter().find(|m| m.id == target) {
            return Some(m.position);
        }
    }
    // character home
    if let Some(p) = home_of.get(target) {
        return Some(*p);
    }
    // prop home mark
    if let Some(p) = world.props.get(target) {
        for l in world.locations.values() {
            if let Some(m) = l.staging_marks.iter().find(|m| m.id == p.home_mark) {
                return Some(m.position);
            }
        }
    }
    None
}

/// Build the deterministic schedule from a validated plan + measured TTS
/// durations. Movement timing is estimated; dialogue timing uses the *measured*
/// audio duration so lips/audio stay in sync.
///
/// The camera plan is expanded by the autonomous director (see
/// `director::plan_shots`) so that a short episode still gets purposeful,
/// legible coverage rather than one shot per beat.
pub fn build_schedule(
    world: &WorldState,
    validated: &ValidatedPlan,
    tts_durations: &HashMap<(String, String), f32>,
) -> Schedule {
    use crate::director::plan_shots;
    let active: Vec<&String> = validated.plan.active_characters.iter().collect();
    let mut occupancy =
        crate::stage::StageOccupancy::for_episode(active.iter().map(|actor| actor.as_str()));
    let mut home_of: HashMap<String, [f32; 3]> = HashMap::new();
    for id in &active {
        let pos = occupancy
            .current(id)
            .map(|reservation| reservation.position)
            .unwrap_or([0.0, 0.0, 0.0]);
        home_of.insert((*id).clone(), pos);
    }

    let mut chars: Vec<CharTrack> = active
        .iter()
        .map(|id| CharTrack {
            id: (*id).clone(),
            home: home_of.get(*id).copied().unwrap_or([0.0; 3]),
            actions: Vec::new(),
        })
        .collect();

    let mut dialogue: Vec<DialogueLine> = Vec::new();
    let mut captions: Vec<Caption> = Vec::new();
    let mut events: Vec<TimedEvent> = Vec::new();
    let mut flicker: Vec<(f32, f32)> = Vec::new();
    let mut prop_attach: Vec<PropAttach> = Vec::new();
    let mut inserts: Vec<(f32, String)> = Vec::new();
    let mut environment: Vec<ScheduledEnvironmentCue> = Vec::new();
    let mut sounds: Vec<ScheduledSoundCue> = Vec::new();
    let mut camera_shots: Vec<CameraShotSpec> = Vec::new();
    let mut movement_resolutions = Vec::new();
    let mut blocking_plan = Vec::new();
    // Long-form authored beats commonly place direction (look, react, point,
    // interact) immediately before dialogue. Those directions are simultaneous
    // performance, not silent timeline blocks. Short proof fixtures remain
    // strictly sequential so their explicit choreography timestamps are stable.
    let overlap_nonblocking_actions = validated.plan.target_duration_seconds >= 45.0;

    let mut clock = 0.0f32;
    for rb in &validated.resolved_beats {
        let beat_start = clock;
        let direction_text = format!(
            "{} {} {} {}",
            rb.outline.description,
            rb.command.dramatic_purpose,
            rb.command.visible_action.as_deref().unwrap_or(""),
            rb.command.intended_reaction.as_deref().unwrap_or("")
        )
        .to_ascii_lowercase();
        let opens_elevator = (direction_text.contains("doors open")
            || direction_text.contains("doors slide open"))
            && !direction_text.contains("slam shut");
        let closes_elevator = direction_text.contains("doors close")
            || direction_text.contains("close violently")
            || direction_text.contains("slam shut");
        let mut t = 0.0f32;
        let mut movement_t = 0.0f32;
        let mut movement_ready_by_actor: HashMap<String, f32> = HashMap::new();
        let mut visible_end = 0.0f32;
        for ra in &rb.resolved_actions {
            let kind = action_kind(&ra.action);
            let dur = match kind {
                ActionKind::Speak => {
                    let key = (ra.actor_id.clone(), ra.text.clone().unwrap_or_default());
                    *tts_durations.get(&key).unwrap_or(&ra.estimated_duration)
                }
                _ => ra.estimated_duration,
            };
            let action_t = if overlap_nonblocking_actions && matches!(kind, ActionKind::Move) {
                movement_t
            } else if overlap_nonblocking_actions && !matches!(kind, ActionKind::Speak) {
                // A direction written after dialogue is a listener/reaction beat:
                // place it under the end of that line rather than adding silent
                // tail time after the actor stops speaking.
                (t - dur)
                    .max(0.0)
                    .max(*movement_ready_by_actor.get(&ra.actor_id).unwrap_or(&0.0))
            } else {
                t
            };
            let action_start = beat_start + action_t;
            let mut resolved_target = ra.target_id.clone();
            let mut resolved_target_position = None;
            if matches!(action_kind(&ra.action), ActionKind::Move) {
                let start_reservation = occupancy.current(&ra.actor_id).cloned();
                let movement_kind = match ra.action.as_str() {
                    "approach" | "follow" | "retreat_from" => crate::stage::MovementKind::Approach,
                    "enter_room" => crate::stage::MovementKind::Enter,
                    "exit_room" | "flee_to" => crate::stage::MovementKind::Exit,
                    _ => crate::stage::MovementKind::Move,
                };
                let requested = ra.target_id.as_deref().unwrap_or("");
                let reservation = if requested.is_empty() {
                    Err("movement has no requested destination".to_string())
                } else {
                    occupancy.reserve(&ra.actor_id, requested, movement_kind)
                };
                match reservation {
                    Ok(reservation) => {
                        resolved_target = Some(reservation.slot.clone());
                        resolved_target_position = Some(reservation.position);
                        let start_position = start_reservation
                            .as_ref()
                            .map(|value| value.position)
                            .unwrap_or(reservation.position);
                        movement_resolutions.push(MovementResolution {
                            actor: ra.actor_id.clone(),
                            action: ra.action.clone(),
                            requested_destination: ra.target_id.clone(),
                            resolved_destination: Some(reservation.slot.clone()),
                            start: action_start,
                            end: action_start + dur,
                            path: vec![start_position, reservation.position],
                            executed: true,
                            unresolved_reason: None,
                        });
                        blocking_plan.push(ExecutableBlockingAction {
                            actor: ra.actor_id.clone(),
                            starting_slot: start_reservation
                                .map(|value| value.slot)
                                .unwrap_or_else(|| "unknown".into()),
                            destination_slot: reservation.slot,
                            travel_path: vec![start_position, reservation.position],
                            facing_target: ra.target_id.clone(),
                            arrival_time: action_start + dur,
                            action_after_arrival: None,
                            required_camera_visible_moment: action_start + dur * 0.5,
                        });
                    }
                    Err(reason) => movement_resolutions.push(MovementResolution {
                        actor: ra.actor_id.clone(),
                        action: ra.action.clone(),
                        requested_destination: ra.target_id.clone(),
                        resolved_destination: None,
                        start: action_start,
                        end: action_start + dur,
                        path: vec![],
                        executed: false,
                        unresolved_reason: Some(reason),
                    }),
                }
            }
            // record on the actor track
            visible_end = visible_end.max(action_t + dur);
            if let Some(ct) = chars.iter_mut().find(|c| c.id == ra.actor_id) {
                ct.actions.push(ScheduledAction {
                    actor: ra.actor_id.clone(),
                    action: ra.action.clone(),
                    target: resolved_target.clone(),
                    target_position: resolved_target_position,
                    text: ra.text.clone(),
                    start: action_start,
                    dur,
                    performance: ra.performance.clone(),
                });
            }
            let interaction_sound = |sound: &str, gain: f32, start: f32| ScheduledSoundCue {
                cue: SoundCue {
                    sound: sound.into(),
                    source: None,
                    gain,
                    start_offset: 0.0,
                },
                start,
            };
            match ra.action.as_str() {
                "activate" if ra.target_id.as_deref() == Some("maintenance_panel") => {
                    sounds.push(interaction_sound(
                        "panel_beep",
                        0.36,
                        action_start + dur * 0.55,
                    ));
                }
                "open_elevator" => {
                    sounds.push(interaction_sound("elevator_ding", 0.42, action_start));
                    sounds.push(interaction_sound("door_motor", 0.28, action_start + 0.15));
                }
                "close_elevator" => {
                    sounds.push(interaction_sound("door_motor", 0.26, action_start));
                }
                "flicker_lights" => {
                    sounds.push(interaction_sound("electrical_flicker", 0.2, action_start));
                }
                _ => {}
            }
            // dialogue + captions for speech
            if matches!(action_kind(&ra.action), ActionKind::Speak) {
                let voice = world
                    .character(&ra.actor_id)
                    .map(|c| c.voice_id.clone())
                    .unwrap_or_else(|| ra.actor_id.clone());
                let s = action_start;
                let e = s + dur;
                let text = ra.text.clone().unwrap_or_default();
                dialogue.push(DialogueLine {
                    start: s,
                    end: e,
                    actor: ra.actor_id.clone(),
                    text: text.clone(),
                    voice_id: voice.clone(),
                });
                captions.extend(caption_phrases(&text, s, e));
            }
            // events log
            events.push(TimedEvent {
                t: action_start,
                kind: ra.action.clone(),
                actor: Some(ra.actor_id.clone()),
                target: resolved_target.clone(),
                detail: ra.text.clone().unwrap_or_default(),
            });
            // flicker
            if ra.action == "flicker_lights" {
                flicker.push((action_start, action_start + dur));
            }
            // insert markers for prop reveal / indicator moments
            if matches!(
                ra.action.as_str(),
                "inspect"
                    | "reveal_object"
                    | "open_elevator"
                    | "close_elevator"
                    | "activate"
                    | "point_at"
            ) {
                if let Some(tgt) = &ra.target_id {
                    inserts.push((action_start, tgt.clone()));
                }
            }
            // prop attach for interaction actions
            if matches!(
                ra.action.as_str(),
                "pick_up" | "give" | "conceal_object" | "reveal_object" | "carry" | "hold"
            ) {
                if let Some(target) = &ra.target_id {
                    if world.props.contains_key(target) {
                        prop_attach.push(PropAttach {
                            prop: target.clone(),
                            char_id: ra.actor_id.clone(),
                            start: action_start,
                            end: action_start + dur + 2.0,
                        });
                    }
                }
            }
            if !overlap_nonblocking_actions {
                t += dur;
            } else if matches!(kind, ActionKind::Speak) {
                t += dur;
            } else if matches!(kind, ActionKind::Move) {
                movement_t += dur;
                movement_ready_by_actor.insert(ra.actor_id.clone(), movement_t);
            }
        }
        for cue in &rb.command.environment {
            environment.push(ScheduledEnvironmentCue {
                target: cue.target.clone(),
                event: cue.event.clone(),
                start: beat_start + cue.start_offset.max(0.0),
                duration: cue.duration.max(0.01),
                from: cue.from,
                to: cue.to,
                value: cue.value.clone(),
                easing: cue.easing.clone(),
            });
        }
        for cue in &rb.command.sounds {
            sounds.push(ScheduledSoundCue {
                cue: cue.clone(),
                start: beat_start + cue.start_offset.max(0.0),
            });
        }
        if opens_elevator {
            let reveal_time = beat_start + 0.1;
            environment.push(ScheduledEnvironmentCue {
                target: "elevator_doors".into(),
                event: EnvironmentEventKind::ElevatorDoors,
                start: reveal_time,
                duration: 1.2,
                from: Some(0.0),
                to: Some(1.0),
                value: None,
                easing: "smoothstep".into(),
            });
            environment.push(ScheduledEnvironmentCue {
                target: "elevator_interior".into(),
                event: EnvironmentEventKind::ImpossibleFloorReveal,
                start: reveal_time + 0.35,
                duration: 0.8,
                from: Some(0.0),
                to: Some(1.0),
                value: None,
                easing: "smoothstep".into(),
            });
            sounds.push(ScheduledSoundCue {
                cue: SoundCue {
                    sound: "elevator_ding".into(),
                    source: None,
                    gain: 0.42,
                    start_offset: 0.0,
                },
                start: reveal_time,
            });
            sounds.push(ScheduledSoundCue {
                cue: SoundCue {
                    sound: "door_motor".into(),
                    source: None,
                    gain: 0.28,
                    start_offset: 0.0,
                },
                start: reveal_time + 0.12,
            });
            inserts.push((reveal_time, "elevator".into()));
            events.push(TimedEvent {
                t: reveal_time,
                kind: "open_elevator".into(),
                actor: None,
                target: Some("elevator_doors".into()),
                detail: "compiled from authored blocking/visible-action prose".into(),
            });
            blocking_plan.push(ExecutableBlockingAction {
                actor: "elevator_doors".into(),
                starting_slot: "closed".into(),
                destination_slot: "open".into(),
                travel_path: vec![],
                facing_target: None,
                arrival_time: reveal_time + 1.2,
                action_after_arrival: Some("reveal_elevator_interior".into()),
                required_camera_visible_moment: reveal_time + 0.7,
            });
        }
        let beat_duration = t.max(movement_t).max(visible_end);
        let beat_padding = if overlap_nonblocking_actions {
            0.0
        } else {
            0.6
        };
        let beat_end = (beat_start + beat_duration + beat_padding).max(
            rb.completion
                .seconds
                .unwrap_or(0.0)
                .max(beat_start + beat_duration),
        );
        if closes_elevator {
            let close_time = (beat_end - 1.1).max(beat_start);
            environment.push(ScheduledEnvironmentCue {
                target: "elevator_doors".into(),
                event: EnvironmentEventKind::ElevatorDoors,
                start: close_time,
                duration: 0.8,
                from: Some(1.0),
                to: Some(0.0),
                value: None,
                easing: "smoothstep".into(),
            });
            sounds.push(ScheduledSoundCue {
                cue: SoundCue {
                    sound: "door_motor".into(),
                    source: None,
                    gain: 0.30,
                    start_offset: 0.0,
                },
                start: close_time,
            });
            sounds.push(ScheduledSoundCue {
                cue: SoundCue {
                    sound: "door_slam".into(),
                    source: None,
                    gain: 0.44,
                    start_offset: 0.0,
                },
                start: close_time + 0.76,
            });
            flicker.push((close_time + 0.7, close_time + 1.05));
            events.push(TimedEvent {
                t: close_time,
                kind: "close_elevator".into(),
                actor: None,
                target: Some("elevator_doors".into()),
                detail: "compiled from authored payoff/visible-action prose".into(),
            });
            inserts.push((close_time, "elevator".into()));
            blocking_plan.push(ExecutableBlockingAction {
                actor: "elevator_doors".into(),
                starting_slot: "open".into(),
                destination_slot: "closed".into(),
                travel_path: vec![],
                facing_target: None,
                arrival_time: close_time + 0.8,
                action_after_arrival: Some("clipboard_disappears".into()),
                required_camera_visible_moment: close_time + 0.4,
            });
            for attachment in &mut prop_attach {
                if attachment.prop == "inspection_clipboard" {
                    attachment.end = close_time + 0.4;
                }
            }
        }
        clock = beat_end;
    }

    // Expand the per-beat camera intent into purposeful coverage via the
    // autonomous director. This is what turns sparse 5-shot plans into 8-14
    // shot episodes with hook/speaker/reaction/insert coverage.
    camera_shots = plan_shots(
        world,
        validated,
        &home_of,
        &dialogue,
        &inserts,
        clock.max(1.0),
    );

    Schedule {
        duration: clock.max(1.0),
        characters: chars,
        camera_shots,
        dialogue,
        captions,
        events,
        flicker,
        prop_attach,
        inserts,
        environment,
        sounds,
        movement_resolutions,
        blocking_plan,
    }
}

pub fn sample_schedule_overlap(
    sched: &Schedule,
    rigs: &HashMap<String, HumanoidRig>,
    world: &WorldState,
    fps: u32,
) -> crate::stage::CharacterOverlapReport {
    let fps = fps.max(1);
    let frame_count = (sched.duration * fps as f32).ceil() as usize;
    let mut samples = Vec::with_capacity(frame_count * sched.characters.len());
    for frame in 0..=frame_count {
        let time = (frame as f32 / fps as f32).min(sched.duration);
        for (character, _) in evaluate_at(sched, rigs, world, time).chars {
            samples.push(crate::stage::ActorRootSample::new(
                time,
                character.id,
                character.root.pos,
                0.45,
            ));
        }
    }
    crate::stage::analyze_actor_overlap(&samples, 0.05)
}

/// Split a spoken line into punctuation-aware, two-line-safe caption phrases.
/// Cues consume the measured WAV interval and stay within the 0.8–3.0 second
/// reading window whenever the source line duration permits it.
pub fn caption_phrases(text: &str, start: f32, end: f32) -> Vec<Caption> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() || end <= start {
        return vec![];
    }
    let mut phrases = Vec::<String>::new();
    let mut phrase = String::new();
    for word in normalized.split_whitespace() {
        let candidate_len = phrase.len() + usize::from(!phrase.is_empty()) + word.len();
        if candidate_len > 38 && !phrase.is_empty() {
            phrases.push(std::mem::take(&mut phrase));
        }
        if !phrase.is_empty() {
            phrase.push(' ');
        }
        phrase.push_str(word);
        if word.ends_with(['.', '!', '?', ';', ':']) && phrase.len() >= 14 {
            phrases.push(std::mem::take(&mut phrase));
        }
    }
    if !phrase.is_empty() {
        phrases.push(phrase);
    }

    let duration = end - start;
    let minimum_count = (duration / 3.0).ceil().max(1.0) as usize;
    while phrases.len() < minimum_count {
        let Some((index, _)) = phrases
            .iter()
            .enumerate()
            .filter(|(_, value)| value.split_whitespace().count() > 1)
            .max_by_key(|(_, value)| value.len())
        else {
            break;
        };
        let words: Vec<_> = phrases[index].split_whitespace().collect();
        let midpoint = words.len() / 2;
        let left = words[..midpoint].join(" ");
        let right = words[midpoint..].join(" ");
        phrases.splice(index..=index, [left, right]);
    }

    let total_weight: usize = phrases
        .iter()
        .map(|value| value.chars().count().max(1))
        .sum();
    let mut cursor = start;
    let phrase_count = phrases.len();
    phrases
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let remaining_cues = phrase_count - index;
            let remaining_time = end - cursor;
            let cue_duration = if remaining_cues == 1 {
                remaining_time
            } else {
                let weighted =
                    duration * text.chars().count().max(1) as f32 / total_weight.max(1) as f32;
                weighted
                    .clamp(0.8, 3.0)
                    .min(remaining_time - 0.8 * (remaining_cues - 1) as f32)
                    .max(0.01)
            };
            let caption = Caption {
                start: cursor,
                end: (cursor + cue_duration).min(end),
                text,
            };
            cursor = caption.end;
            caption
        })
        .collect()
}

/// Rebuild captions from Parakeet word boundaries while preserving the exact
/// authored caption text. ASR is timing evidence, never a dialogue rewrite.
pub fn apply_word_aligned_captions(
    schedule: &mut Schedule,
    alignments: &HashMap<(String, String), crate::asr::WordAlignment>,
) {
    let mut captions = Vec::new();
    for dialogue in &schedule.dialogue {
        let key = (dialogue.actor.clone(), dialogue.text.clone());
        let Some(alignment) = alignments.get(&key).filter(|value| !value.words.is_empty()) else {
            captions.extend(caption_phrases(
                &dialogue.text,
                dialogue.start,
                dialogue.end,
            ));
            continue;
        };
        let authored_words: Vec<&str> = dialogue.text.split_whitespace().collect();
        if authored_words.is_empty() {
            continue;
        }
        let mut groups: Vec<(usize, usize, String)> = Vec::new();
        let mut first = 0usize;
        let mut current = String::new();
        for (index, word) in authored_words.iter().enumerate() {
            let candidate = current.len() + usize::from(!current.is_empty()) + word.len();
            if candidate > 38 && !current.is_empty() {
                groups.push((first, index, std::mem::take(&mut current)));
                first = index;
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            if word.ends_with(['.', '!', '?', ';', ':']) && current.len() >= 14 {
                groups.push((first, index + 1, std::mem::take(&mut current)));
                first = index + 1;
            }
        }
        if !current.is_empty() {
            groups.push((first, authored_words.len(), current));
        }
        for (first, exclusive_end, text) in groups {
            let map_index = |index: usize| -> usize {
                ((index as f32 / authored_words.len() as f32) * alignment.words.len() as f32)
                    .floor()
                    .clamp(0.0, alignment.words.len().saturating_sub(1) as f32)
                    as usize
            };
            let start_word = map_index(first);
            let end_word = map_index(exclusive_end.saturating_sub(1));
            captions.push(Caption {
                start: (dialogue.start + alignment.words[start_word].start)
                    .clamp(dialogue.start, dialogue.end),
                end: (dialogue.start + alignment.words[end_word].end)
                    .clamp(dialogue.start, dialogue.end),
                text,
            });
        }
    }
    schedule.captions = captions;
}

/// Compress a schedule so it opens with content almost immediately and never
/// has a long stretch of dead air between lines. This is the Phase-8 watchability
/// fix: the director spaces beats with `completion` padding that produced 5+ second
/// silences and a 4-second cold open. We build a piecewise-linear time warp from the
/// dialogue boundaries and apply it uniformly to every timed element (dialogue,
/// captions, camera shots, events, flicker, prop attaches, inserts, and character
/// actions) so the whole episode stays self-consistent.
///
/// * The first line is moved to start at `lead` seconds (well within ~1s).
/// * Every inter-line gap is clamped into `[min_gap, max_gap]`, where `max_gap` is
///   kept under the configured `max_dead_air` so the silence check passes.
/// * Trailing silence is trimmed to at most `max_gap`.
pub fn compact_dead_air(sched: &mut Schedule, max_dead_air: f32) {
    if sched.dialogue.is_empty() {
        return;
    }
    let lead = 0.6f32;
    let min_gap = 0.5f32;
    let max_gap = (max_dead_air - 0.6).clamp(1.5, 3.0);

    // Sorted dialogue boundaries (old space).
    let mut d: Vec<(f32, f32)> = sched
        .dialogue
        .iter()
        .map(|x| (x.start, x.end.max(x.start)))
        .collect();
    d.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let old_dur = sched.duration;

    // Build control points (old_time, new_time) for the warp.
    let mut cp: Vec<(f32, f32)> = Vec::new();
    cp.push((0.0, 0.0));
    let mut prev_old_end = 0.0f32;
    let mut prev_new_end = 0.0f32;
    for (i, (s, e)) in d.iter().enumerate() {
        let ns = if i == 0 {
            lead
        } else {
            let gap_old = (s - prev_old_end).max(0.0);
            let gap_new = gap_old.clamp(min_gap, max_gap);
            prev_new_end + gap_new
        };
        let dur = (e - s).max(0.0);
        let ne = ns + dur;
        let s_cp = if i == 0 {
            *s
        } else {
            (*s).max(prev_old_end + 1e-3)
        };
        cp.push((s_cp, ns));
        if *e > s_cp + 1e-3 {
            cp.push((*e, ne));
        }
        prev_old_end = e.max(prev_old_end);
        prev_new_end = ne;
    }
    let tail = (old_dur - prev_old_end).clamp(0.0, max_gap);
    let new_dur = prev_new_end + tail;
    cp.push((old_dur, new_dur));
    cp.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let warp = |t: f32| -> f32 {
        if t <= cp[0].0 {
            return cp[0].1;
        }
        if t >= cp.last().unwrap().0 {
            return cp.last().unwrap().1;
        }
        for i in 0..cp.len() - 1 {
            let (o0, n0) = cp[i];
            let (o1, n1) = cp[i + 1];
            if t >= o0 && t <= o1 {
                let k = if o1 - o0 > 1e-6 {
                    (t - o0) / (o1 - o0)
                } else {
                    0.0
                };
                return n0 + (n1 - n0) * k;
            }
        }
        cp.last().unwrap().1
    };

    for x in &mut sched.dialogue {
        x.start = warp(x.start);
        x.end = warp(x.end);
    }
    for c in &mut sched.captions {
        c.start = warp(c.start);
        c.end = warp(c.end);
    }
    for ev in &mut sched.events {
        ev.t = warp(ev.t);
    }
    for f in &mut sched.flicker {
        f.0 = warp(f.0);
        f.1 = warp(f.1);
    }
    for ins in &mut sched.inserts {
        ins.0 = warp(ins.0);
    }
    for cue in &mut sched.environment {
        let ns = warp(cue.start);
        let ne = warp(cue.start + cue.duration);
        cue.start = ns;
        cue.duration = (ne - ns).max(0.01);
    }
    for cue in &mut sched.sounds {
        cue.start = warp(cue.start);
    }
    for pa in &mut sched.prop_attach {
        pa.start = warp(pa.start);
        pa.end = warp(pa.end);
    }
    for cs in &mut sched.camera_shots {
        cs.start = warp(cs.start);
        cs.end = warp(cs.end);
    }
    for ct in &mut sched.characters {
        for a in &mut ct.actions {
            let ns = warp(a.start);
            let ne = warp(a.start + a.dur);
            a.start = ns;
            a.dur = (ne - ns).max(0.01);
        }
    }
    for movement in &mut sched.movement_resolutions {
        movement.start = warp(movement.start);
        movement.end = warp(movement.end);
    }
    for action in &mut sched.blocking_plan {
        action.arrival_time = warp(action.arrival_time);
        action.required_camera_visible_moment = warp(action.required_camera_visible_moment);
    }
    sched.duration = new_dur;
}

#[cfg(test)]
mod timeline_tests {
    use super::*;
    use crate::package::DialogueLine;

    #[test]
    fn compact_dead_air_moves_first_line_and_clamps_gaps() {
        let mut sched = Schedule {
            duration: 60.0,
            characters: vec![],
            camera_shots: vec![],
            dialogue: vec![
                DialogueLine {
                    start: 4.0,
                    end: 7.0,
                    actor: "a".into(),
                    text: "x".into(),
                    voice_id: "a".into(),
                },
                DialogueLine {
                    start: 14.0,
                    end: 16.0,
                    actor: "b".into(),
                    text: "y".into(),
                    voice_id: "b".into(),
                },
                DialogueLine {
                    start: 25.0,
                    end: 27.0,
                    actor: "c".into(),
                    text: "z".into(),
                    voice_id: "c".into(),
                },
            ],
            captions: vec![],
            events: vec![],
            flicker: vec![],
            prop_attach: vec![],
            inserts: vec![],
            environment: vec![],
            sounds: vec![],
            movement_resolutions: vec![],
            blocking_plan: vec![],
        };
        compact_dead_air(&mut sched, 4.0);
        // First line starts within ~1s (was 4.0s cold open).
        assert!(
            sched.dialogue[0].start < 1.0,
            "first content must start within ~1s"
        );
        // No inter-line gap exceeds the dead-air limit (max_gap <= 3.5).
        for i in 1..sched.dialogue.len() {
            let gap = sched.dialogue[i].start - sched.dialogue[i - 1].end;
            assert!(gap <= 3.6, "gap {gap} exceeds dead-air limit");
        }
        // Timeline shrank (dead air removed).
        assert!(
            sched.duration < 40.0,
            "duration {} not compressed",
            sched.duration
        );
        // Dialogue stays ordered.
        for i in 1..sched.dialogue.len() {
            assert!(sched.dialogue[i].start >= sched.dialogue[i - 1].start);
        }
    }
}

// ===========================================================================
// Frame evaluation
// ===========================================================================

#[derive(Debug, Clone)]
pub struct CharFrame {
    pub id: String,
    pub root: Xform,
    pub state: PerformanceState,
    pub walk_phase: f32,
    pub speaking: bool,
    pub action_local_time: f32,
    pub action_weight: f32,
    pub focus_target: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PropFrame {
    pub id: String,
    pub pos: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct FrameState {
    pub chars: Vec<(CharFrame, Pose)>,
    pub camera_eye: [f32; 3],
    pub camera_look: [f32; 3],
    pub props: Vec<PropFrame>,
    pub flicker: bool,
    /// Elevator door open amount in [0,1] (0 = closed, 1 = fully open).
    pub elevator_open: f32,
    pub elevator_indicator: Option<String>,
    pub panel_active: f32,
    pub impossible_reveal: f32,
}

/// Compute the world state at absolute time `t` (deterministic).
pub fn evaluate_at(
    sched: &Schedule,
    rigs: &HashMap<String, HumanoidRig>,
    world: &WorldState,
    t: f32,
) -> FrameState {
    // First pass: root positions + performance state per character.
    let mut frames: Vec<CharFrame> = Vec::new();
    for ct in &sched.characters {
        // position from sequential moves
        let mut pos = ct.home;
        let mut from = ct.home;
        let mut moving = false;
        let mut dir = [0.0f32, 0.0, 0.0];
        let mut yaw = 0.0f32;
        let mut active_state: Option<(PerformanceState, bool)> = None;
        let mut focus_target: Option<String> = None;
        let mut action_local_time = 0.0f32;
        let mut action_weight = 0.0f32;
        // gather this character's actions sorted by start
        let mut acts: Vec<&ScheduledAction> = ct.actions.iter().collect();
        acts.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        for a in &acts {
            if t < a.start {
                break;
            }
            match action_kind(&a.action) {
                ActionKind::Move => {
                    if let Some(p) = a.target_position.or_else(|| {
                        resolve_pos(
                            a.target.as_deref().unwrap_or(""),
                            world,
                            &char_home_map(sched),
                        )
                    }) {
                        let k = ((t - a.start) / a.dur.max(0.001)).clamp(0.0, 1.0);
                        if t < a.start + a.dur {
                            pos = [
                                from[0] + (p[0] - from[0]) * k,
                                from[1] + (p[1] - from[1]) * k,
                                from[2] + (p[2] - from[2]) * k,
                            ];
                            dir = [p[0] - from[0], 0.0, p[2] - from[2]];
                            moving = true;
                        } else {
                            // move already completed; advance current position
                            pos = p;
                            dir = [p[0] - from[0], 0.0, p[2] - from[2]];
                            from = p;
                        }
                    }
                }
                _ if t >= a.start && t < a.start + a.dur => {
                    let local = ((t - a.start) / a.dur.max(0.001)).clamp(0.0, 1.0);
                    action_local_time = t - a.start;
                    action_weight = action_phase_weight(
                        a.performance.as_ref().map(|p| p.phases).unwrap_or_default(),
                        local,
                    );
                    match action_kind(&a.action) {
                        ActionKind::Speak => active_state = Some((PerformanceState::Talk, true)),
                        ActionKind::React => active_state = Some((PerformanceState::React, false)),
                        ActionKind::Gesture => {
                            active_state = Some((PerformanceState::Gesture, false))
                        }
                        ActionKind::Point => {
                            active_state = Some((PerformanceState::Point, false));
                            focus_target = a.target.clone();
                        }
                        ActionKind::Look => {
                            active_state = Some((PerformanceState::Look, false));
                            focus_target = a.target.clone();
                        }
                        ActionKind::Listen => {
                            active_state = Some((PerformanceState::Listen, false))
                        }
                        ActionKind::Interact => {
                            active_state = Some((PerformanceState::Gesture, false));
                            focus_target = a.target.clone();
                        }
                        ActionKind::Environment | ActionKind::Narrative | ActionKind::Unknown => {}
                        ActionKind::Move => {}
                    }
                }
                _ => {}
            }
        }
        if moving && (dir[0] != 0.0 || dir[2] != 0.0) {
            yaw = dir[0].atan2(dir[2]);
        } else if let Some(target) = focus_target.as_deref() {
            if let Some(p) = resolve_pos(target, world, &char_home_map(sched)) {
                let dx = p[0] - pos[0];
                let dz = p[2] - pos[2];
                if dx != 0.0 || dz != 0.0 {
                    yaw = dx.atan2(dz);
                }
            }
        }
        let (state, speaking) = match active_state {
            Some(s) => s,
            None if moving => (PerformanceState::Walk, false),
            None => {
                // listening if someone else is speaking
                let other_speaking = sched
                    .dialogue
                    .iter()
                    .any(|d| d.start <= t && t < d.end && d.actor != ct.id);
                if other_speaking {
                    (PerformanceState::Listen, false)
                } else {
                    (PerformanceState::Idle, false)
                }
            }
        };
        frames.push(CharFrame {
            id: ct.id.clone(),
            root: Xform {
                pos: [pos[0], 0.0, pos[2]],
                rot: [0.0, yaw, 0.0],
            },
            state,
            walk_phase: t,
            speaking,
            action_local_time,
            action_weight: if moving { 1.0 } else { action_weight },
            focus_target,
        });
    }

    // Second pass: poses + focus yaw (face conversational partner).
    let mut posed: Vec<(CharFrame, Pose)> = Vec::new();
    for f in &frames {
        let idle = character_pose(PerformanceState::Idle, t, f.walk_phase);
        let active = character_pose(f.state, f.action_local_time, f.walk_phase);
        let mut pose = if matches!(
            f.state,
            PerformanceState::Idle | PerformanceState::Walk | PerformanceState::Listen
        ) {
            active
        } else {
            Pose::blend(&idle, &active, f.action_weight)
        };
        // gaze toward conversational partner if listening/speaking
        if matches!(
            f.state,
            PerformanceState::Listen | PerformanceState::Talk | PerformanceState::Look
        ) {
            let partner = f
                .focus_target
                .as_ref()
                .and_then(|target| frames.iter().find(|o| o.id == *target).map(|o| o.root.pos))
                .or_else(|| frames.iter().find(|o| o.id != f.id).map(|o| o.root.pos));
            if let Some(p) = partner {
                let dx = p[0] - f.root.pos[0];
                let dz = p[2] - f.root.pos[2];
                if dx != 0.0 || dz != 0.0 {
                    let yaw = dx.atan2(dz);
                    // blend into head yaw
                    if let Some(h) = pose.get(crate::avatar::SemanticJoint::Head) {
                        let mut nh = h.clone();
                        nh.rot[1] = yaw;
                        pose.set(crate::avatar::SemanticJoint::Head, nh);
                    } else {
                        pose.set(
                            crate::avatar::SemanticJoint::Head,
                            Xform {
                                pos: [0.0; 3],
                                rot: [0.0, yaw, 0.0],
                            },
                        );
                    }
                }
            }
        }
        posed.push((f.clone(), pose));
    }

    // Camera: pick the active shot and frame a real on-screen performer.
    let active_shot = sched
        .camera_shots
        .iter()
        .find(|s| t >= s.start && t < s.end)
        .or_else(|| sched.camera_shots.last());

    // Explicit door actions are persistent state transitions. The latest action
    // wins so authored payoffs can close doors after an earlier reveal.
    let authored_elevator_state = sched
        .characters
        .iter()
        .flat_map(|c| c.actions.iter())
        .filter(|a| matches!(a.action.as_str(), "open_elevator" | "close_elevator") && a.start <= t)
        .max_by(|left, right| left.start.total_cmp(&right.start))
        .map(|action| {
            if action.action == "open_elevator" {
                1.0
            } else {
                0.0
            }
        });
    let cue_value = |kind: EnvironmentEventKind| -> f32 {
        sched
            .environment
            .iter()
            .filter(|cue| cue.event == kind && t >= cue.start)
            .map(|cue| {
                let k = smoothstep(((t - cue.start) / cue.duration.max(0.01)).clamp(0.0, 1.0));
                cue.from.unwrap_or(0.0) + (cue.to.unwrap_or(1.0) - cue.from.unwrap_or(0.0)) * k
            })
            .last()
            .unwrap_or(0.0)
    };
    let elevator_open =
        authored_elevator_state.unwrap_or_else(|| cue_value(EnvironmentEventKind::ElevatorDoors));
    let panel_active = cue_value(EnvironmentEventKind::ControlPanel);
    let impossible_reveal = cue_value(EnvironmentEventKind::ImpossibleFloorReveal);
    let elevator_indicator = sched
        .environment
        .iter()
        .filter(|cue| cue.event == EnvironmentEventKind::ElevatorIndicator && t >= cue.start)
        .last()
        .and_then(|cue| cue.value.clone());

    let (eye, look) = if let Some(shot) = active_shot {
        let static_subject = if posed.iter().any(|(frame, _)| frame.id == shot.subject) {
            None
        } else {
            crate::stage::feature_position(&shot.subject)
                .or_else(|| resolve_pos(&shot.subject, world, &char_home_map(sched)))
                .map(|mut position| {
                    position[1] = if shot.subject.contains("indicator") {
                        2.65
                    } else if shot.subject.contains("panel") {
                        1.25
                    } else if shot.subject.contains("elevator") {
                        1.45
                    } else {
                        position[1].max(0.8)
                    };
                    position
                })
        };
        let subject_char = posed
            .iter()
            .find(|(f, _)| f.id == shot.subject)
            .map(|(f, _)| f.id.clone())
            .or_else(|| {
                shot.reaction.as_ref().and_then(|r| {
                    posed
                        .iter()
                        .find(|(f, _)| f.id == *r)
                        .map(|(f, _)| f.id.clone())
                })
            })
            .or_else(|| {
                sched
                    .dialogue
                    .iter()
                    .find(|d| d.start <= t && t < d.end)
                    .map(|d| d.actor.clone())
            })
            .or_else(|| posed.first().map(|(f, _)| f.id.clone()))
            .unwrap_or_default();
        // Reaction shots should *show the reactor*, so frame the reaction
        // subject instead of the speaker.
        let frame_char_id = if shot.intent == "reaction" {
            shot.reaction.clone().unwrap_or(subject_char.clone())
        } else {
            subject_char.clone()
        };
        let frame_frame = posed
            .iter()
            .find(|(f, _)| f.id == frame_char_id)
            .or_else(|| posed.iter().find(|(f, _)| f.id == subject_char));
        let frame_pos = static_subject.unwrap_or_else(|| {
            frame_frame
                .map(|(f, p)| {
                    rigs.get(&f.id)
                        .map(|r| r.camera_target(CameraTargetRole::Head, &f.root, p))
                        .unwrap_or(f.root.pos)
                })
                .unwrap_or([0.0, 1.5, 0.0])
        });
        let yaw = if static_subject.is_some() {
            0.0
        } else {
            frame_frame.map(|(f, _)| f.root.rot[1]).unwrap_or(0.0)
        };
        // Frame around the chest so the performer is vertically centred.
        let chest = if static_subject.is_some() {
            frame_pos
        } else {
            [frame_pos[0], frame_pos[1] - 0.55, frame_pos[2]]
        };
        let react_pos = shot
            .reaction
            .as_ref()
            .and_then(|rid| posed.iter().find(|(f, _)| f.id == *rid))
            .map(|(f, p)| {
                rigs.get(&f.id)
                    .map(|r| r.camera_target(CameraTargetRole::Head, &f.root, p))
                    .unwrap_or(f.root.pos)
            });
        let (loff, _look_role) = camera_offset(&shot.intent, react_pos);
        // Offset is expressed in the subject's local frame (+z = in front of the
        // performer), then rotated by the subject's facing so we see their face.
        let world_off = rotate_y([loff.0, loff.1, loff.2], yaw);
        let mut eye = clamp_camera_to_hallway(
            [
                chest[0] + world_off[0],
                chest[1] + world_off[1],
                chest[2] + world_off[2],
            ],
            &frame_pos,
        );
        // Enforce a minimum camera-to-subject distance. Without this a close
        // (e.g. reaction / OTS) shot could place the camera inside a performer's
        // near plane, making a limb triangle explode into a full-frame shard.
        let min_dist = 1.6f32;
        let dx = eye[0] - chest[0];
        let dy = eye[1] - chest[1];
        let dz = eye[2] - chest[2];
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        if d < min_dist {
            let s = min_dist / d.max(1e-3);
            eye = [chest[0] + dx * s, chest[1] + dy * s, chest[2] + dz * s];
            eye = clamp_camera_to_hallway(eye, &frame_pos);
        }
        // Look at the framed performer (chest). Reaction / OTS shots still point
        // here because `frame_char_id` already resolves to the reactor.
        let look = chest;
        (eye, look)
    } else {
        ([0.0, 3.0, 7.0], [0.0, 1.2, 0.0])
    };

    // Props: attached to grip or at home mark.
    let mut props: Vec<PropFrame> = Vec::new();
    for p in world.props.values() {
        let attached = sched
            .prop_attach
            .iter()
            .find(|a| a.prop == p.id && t >= a.start && t < a.end);
        let pos = if let Some(a) = attached {
            posed
                .iter()
                .find(|(f, _)| f.id == a.char_id)
                .and_then(|(f, pose)| {
                    rigs.get(&f.id)
                        .map(|r| r.camera_target(CameraTargetRole::PropGrip, &f.root, pose))
                })
                .unwrap_or([0.0, 1.0, 0.0])
        } else {
            let home = world
                .locations
                .values()
                .flat_map(|l| l.staging_marks.iter())
                .find(|m| m.id == p.home_mark)
                .map(|m| m.position)
                .unwrap_or([0.0; 3]);
            [home[0], 0.5, home[2]]
        };
        props.push(PropFrame {
            id: p.id.clone(),
            pos,
        });
    }

    let flicker = sched.flicker.iter().any(|(s, e)| t >= *s && t < *e);

    FrameState {
        chars: posed,
        camera_eye: eye,
        camera_look: look,
        props,
        flicker,
        elevator_open,
        elevator_indicator,
        panel_active,
        impossible_reveal,
    }
}

pub fn char_home_map(sched: &Schedule) -> HashMap<String, [f32; 3]> {
    sched
        .characters
        .iter()
        .map(|c| (c.id.clone(), c.home))
        .collect()
}

/// Camera offset per intent, expressed in the *subject's local frame* as
/// `(side, height_above_chest, forward)` in world meters. `+z` local is in front
/// of the performer; the caller rotates this by the subject yaw so the camera
/// always sits in front of the face. Height is measured above the chest
/// (roughly 0.55 m below the head) so the framing subject is vertically centred.
pub fn camera_offset(intent: &str, react: Option<[f32; 3]>) -> ((f32, f32, f32), CameraTargetRole) {
    let o = match intent {
        "establish" | "comedic_wide" | "group_coverage" => (0.0, 1.1, 5.6),
        "speaker_closeup" | "follow" | "conversation" => (0.0, 0.3, 3.4),
        "reaction" => {
            let r = react.unwrap_or([0.0, 1.5, 0.0]);
            // approach the reactor from its side, slightly closer
            (r[0].signum().max(1.0) * 1.2, 0.4, 3.3)
        }
        "reveal" | "insert_object" => (0.8, 0.2, 2.8),
        "tension_push" => (0.0, 0.4, 3.1),
        "cliffhanger_hold" => (0.0, 0.5, 3.2),
        "over_the_shoulder" => (-1.0, 0.4, 3.5),
        "exit_transition" => (0.0, 1.5, 5.0),
        _ => (0.0, 0.4, 3.5),
    };
    (o, CameraTargetRole::Head)
}

/// Rotate a vector around the world Y axis by `yaw` (radians).
fn rotate_y(v: [f32; 3], yaw: f32) -> [f32; 3] {
    let (s, c) = yaw.sin_cos();
    [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c]
}

/// Keep the camera inside the hallway and out of solid set geometry (notably
/// the elevator shell). This prevents "camera inside a wall" compositions.
fn clamp_camera_to_hallway(eye: [f32; 3], _subj: &[f32; 3]) -> [f32; 3] {
    let mut e = eye;
    e[0] = e[0].clamp(-7.0, 7.0);
    e[2] = e[2].clamp(-2.4, 9.0);
    // Elevator cabin occupies the rear-left bay; keep cameras in the hallway
    // side of its doors rather than inside the shell.
    if e[2] < -3.0 && e[0] > -6.8 && e[0] < -3.5 {
        e[2] = -2.4;
    }
    e
}
