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
        let s_cp = if i == 0 { *s } else { (*s).max(prev_old_end + 1e-3) };
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
                let k = if o1 - o0 > 1e-6 { (t - o0) / (o1 - o0) } else { 0.0 };
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
                DialogueLine { start: 4.0, end: 7.0, actor: "a".into(), text: "x".into(), voice_id: "a".into() },
                DialogueLine { start: 14.0, end: 16.0, actor: "b".into(), text: "y".into(), voice_id: "b".into() },
                DialogueLine { start: 25.0, end: 27.0, actor: "c".into(), text: "z".into(), voice_id: "c".into() },
            ],
            captions: vec![],
            events: vec![],
            flicker: vec![],
            prop_attach: vec![],
            inserts: vec![],
        };
        compact_dead_air(&mut sched, 4.0);
        // First line starts within ~1s (was 4.0s cold open).
        assert!(sched.dialogue[0].start < 1.0, "first content must start within ~1s");
        // No inter-line gap exceeds the dead-air limit (max_gap <= 3.5).
        for i in 1..sched.dialogue.len() {
            let gap = sched.dialogue[i].start - sched.dialogue[i - 1].end;
            assert!(gap <= 3.6, "gap {gap} exceeds dead-air limit");
        }
        // Timeline shrank (dead air removed).
        assert!(sched.duration < 40.0, "duration {} not compressed", sched.duration);
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
        let mut focus_target: Option<String> = None;
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
                _ if t >= a.start && t < a.start + a.dur => match action_kind(&a.action) {
                    ActionKind::Speak => active_state = Some((PerformanceState::Talk, true)),
                    ActionKind::React => active_state = Some((PerformanceState::React, false)),
                    ActionKind::Gesture => active_state = Some((PerformanceState::Gesture, false)),
                    ActionKind::Point => {
                        active_state = Some((PerformanceState::Point, false));
                        focus_target = a.target.clone();
                    }
                    ActionKind::Look => {
                        active_state = Some((PerformanceState::Look, false));
                        focus_target = a.target.clone();
                    }
                    ActionKind::Listen => active_state = Some((PerformanceState::Listen, false)),
                    // Object interactions need readable body business rather than
                    // silently retaining a previous pose.
                    ActionKind::Other => active_state = Some((PerformanceState::Gesture, false)),
                    ActionKind::Move => {}
                },
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

    // Elevator door state: once an `open_elevator` action has started, the doors
    // stay open for the rest of the episode.
    let elevator_open = sched
        .characters
        .iter()
        .flat_map(|c| c.actions.iter())
        .any(|a| a.action == "open_elevator" && a.start <= t)
        .then(|| 1.0f32)
        .unwrap_or(0.0);

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
        let frame_pos = frame_frame
            .map(|(f, p)| {
                rigs.get(&f.id)
                    .map(|r| r.camera_target(CameraTargetRole::Head, &f.root, p))
                    .unwrap_or(f.root.pos)
            })
            .unwrap_or([0.0, 1.5, 0.0]);
        let yaw = frame_frame.map(|(f, _)| f.root.rot[1]).unwrap_or(0.0);
        // Frame around the chest so the performer is vertically centred.
        let chest = [frame_pos[0], frame_pos[1] - 0.55, frame_pos[2]];
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
            [chest[0] + world_off[0], chest[1] + world_off[1], chest[2] + world_off[2]],
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
        props.push(PropFrame { id: p.id.clone(), pos });
    }

    let flicker = sched.flicker.iter().any(|(s, e)| t >= *s && t < *e);

    FrameState {
        chars: posed,
        camera_eye: eye,
        camera_look: look,
        props,
        flicker,
        elevator_open,
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
        "establish" | "comedic_wide" | "group_coverage" => (0.0, 1.5, 5.2),
        "speaker_closeup" | "follow" | "conversation" => (0.0, 0.3, 2.7),
        "reaction" => {
            let r = react.unwrap_or([0.0, 1.5, 0.0]);
            // approach the reactor from its side, slightly closer
            (r[0].signum().max(1.0) * 1.2, 0.4, 2.4)
        }
        "reveal" | "insert_object" => (0.8, 0.2, 2.0),
        "tension_push" => (0.0, 0.4, 1.9),
        "cliffhanger_hold" => (0.0, 0.5, 2.1),
        "over_the_shoulder" => (-1.0, 0.4, 2.4),
        "exit_transition" => (0.0, 1.5, 4.0),
        _ => (0.0, 0.4, 2.6),
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
    // Elevator cabin sits at x≈3, z in [-3.0, -1.0]; keep the camera in front.
    if e[2] < -1.0 && e[0] > 2.0 && e[0] < 4.0 {
        e[2] = -0.6;
    }
    e
}
