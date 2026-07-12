//! Shared, engine-agnostic episode timeline.
//!
//! This is the **single authoritative representation** of an episode's committed
//! motion. Both the offline CPU renderer (`render.rs`) and the real Bevy renderer
//! (`backlot-app`) consume the same `Schedule` and `evaluate_at`, so there is no
//! second, divergent interpretation of the world. The Bevy renderer merely draws
//! the `FrameState` this module produces; it never re-derives scene state.

use crate::avatar::{
    character_pose, CameraTargetRole, HumanoidRig, PerformanceState, Pose, Xform,
};
use crate::package::{Caption, DialogueLine, TimedEvent};
use crate::validation::{validate_beat_command, validate_plan, ValidatedPlan};
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
    pub text: Option<String>,
    pub start: f32,
    pub dur: f32,
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
    Other,
}

pub fn action_kind(a: &str) -> ActionKind {
    match a {
        "move_to" | "approach" | "retreat_from" | "follow" | "flee_to" | "enter_room" | "exit_room" => ActionKind::Move,
        "speak" | "whisper" | "shout" => ActionKind::Speak,
        "react" => ActionKind::React,
        "gesture" => ActionKind::Gesture,
        "point_at" => ActionKind::Point,
        "look_at" | "turn_toward" => ActionKind::Look,
        "sigh" | "laugh" | "display_emotion" | "conceal_emotion" | "pause" | "interrupt" => ActionKind::Listen,
        _ => ActionKind::Other,
    }
}

/// Resolve a target id to a static world position (marks, character homes, or
/// prop home marks). Deterministic and resolution-independent.
pub fn resolve_pos(
    target: &str,
    world: &WorldState,
    home_of: &HashMap<String, [f32; 3]>,
) -> Option<[f32; 3]> {
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
    let loc_id = &validated.plan.primary_location;
    let marks: Vec<[f32; 3]> = world
        .locations
        .get(loc_id)
        .map(|l| l.staging_marks.iter().map(|m| m.position).collect())
        .unwrap_or_default();

    let active: Vec<&String> = validated.plan.active_characters.iter().collect();
    let mut home_of: HashMap<String, [f32; 3]> = HashMap::new();
    let default_marks = ["hall_center", "apt_3b_door", "maintenance_panel", "apt_4a_door"];
    for (i, id) in active.iter().enumerate() {
        let pos = marks
            .get(i)
            .copied()
            .or_else(|| {
                world
                    .locations
                    .get(loc_id)
                    .and_then(|l| l.staging_marks.iter().find(|m| m.id == default_marks[i % default_marks.len()]).map(|m| m.position))
            })
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
    let mut camera_shots: Vec<CameraShotSpec> = Vec::new();

    let mut clock = 0.0f32;
    for rb in &validated.resolved_beats {
        let beat_start = clock;
        let mut t = 0.0f32;
        for ra in &rb.resolved_actions {
            let dur = match action_kind(&ra.action) {
                ActionKind::Speak => {
                    let key = (ra.actor_id.clone(), ra.text.clone().unwrap_or_default());
                    *tts_durations.get(&key).unwrap_or(&ra.estimated_duration)
                }
                _ => ra.estimated_duration,
            };
            // record on the actor track
            if let Some(ct) = chars.iter_mut().find(|c| c.id == ra.actor_id) {
                ct.actions.push(ScheduledAction {
                    actor: ra.actor_id.clone(),
                    action: ra.action.clone(),
                    target: ra.target_id.clone(),
                    text: ra.text.clone(),
                    start: beat_start + t,
                    dur,
                });
            }
            // dialogue + captions for speech
            if matches!(action_kind(&ra.action), ActionKind::Speak) {
                let voice = world
                    .character(&ra.actor_id)
                    .map(|c| c.voice_id.clone())
                    .unwrap_or_else(|| ra.actor_id.clone());
                let s = beat_start + t;
                let e = s + dur;
                let text = ra.text.clone().unwrap_or_default();
                dialogue.push(DialogueLine {
                    start: s,
                    end: e,
                    actor: ra.actor_id.clone(),
                    text: text.clone(),
                    voice_id: voice.clone(),
                });
                captions.push(Caption { start: s, end: e, text });
            }
            // events log
            events.push(TimedEvent {
                t: beat_start + t,
                kind: ra.action.clone(),
                actor: Some(ra.actor_id.clone()),
                target: ra.target_id.clone(),
                detail: ra.text.clone().unwrap_or_default(),
            });
            // flicker
            if ra.action == "flicker_lights" {
                flicker.push((beat_start + t, beat_start + t + dur));
            }
            // insert markers for prop reveal / indicator moments
            if matches!(
                ra.action.as_str(),
                "inspect" | "reveal_object" | "open_elevator" | "activate" | "point_at"
            ) {
                if let Some(tgt) = &ra.target_id {
                    inserts.push((beat_start + t, tgt.clone()));
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
                            start: beat_start + t,
                            end: beat_start + t + dur + 2.0,
                        });
                    }
                }
            }
            t += dur;
        }
        let beat_end = (beat_start + t + 0.6).max(
            rb.completion
                .seconds
                .unwrap_or(0.0)
                .max(beat_start + t),
        );
        clock = beat_end;
    }

    // Expand the per-beat camera intent into purposeful coverage via the
    // autonomous director. This is what turns sparse 5-shot plans into 8-14
    // shot episodes with hook/speaker/reaction/insert coverage.
    camera_shots = plan_shots(world, validated, &home_of, &dialogue, &inserts, clock.max(1.0));

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
}

/// Compute the world state at absolute time `t` (deterministic).
pub fn evaluate_at(sched: &Schedule, rigs: &HashMap<String, HumanoidRig>, world: &WorldState, t: f32) -> FrameState {
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
        // gather this character's actions sorted by start
        let mut acts: Vec<&ScheduledAction> = ct.actions.iter().collect();
        acts.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        for a in &acts {
            if t < a.start {
                break;
            }
            match action_kind(&a.action) {
                ActionKind::Move => {
                    if let Some(p) = resolve_pos(a.target.as_deref().unwrap_or(""), world, &char_home_map(sched)) {
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
                ActionKind::Speak => active_state = Some((PerformanceState::Talk, true)),
                ActionKind::React => active_state = Some((PerformanceState::React, false)),
                ActionKind::Gesture => active_state = Some((PerformanceState::Gesture, false)),
                ActionKind::Point => active_state = Some((PerformanceState::Point, false)),
                ActionKind::Look => active_state = Some((PerformanceState::Look, false)),
                ActionKind::Listen => active_state = Some((PerformanceState::Listen, false)),
                ActionKind::Other => { /* interaction: keep current */ }
            }
        }
        if moving && (dir[0] != 0.0 || dir[2] != 0.0) {
            yaw = dir[0].atan2(dir[2]);
        }
        let (state, speaking) = match active_state {
            Some(s) => s,
            None => {
                // listening if someone else is speaking
                let other_speaking = sched.dialogue.iter().any(|d| d.start <= t && t < d.end && d.actor != ct.id);
                if other_speaking {
                    (PerformanceState::Listen, false)
                } else {
                    (PerformanceState::Idle, false)
                }
            }
        };
        frames.push(CharFrame {
            id: ct.id.clone(),
            root: Xform { pos: [pos[0], 0.0, pos[2]], rot: [0.0, yaw, 0.0] },
            state,
            walk_phase: t,
            speaking,
        });
    }

    // Second pass: poses + focus yaw (face conversational partner).
    let mut posed: Vec<(CharFrame, Pose)> = Vec::new();
    for f in &frames {
        let mut pose = character_pose(f.state, t, f.walk_phase);
        // gaze toward conversational partner if listening/speaking
        if matches!(f.state, PerformanceState::Listen | PerformanceState::Talk | PerformanceState::Look) {
            let partner = frames
                .iter()
                .find(|o| o.id != f.id)
                .map(|o| o.root.pos);
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
                        pose.set(crate::avatar::SemanticJoint::Head, Xform { pos: [0.0; 3], rot: [0.0, yaw, 0.0] });
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
    let (eye, look) = if let Some(shot) = active_shot {
        let subject_char = posed
            .iter()
            .find(|(f, _)| f.id == shot.subject)
            .map(|(f, _)| f.id.clone())
            .or_else(|| {
                shot.reaction
                    .as_ref()
                    .and_then(|r| posed.iter().find(|(f, _)| f.id == *r).map(|(f, _)| f.id.clone()))
            })
            .or_else(|| {
                sched
                    .dialogue
                    .iter()
                    .find(|d| d.start <= t && t < d.end)
                    .map(|d| d.actor.clone())
            })
            .or_else(|| posed.first().map(|(f, _)| f.id.clone()));
        let subj_frame = subject_char.as_ref().and_then(|id| posed.iter().find(|(f, _)| &f.id == id));
        let subj_pos = subj_frame
            .map(|(f, p)| {
                rigs.get(&f.id)
                    .map(|r| r.camera_target(CameraTargetRole::Head, &f.root, p))
                    .unwrap_or(f.root.pos)
            })
            .unwrap_or([0.0, 1.5, 0.0]);
        let react_pos = shot
            .reaction
            .as_ref()
            .and_then(|rid| posed.iter().find(|(f, _)| f.id == *rid))
            .map(|(f, p)| {
                rigs.get(&f.id)
                    .map(|r| r.camera_target(CameraTargetRole::Head, &f.root, p))
                    .unwrap_or(f.root.pos)
            });
        let (off, _look_role) = camera_offset(&shot.intent, react_pos);
        let eye = [subj_pos[0] + off.0, subj_pos[1] + off.1, subj_pos[2] + off.2];
        let look = if let Some(rp) = react_pos {
            if matches!(shot.intent.as_str(), "reaction" | "over_the_shoulder") {
                rp
            } else {
                subj_pos
            }
        } else {
            subj_pos
        };
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
        props.push(PropFrame { id: p.id.clone(), pos });
    }

    let flicker = sched.flicker.iter().any(|(s, e)| t >= *s && t < *e);

    FrameState {
        chars: posed,
        camera_eye: eye,
        camera_look: look,
        props,
        flicker,
    }
}

pub fn char_home_map(sched: &Schedule) -> HashMap<String, [f32; 3]> {
    sched
        .characters
        .iter()
        .map(|c| (c.id.clone(), c.home))
        .collect()
}

/// Camera offset (relative to subject head) per intent, in world meters.
/// Always offsets from a *character* head, so the camera frames a performer
/// rather than a wall or the elevator shell.
pub fn camera_offset(intent: &str, react: Option<[f32; 3]>) -> ((f32, f32, f32), CameraTargetRole) {
    let o = match intent {
        "establish" | "comedic_wide" | "group_coverage" => (0.0, 2.4, 5.2),
        "speaker_closeup" | "follow" | "conversation" => (0.0, 1.5, 2.7),
        "reaction" => {
            let r = react.unwrap_or([0.0, 1.5, 0.0]);
            // approach the reactor from the side, slightly closer
            (r[0].signum().max(1.0) * 1.2, 1.5, 2.6)
        }
        "reveal" | "insert_object" => (1.2, 1.2, 2.2),
        "tension_push" => (0.0, 1.6, 2.0),
        "cliffhanger_hold" => (0.0, 1.7, 2.2),
        "over_the_shoulder" => (-1.2, 1.5, 2.4),
        "exit_transition" => (0.0, 2.0, 4.2),
        _ => (0.0, 1.5, 3.2),
    };
    (o, CameraTargetRole::Head)
}
