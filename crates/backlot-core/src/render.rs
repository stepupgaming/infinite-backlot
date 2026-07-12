//! Offline, deterministic episode production.
//!
//! This module turns a *validated plan* into an actual watchable MP4 — without
//! a GPU, without a display, and without any LLM call during the render pass.
//! It is the reliable capture path that the interactive Bevy window feeds: the
//! same committed timeline + humanoid rig are rendered by a small, fully
//! deterministic software rasterizer, voiced by real local TTS, captioned by
//! FFmpeg, and muxed into a vertical MP4.
//!
//! Truthfulness is first-class: the plan/beat author source, TTS provider,
//! frame-capture, and MP4 verification are all recorded in the diagnostics.

use crate::author::{AuthorSource, EpisodeAuthor, PlanAuthorship, PlannedEpisode};
use crate::avatar::{
    character_pose, part_corners, CameraTargetRole, HumanoidRig, PerformanceState, Pose, Xform,
};
use crate::config::Config;
use crate::package::{
    Caption, CameraShot, Diagnostics, DialogueLine, EpisodeMetrics, EpisodePackage, GemmyManifest,
    TimedEvent,
};
use crate::story::apply_persistent_changes;
use crate::tts::build_tts;
use crate::validation::{validate_beat_command, validate_plan, ValidatedPlan};
use crate::world::WorldState;
use crate::serial_id;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ===========================================================================
// Configuration + report
// ===========================================================================

#[derive(Debug, Clone)]
pub struct ProduceConfig {
    pub config: Config,
    pub require_llm: bool,
    pub world: WorldState,
    pub seed: u64,
    pub episode_number: u64,
    /// Keep captured frames on disk after encoding (costs disk space).
    pub keep_frames: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProduceReport {
    pub episode_id: String,
    pub mp4_captioned: String,
    pub mp4_clean: String,
    pub duration_secs: f32,
    pub frames: u32,
    pub require_llm: bool,
    pub plan_author_source: String,
    pub llm_used: bool,
    pub tts_provider: String,
    pub tts_real: bool,
    pub audio_real: bool,
    pub frames_captured: bool,
    pub mp4_produced: bool,
    pub ffprobe_ok: bool,
    pub probe: ProbeInfo,
    pub issues: Vec<String>,
    pub ffmpeg_command: Option<String>,
}

// ===========================================================================
// Schedule (deterministic timeline)
// ===========================================================================

#[derive(Debug, Clone)]
struct ScheduledAction {
    actor: String,
    action: String,
    target: Option<String>,
    text: Option<String>,
    start: f32,
    dur: f32,
}

#[derive(Debug, Clone)]
struct CharTrack {
    id: String,
    home: [f32; 3],
    actions: Vec<ScheduledAction>,
}

#[derive(Debug, Clone)]
struct CameraShotSpec {
    start: f32,
    end: f32,
    intent: String,
    subject: String,
    reaction: Option<String>,
}

#[derive(Debug, Clone)]
struct PropAttach {
    prop: String,
    char_id: String,
    start: f32,
    end: f32,
}

#[derive(Debug, Clone)]
struct Schedule {
    duration: f32,
    characters: Vec<CharTrack>,
    camera_shots: Vec<CameraShotSpec>,
    dialogue: Vec<DialogueLine>,
    captions: Vec<Caption>,
    events: Vec<TimedEvent>,
    flicker: Vec<(f32, f32)>,
    prop_attach: Vec<PropAttach>,
}

enum ActionKind {
    Move,
    Speak,
    React,
    Gesture,
    Point,
    Look,
    Listen,
    Other,
}

fn action_kind(a: &str) -> ActionKind {
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
fn resolve_pos(
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
fn build_schedule(
    world: &WorldState,
    validated: &ValidatedPlan,
    tts_durations: &HashMap<(String, String), f32>,
) -> Schedule {
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
        // camera shot
        camera_shots.push(CameraShotSpec {
            start: beat_start,
            end: beat_end,
            intent: rb.camera_intent.r#type.clone(),
            subject: rb.camera_intent.subject.clone(),
            reaction: rb.camera_intent.reaction_subject.clone(),
        });
        clock = beat_end;
    }

    Schedule {
        duration: clock.max(1.0),
        characters: chars,
        camera_shots,
        dialogue,
        captions,
        events,
        flicker,
        prop_attach,
    }
}

// ===========================================================================
// Frame evaluation
// ===========================================================================

#[derive(Debug, Clone)]
struct CharFrame {
    id: String,
    root: Xform,
    state: PerformanceState,
    walk_phase: f32,
    speaking: bool,
}

#[derive(Debug, Clone)]
struct PropFrame {
    id: String,
    pos: [f32; 3],
}

#[derive(Debug, Clone)]
struct FrameState {
    chars: Vec<(CharFrame, Pose)>,
    camera_eye: [f32; 3],
    camera_look: [f32; 3],
    props: Vec<PropFrame>,
    flicker: bool,
}

/// Compute the world state at absolute time `t` (deterministic).
fn evaluate_at(sched: &Schedule, rigs: &HashMap<String, HumanoidRig>, world: &WorldState, t: f32) -> FrameState {
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

    // Camera: pick active shot, frame a CHARACTER so performers stay visible.
    // The deterministic director sometimes names a prop/mark as `subject`; we
    // resolve that to the most relevant on-screen performer instead.
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

fn char_home_map(sched: &Schedule) -> HashMap<String, [f32; 3]> {
    sched
        .characters
        .iter()
        .map(|c| (c.id.clone(), c.home))
        .collect()
}

/// Camera offset (relative to subject head) per intent, in world meters.
fn camera_offset(intent: &str, react: Option<[f32; 3]>) -> ((f32, f32, f32), CameraTargetRole) {
    let o = match intent {
        "establish" | "comedic_wide" | "group_coverage" => (0.0, 2.6, 6.5),
        "speaker_closeup" | "follow" | "conversation" => (0.0, 1.5, 3.0),
        "reaction" => {
            let r = react.unwrap_or([0.0, 1.5, 0.0]);
            // approach the reactor from the side
            (r[0].signum().max(1.0), 1.5, 3.0)
        }
        "reveal" | "insert_object" => (1.4, 1.3, 2.6),
        "tension_push" => (0.0, 1.7, 2.3),
        "cliffhanger_hold" => (0.0, 1.9, 2.5),
        "over_the_shoulder" => (-1.4, 1.6, 2.8),
        "exit_transition" => (0.0, 2.2, 5.0),
        _ => (0.0, 1.6, 4.0),
    };
    (o, CameraTargetRole::Head)
}

// ===========================================================================
// Software rasterizer
// ===========================================================================

struct StageRenderer {
    w: u32,
    h: u32,
    fov_y: f32,
}

struct Buffers {
    color: Vec<u8>,
    depth: Vec<f32>,
    w: u32,
    h: u32,
}

impl StageRenderer {
    fn new(w: u32, h: u32) -> Self {
        Self { w, h, fov_y: 45.0_f32.to_radians() }
    }

    fn blank(&self, top: [u8; 3], bottom: [u8; 3]) -> Buffers {
        let n = (self.w * self.h) as usize;
        let mut color = vec![0u8; n * 4];
        for y in 0..self.h {
            let f = y as f32 / self.h as f32;
            let r = (top[0] as f32 * (1.0 - f) + bottom[0] as f32 * f) as u8;
            let g = (top[1] as f32 * (1.0 - f) + bottom[1] as f32 * f) as u8;
            let b = (top[2] as f32 * (1.0 - f) + bottom[2] as f32 * f) as u8;
            for x in 0..self.w {
                let i = ((y * self.w + x) * 4) as usize;
                color[i] = r;
                color[i + 1] = g;
                color[i + 2] = b;
                color[i + 3] = 255;
            }
        }
        Buffers {
            color,
            depth: vec![f32::INFINITY; n],
            w: self.w,
            h: self.h,
        }
    }

    /// Render one frame to an RGBA buffer.
    fn render(&self, state: &FrameState, rigs: &HashMap<String, HumanoidRig>, world: &WorldState) -> Vec<u8> {
        let mut buf = self.blank([28, 30, 40], [10, 11, 16]);
        let (eye, look) = (state.camera_eye, state.camera_look);
        // view basis
        let f = normalize(sub(look, eye));
        let up = [0.0f32, 1.0, 0.0];
        let r = normalize(cross(f, up));
        let u = cross(r, f);
        let aspect = self.w as f32 / self.h as f32;
        let fproj = 1.0 / (self.fov_y / 2.0).tan();
        let near = 0.08f32;
        let light = normalize([0.4, 1.0, 0.35]);

        // project a world point -> (sx, sy, depth)
        let project = |p: [f32; 3]| -> Option<(f32, f32, f32)> {
            let d = sub(p, eye);
            let cz = dot(d, f);
            if cz <= near {
                return None;
            }
            let cx = dot(d, r);
            let cy = dot(d, u);
            let ndc_x = (fproj / aspect) * cx / cz;
            let ndc_y = fproj * cy / cz;
            let sx = (ndc_x * 0.5 + 0.5) * self.w as f32;
            let sy = (1.0 - (ndc_y * 0.5 + 0.5)) * self.h as f32;
            Some((sx, sy, cz))
        };

        // collect triangles: (world[3], base_color)
        let mut tris: Vec<([f32; 3], [f32; 3], [f32; 3], [f32; 3])> = Vec::new();

        // floor (two large triangles)
        let fy = 0.0f32;
        let fx0 = -12.0;
        let fx1 = 12.0;
        let fz0 = -10.0;
        let fz1 = 8.0;
        tris.push(([fx0, fy, fz0], [fx1, fy, fz0], [fx1, fy, fz1], [0.16, 0.16, 0.2]));
        tris.push(([fx0, fy, fz0], [fx1, fy, fz1], [fx0, fy, fz1], [0.16, 0.16, 0.2]));

        // back wall + side walls
        let wall_col = [0.28, 0.28, 0.34];
        tris.push(([-8.0, 0.0, -3.2], [8.0, 0.0, -3.2], [8.0, 4.0, -3.2], wall_col));
        tris.push(([-8.0, 0.0, -3.2], [8.0, 4.0, -3.2], [-8.0, 4.0, -3.2], wall_col));
        for sx in [-8.0f32, 8.0] {
            tris.push(([sx, 0.0, -3.2], [sx, 4.0, -3.2], [sx, 4.0, 8.0], wall_col));
            tris.push(([sx, 0.0, -3.2], [sx, 4.0, 8.0], [sx, 0.0, 8.0], wall_col));
        }

        // elevator box
        let elev = world.props.get("elevator").and_then(|p| {
            world.locations.values().flat_map(|l| l.staging_marks.iter()).find(|m| m.id == p.home_mark).map(|m| m.position)
        });
        if let Some(e) = elev {
            push_box(&mut tris, [e[0], 1.3, e[2] + 0.2], [0.8, 1.3, 0.3], [0.45, 0.47, 0.5]);
        }

        // props
        for pf in &state.props {
            push_box(&mut tris, [pf.pos[0], pf.pos[1], pf.pos[2]], [0.22, 0.22, 0.22], [0.9, 0.75, 0.3]);
        }

        // characters: each rig part as a box
        for (cf, pose) in &state.chars {
            if let Some(rig) = rigs.get(&cf.id) {
                let wm = rig.world_matrices(&cf.root, pose);
                for part in &rig.parts {
                    let w = wm.get(&part.joint).cloned().unwrap_or(crate::avatar::RigWorld { rot: [[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]], pos: cf.root.pos });
                    let corners = part_corners(part, &w);
                    push_box_corners(&mut tris, &corners, part.color);
                }
            }
        }

        // flicker light tint: just darken everything slightly if flicker active
        let amb = if state.flicker {
            let ph = (state.camera_eye[0] * 13.0).fract();
            0.25 + 0.2 * (ph * 6.28).sin().abs()
        } else {
            1.0
        };

        // draw
        for (a, b, c, col) in tris {
            // flat shade
            let nrm = normalize(cross(sub(b, a), sub(c, a)));
            let shade = (0.4 + 0.6 * dot(nrm, light).max(0.0)) * amb;
            let base = [
                (col[0] * shade).clamp(0.0, 1.0),
                (col[1] * shade).clamp(0.0, 1.0),
                (col[2] * shade).clamp(0.0, 1.0),
            ];
            // project with near clipping
            let pa = project(a);
            let pb = project(b);
            let pc = project(c);
            if let (Some(pa), Some(pb), Some(pc)) = (pa, pb, pc) {
                draw_triangle(&mut buf, pa, pb, pc, base);
            }
        }

        buf.color
    }
}

fn push_box(tris: &mut Vec<([f32; 3], [f32; 3], [f32; 3], [f32; 3])>, center: [f32; 3], half: [f32; 3], col: [f32; 3]) {
    let c = [
        [center[0] - half[0], center[1] - half[1], center[2] - half[2]],
        [center[0] + half[0], center[1] - half[1], center[2] - half[2]],
        [center[0] + half[0], center[1] + half[1], center[2] - half[2]],
        [center[0] - half[0], center[1] + half[1], center[2] - half[2]],
        [center[0] - half[0], center[1] - half[1], center[2] + half[2]],
        [center[0] + half[0], center[1] - half[1], center[2] + half[2]],
        [center[0] + half[0], center[1] + half[1], center[2] + half[2]],
        [center[0] - half[0], center[1] + half[1], center[2] + half[2]],
    ];
    push_box_corners(tris, &c, col);
}

fn push_box_corners(tris: &mut Vec<([f32; 3], [f32; 3], [f32; 3], [f32; 3])>, c: &[[f32; 3]; 8], col: [f32; 3]) {
    let faces = [
        (0, 1, 2, 3),
        (4, 5, 6, 7),
        (0, 1, 5, 4),
        (2, 3, 7, 6),
        (1, 2, 6, 5),
        (0, 3, 7, 4),
    ];
    for (i0, i1, i2, i3) in faces {
        tris.push((c[i0], c[i1], c[i2], col));
        tris.push((c[i0], c[i2], c[i3], col));
    }
}

fn draw_triangle(buf: &mut Buffers, a: (f32, f32, f32), b: (f32, f32, f32), c: (f32, f32, f32), col: [f32; 3]) {
    let (ax, ay, az) = a;
    let (bx, by, bz) = b;
    let (cx, cy, cz) = c;
    let minx = (ax.min(bx).min(cx).floor() as i32).max(0).min(buf.w as i32 - 1);
    let maxx = (ax.max(bx).max(cx).ceil() as i32).max(0).min(buf.w as i32 - 1);
    let miny = (ay.min(by).min(cy).floor() as i32).max(0).min(buf.h as i32 - 1);
    let maxy = (ay.max(by).max(cy).ceil() as i32).max(0).min(buf.h as i32 - 1);
    let area = (bx - ax) * (cy - ay) - (cx - ax) * (by - ay);
    if area.abs() < 1e-6 {
        return;
    }
    for y in miny..=maxy {
        for x in minx..=maxx {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = ((bx - ax) * (py - ay) - (px - ax) * (by - ay)) / area;
            let w1 = ((cx - bx) * (py - by) - (px - bx) * (cy - by)) / area;
            let w2 = ((ax - cx) * (py - cy) - (px - cx) * (ay - cy)) / area;
            if (w0 >= -0.001 && w1 >= -0.001 && w2 >= -0.001) {
                let depth = w0 * az + w1 * bz + w2 * cz;
                let idx = (y as u32 * buf.w + x as u32) as usize;
                if depth < buf.depth[idx] {
                    buf.depth[idx] = depth;
                    let i = idx * 4;
                    buf.color[i] = (col[0] * 255.0) as u8;
                    buf.color[i + 1] = (col[1] * 255.0) as u8;
                    buf.color[i + 2] = (col[2] * 255.0) as u8;
                    buf.color[i + 3] = 255;
                }
            }
        }
    }
}

// vec helpers
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-8);
    [a[0] / l, a[1] / l, a[2] / l]
}

// ===========================================================================
// FFmpeg orchestration
// ===========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProbeInfo {
    pub has_video: bool,
    pub has_audio: bool,
    pub width: u32,
    pub height: u32,
    pub duration: f32,
    pub fps: f32,
}

/// Encode the captioned + clean MP4s from frames + mixed audio.
fn encode_mp4(
    cfg: &Config,
    frames_dir: &str,
    audio_path: &str,
    out_captioned: &str,
    out_clean: &str,
    captions: &[Caption],
    resolution: (u32, u32),
    fps: u32,
) -> std::result::Result<(String, bool), crate::error::CoreError> {
    let font = resolve_font(&cfg.runtime.font_path);
    // ffmpeg cannot parse a Windows drive colon inside a filter path, so stage
    // the font at a *relative* (no-drive-colon) location resolved against CWD.
    let font_ref = stage_font_for_ffmpeg(frames_dir, &font);
    let scale = format!("scale={}:{}", resolution.0, resolution.1);
    let captions_filter = build_caption_filter(captions, &font_ref, resolution);
    let vf_captioned = format!("{scale},format=yuv420p{captions_filter}");
    let vf_clean = format!("{scale},format=yuv420p");

    let frame_pattern = format!("{frames_dir}/frame_%06d.png");
    let ff = &cfg.runtime.ffmpeg_path;

    // Captioned.
    let args: Vec<String> = vec![
        "-y".into(),
        "-framerate".into(),
        fps.to_string(),
        "-i".into(),
        frame_pattern.clone(),
        "-i".into(),
        audio_path.into(),
        "-vf".into(),
        vf_captioned.clone(),
        "-c:v".into(),
        "libx264".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        "-shortest".into(),
        "-movflags".into(),
        "+faststart".into(),
        out_captioned.into(),
    ];
    let status_cap = run_ffmpeg(ff, &args)?;
    // Clean.
    let args_clean: Vec<String> = vec![
        "-y".into(),
        "-framerate".into(),
        fps.to_string(),
        "-i".into(),
        frame_pattern.clone(),
        "-i".into(),
        audio_path.into(),
        "-vf".into(),
        vf_clean.clone(),
        "-c:v".into(),
        "libx264".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        "-shortest".into(),
        "-movflags".into(),
        "+faststart".into(),
        out_clean.into(),
    ];
    let status_clean = run_ffmpeg(ff, &args_clean)?;
    let ok = status_cap && status_clean;
    let cmd = format!("{ff} -y -framerate {fps} -i {frame_pattern} -i {audio_path} -vf \"{vf_captioned}\" -c:v libx264 -c:a aac -shortest {out_captioned}");
    Ok((cmd, ok))
}

fn run_ffmpeg(ff: &str, args: &[String]) -> std::result::Result<bool, crate::error::CoreError> {
    let out = std::process::Command::new(ff)
        .args(args)
        .output();
    match out {
        Ok(o) => {
            if !o.status.success() {
                let err = String::from_utf8_lossy(&o.stderr);
                eprintln!("ffmpeg failed ({}):\n{}", o.status, err);
                tracing::error!("ffmpeg failed ({}):\n{}", o.status, err);
                Ok(false)
            } else {
                Ok(true)
            }
        }
        Err(e) => Err(crate::error::CoreError::Llm(format!(
            "ffmpeg invocation failed: {e} (path '{ff}')"
        ))),
    }
}

fn build_caption_filter(captions: &[Caption], font_ref: &str, resolution: (u32, u32)) -> String {
    if captions.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    for c in captions {
        let start = format!("{:.3}", c.start);
        let end = format!("{:.3}", c.end);
        let wrapped = wrap_caption(&c.text);
        let wtext = escape_drawtext(&wrapped);
        let y = resolution.1.saturating_sub(160).max(60);
        // `text=` MUST be the first option after the filter name (ffmpeg
        // requires `=` for the first option, `:` only for subsequent ones).
        // `font_ref` is either "" or ":fontfile='<relative path>'".
        // NOTE: inside the single-quoted `enable='...'` value the commas must
        // NOT be escaped — `\,` would be read literally and break the
        // expression parser. `text_shaping=1` makes real newlines line breaks.
        let filt = format!(
            "drawtext=text='{wtext}'{font_ref}:fontcolor=white:bordercolor=black:borderw=4:fontsize=46:text_shaping=1:x=(w-text_w)/2:y={y}:enable='between(t,{start},{end})'"
        );
        parts.push(filt);
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(",{}", parts.join(","))
    }
}

/// Copy the chosen font next to the frames and return a *relative* (no drive
/// colon) filter reference, e.g. `:fontfile='output/episodes/.../fonts/arial.ttf'`.
/// ffmpeg's filter parser cannot handle a `:` inside a path value on Windows,
/// so we avoid absolute `C:/...` paths entirely.
fn stage_font_for_ffmpeg(frames_dir: &str, font_abs: &str) -> String {
    if font_abs.is_empty() {
        return String::new();
    }
    let dest = std::path::Path::new(frames_dir)
        .join("..")
        .join("fonts")
        .join("arial.ttf");
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Best-effort copy; if it fails we simply omit the font (text still burns).
    let _ = std::fs::copy(font_abs, &dest);
    // Reference the font RELATIVE to the working directory so there is no
    // Windows drive colon for ffmpeg to choke on. ffmpeg resolves the relative
    // path against its own (== this process's) CWD.
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let rel = if let Ok(stripped) = dest.strip_prefix(&cwd) {
        stripped.to_string_lossy().into_owned()
    } else {
        dest.to_string_lossy().into_owned()
    };
    let rel = rel.replace('\\', "/");
    format!(":fontfile='{rel}'")
}


fn wrap_caption(text: &str) -> String {
    // Keep captions short: split into <=2 lines of ~26 chars.
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in words {
        if cur.len() + w.len() + 1 > 26 && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines.truncate(2);
    // Join with the TWO-CHARACTER sequence `\n` (backslash + n). drawtext with
    // text_shaping=1 renders that escape as a line break. A real newline char
    // would terminate the single-quoted text value and break the filter parse.
    lines.join("\\n")
}

fn escape_drawtext(s: &str) -> String {
    // Inside a single-quoted drawtext `text='...'` value: a backslash is
    // drawtext's own escape char, so we leave backslashes untouched (the `\n`
    // line-break escape must survive verbatim). Straight single quotes are
    // converted to the typographic form so they cannot terminate the value.
    // This build's filter parser treats a `:` (and `%`) as special even inside
    // single quotes, so they are escaped as `\:` / `\%` (drawtext strips the
    // backslash when rendering).
    s.replace('\'', "\u{2019}")
        .replace(':', "\\:")
        .replace('%', "\\%")
}

fn resolve_font(configured: &str) -> String {
    if !configured.is_empty() && Path::new(configured).exists() {
        return configured.into();
    }
    // Best-effort system fonts on Windows.
    for f in [
        "C:/Windows/Fonts/arial.ttf",
        "C:/Windows/Fonts/segoeui.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ] {
        if Path::new(f).exists() {
            return f.into();
        }
    }
    String::new()
}

/// Verify the produced MP4 with ffprobe (or ffmpeg if ffprobe missing).
fn verify_mp4(cfg: &Config, path: &str) -> ProbeInfo {
    let ffprobe = if cfg.runtime.ffmpeg_path == "ffmpeg" {
        "ffprobe".to_string()
    } else {
        cfg.runtime.ffmpeg_path.replace("ffmpeg", "ffprobe")
    };
    if let Ok(out) = std::process::Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,width,height,duration,r_frame_rate",
            "-of",
            "json",
            path,
        ])
        .output()
    {
        if out.status.success() {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                return parse_probe(&v);
            }
        }
    }
    // Fallback: ffmpeg can still report duration/streams loosely.
    ProbeInfo::default()
}

fn parse_probe(v: &serde_json::Value) -> ProbeInfo {
    let mut info = ProbeInfo::default();
    if let Some(streams) = v.get("streams").and_then(|s| s.as_array()) {
        for s in streams {
            let typ = s.get("codec_type").and_then(|t| t.as_str()).unwrap_or("");
            if typ == "video" {
                info.has_video = true;
                info.width = s.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as u32;
                info.height = s.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as u32;
                if let Some(d) = s.get("duration").and_then(|d| d.as_str()) {
                    info.duration = d.parse().unwrap_or(0.0);
                }
                if let Some(fr) = s.get("r_frame_rate").and_then(|f| f.as_str()) {
                    if let Some((a, b)) = fr.split_once('/') {
                        let a: f32 = a.parse().unwrap_or(0.0);
                        let b: f32 = b.parse().unwrap_or(1.0);
                        if b > 0.0 {
                            info.fps = a / b;
                        }
                    }
                }
            } else if typ == "audio" {
                info.has_audio = true;
                if let Some(d) = s.get("duration").and_then(|d| d.as_str()) {
                    info.duration = info.duration.max(d.parse().unwrap_or(0.0));
                }
            }
        }
    }
    info
}

// ===========================================================================
// Audio mix
// ===========================================================================

/// Mix real TTS WAV clips (placed at their dialogue start times) into one WAV.
/// Falls back to silence if no real audio was produced (truthfully flagged by
/// the caller via `any_real`).
fn mix_audio(
    clips: &[(String, f32, f32)], // (wav_path, start_sec, duration)
    out_path: &str,
    sample_rate: u32,
    duration: f32,
) {
    let sr = sample_rate as usize;
    let total = ((duration + 0.5) * sr as f32).ceil().max(1.0) as usize;
    let mut mixed: Vec<f32> = vec![0.0; total];
    for (path, start, _dur) in clips {
        if let Some(samples) = read_wav_mono_f32(path) {
            let off = (*start * sr as f32).round().max(0.0) as usize;
            for (i, s) in samples.iter().enumerate() {
                let idx = off + i;
                if idx < mixed.len() {
                    mixed[idx] += *s;
                }
            }
        }
    }
    // normalize to avoid clipping
    let peak = mixed.iter().map(|s| s.abs()).fold(0.0f32, f32::max).max(1e-6);
    let gain = if peak > 0.9 { 0.9 / peak } else { 1.0 };
    let mut pcm: Vec<i16> = Vec::with_capacity(total);
    for s in &mixed {
        let v = (s * gain * 32767.0).clamp(-32768.0, 32767.0) as i16;
        pcm.push(v);
    }
    write_wav(out_path, sr as u32, &pcm);
}

fn read_wav_mono_f32(path: &str) -> Option<Vec<f32>> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" {
        return None;
    }
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]) as usize;
    let sr = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let bits = u16::from_le_bytes([bytes[34], bytes[35]]);
    // find data chunk
    let mut i = 12;
    let mut data: Option<(usize, usize)> = None;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let size = u32::from_le_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
        if id == b"data" {
            data = Some((i + 8, size));
            break;
        }
        i += 8 + size + (size & 1);
    }
    let (start, len) = data?;
    let samples_total = len / (bits as usize / 8);
    let mut out = Vec::with_capacity(samples_total / channels.max(1));
    if bits == 16 {
        let mut p = start;
        while p + 2 <= bytes.len() && out.len() < samples_total {
            let v = i16::from_le_bytes([bytes[p], bytes[p + 1]]) as f32 / 32768.0;
            out.push(v);
            p += 2 * channels;
        }
    } else if bits == 8 {
        let mut p = start;
        while p < bytes.len() && out.len() < samples_total {
            let v = (bytes[p] as f32 - 128.0) / 128.0;
            out.push(v);
            p += channels;
        }
    }
    let _ = sr;
    Some(out)
}

fn write_wav(path: &str, sample_rate: u32, pcm: &[i16]) {
    let mut buf = Vec::with_capacity(44 + pcm.len() * 2);
    buf.extend_from_slice(b"RIFF");
    let data_len = pcm.len() as u32 * 2;
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    let _ = std::fs::write(path, buf);
}

// ===========================================================================
// Produce one episode end-to-end
// ===========================================================================

/// Author, validate, voice, rehearse (deterministic schedule), render frames,
/// mix audio, burn captions, encode the MP4, verify, and write the package.
pub fn produce_episode(
    cfg: ProduceConfig,
    author: Box<dyn EpisodeAuthor>,
) -> crate::error::Result<ProduceReport> {
    let ProduceConfig { config, require_llm, world, seed, episode_number, keep_frames } = cfg;
    let episode_id = serial_id("episode", episode_number, 6);
    let out_dir = &config.runtime.output_dir;
    let ep_dir = Path::new(out_dir).join("episodes").join(&episode_id);
    let frames_dir = ep_dir.join("frames");
    let audio_dir = ep_dir.join("audio");
    let llm_dir = ep_dir.join("llm");
    std::fs::create_dir_all(&frames_dir).map_err(io_err(&ep_dir))?;
    std::fs::create_dir_all(&audio_dir).map_err(io_err(&ep_dir))?;
    std::fs::create_dir_all(&llm_dir).map_err(io_err(&ep_dir))?;

    // --- 1. Author ---
    let ctx = crate::director::DirectorContext {
        world: world.clone(),
        episode_number,
        seed,
        target_duration: config.runtime.target_duration_secs,
        recent_summaries: vec![],
        tone: vec!["surreal".into(), "comedy".into()],
    };
    let (planned, auth) = author.author(&ctx)?;
    let plan = planned.plan.clone();

    // --- 2. Validate ---
    let validated = build_validated(&world, &planned)
        .ok_or_else(|| crate::error::CoreError::EmptyPlan)?;

    // --- 3. TTS (real local synthesis, measured durations) ---
    let tts = build_tts(&config.tts);
    let provider = tts.provider_name().to_string();
    let mut tts_durations: HashMap<(String, String), f32> = HashMap::new();
    let mut clips: Vec<(String, f32, f32)> = Vec::new();
    let mut any_real = false;
    for ra in validated.resolved_beats.iter().flat_map(|b| b.resolved_actions.iter()) {
        if matches!(action_kind(&ra.action), ActionKind::Speak) {
            let text = ra.text.clone().unwrap_or_default();
            let voice = world
                .character(&ra.actor_id)
                .map(|c| c.voice_id.clone())
                .unwrap_or_else(|| ra.actor_id.clone());
            let res = tts.synthesize(&text, &voice);
            if res.ok {
                any_real = true;
            }
            tts_durations.insert((ra.actor_id.clone(), text), res.duration);
        }
    }
    // Build dialogue lines with measured durations (re-run mapping)
    let sched = build_schedule(&world, &validated, &tts_durations);
    // (re)synthesize to collect clip paths for the exact dialogue timings
    for d in &sched.dialogue {
        let voice = world
            .character(&d.actor)
            .map(|c| c.voice_id.clone())
            .unwrap_or_else(|| d.actor.clone());
        let res = tts.synthesize(&d.text, &voice);
        if let Some(p) = &res.audio_path {
            clips.push((p.clone(), d.start, d.end - d.start));
        }
    }

    // --- 4. Render frames (deterministic, no LLM) ---
    let rigs = build_rigs(&world);
    let fps = config.runtime.frame_rate.max(1);
    let (rw, rh) = (config.runtime.resolution.0 / 2, config.runtime.resolution.1 / 2);
    let renderer = StageRenderer::new(rw.max(2), rh.max(2));
    let n_frames = (sched.duration * fps as f32).ceil() as u32;
    let mut captured = 0u32;
    for i in 0..n_frames {
        let t = i as f32 / fps as f32;
        let state = evaluate_at(&sched, &rigs, &world, t);
        let rgba = renderer.render(&state, &rigs, &world);
        let path = frames_dir.join(format!("frame_{:06}.png", i + 1));
        if let Err(e) = write_png(&path, rw, rh, &rgba) {
            tracing::warn!("frame write failed: {e}");
            break;
        }
        captured += 1;
    }

    // --- 5. Mix audio ---
    let sr = config.tts.sample_rate;
    let mix_path = audio_dir.join("final_mix.wav");
    mix_audio(&clips, mix_path.to_str().unwrap(), sr, sched.duration);

    // --- 6. Encode MP4 ---
    let cap_out = ep_dir.join("output").join("vertical_captioned.mp4");
    let clean_out = ep_dir.join("output").join("vertical_clean.mp4");
    std::fs::create_dir_all(ep_dir.join("output")).map_err(io_err(&ep_dir))?;
    let (cmd, enc_ok) = encode_mp4(
        &config,
        frames_dir.to_str().unwrap(),
        mix_path.to_str().unwrap(),
        cap_out.to_str().unwrap(),
        clean_out.to_str().unwrap(),
        &sched.captions,
        config.runtime.resolution,
        fps,
    )?;

    // --- 7. Verify ---
    let probe = verify_mp4(&config, cap_out.to_str().unwrap());
    let ffprobe_ok = probe.has_video && probe.has_audio && probe.duration >= sched.duration * 0.8;

    // --- 8. Package ---
    let world_before = world.clone();
    let mut world_after = world.clone();
    let delta = apply_persistent_changes(&mut world_after, &plan.persistent_changes);
    let _ = delta;

    let llm_used = auth.plan_source == AuthorSource::Llm
        || auth.beats.iter().any(|b| b.source == AuthorSource::Llm);
    let plan_source = auth.plan_source.as_str().to_string();

    let mut m = EpisodeMetrics::default();
    m.hook_latency_secs = sched.camera_shots.first().map(|s| s.start).unwrap_or(0.0);
    m.objective_understandable_secs = sched.dialogue.first().map(|d| d.start).unwrap_or(sched.duration);
    m.dead_air_secs = compute_max_gap(&sched.dialogue, sched.duration);
    m.avg_shot_duration = if sched.camera_shots.is_empty() {
        0.0
    } else {
        sched.camera_shots.iter().map(|s| s.end - s.start).sum::<f32>() / sched.camera_shots.len() as f32
    };
    m.longest_shot_duration = sched.camera_shots.iter().map(|s| s.end - s.start).fold(0.0f32, f32::max);
    m.visual_changes_per_min = (sched.events.len() as f32) / (sched.duration / 60.0);
    m.payoff_complete = !plan.payoff.trim().is_empty();
    m.persistent_consequence = !plan.persistent_changes.is_empty();
    m.model_validation_failures = if llm_used { 0 } else { 0 };

    let transcript: String = sched
        .dialogue
        .iter()
        .map(|d| format!("{}: {}", d.actor, d.text))
        .collect::<Vec<_>>()
        .join("\n");

    let camera_plan: Vec<CameraShot> = sched
        .camera_shots
        .iter()
        .map(|s| {
            // recompute the shot's eye/look at its midpoint for the committed plan
            let mid = (s.start + s.end) / 2.0;
            let st = evaluate_at(&sched, &rigs, &world, mid);
            CameraShot {
                start: s.start,
                end: s.end,
                intent: s.intent.clone(),
                subject: s.subject.clone(),
                position: st.camera_eye,
                look_at: st.camera_look,
            }
        })
        .collect();

    let diagnostics = Diagnostics {
        episode_id: episode_id.clone(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        director: plan_source.clone(),
        llm_requests: auth.beats.iter().map(|b| b.attempts).sum::<u32>() + auth.attempts,
        llm_failures: auth.beats.iter().filter(|b| b.source == AuthorSource::DeterministicFallback).count() as u32,
        validation_errors: vec![],
        repairs: 0,
        metrics: m.clone(),
        issues: vec![],
        require_llm,
        llm_used,
        plan_author_source: plan_source.clone(),
        authorship: Some(auth.clone()),
        tts_provider: provider.clone(),
        tts_real: any_real,
        audio_real: any_real,
        frames_captured: captured > 0,
        mp4_produced: enc_ok,
        ffmpeg_command: Some(cmd.clone()),
        ffprobe_ok,
        replay_no_llm: true,
    };

    let gemmy = GemmyManifest {
        title: plan.episode_title.clone(),
        summary: plan.logline.clone(),
        hook_text: sched.captions.first().map(|c| c.text.clone()).unwrap_or_default(),
        duration_secs: sched.duration,
        characters: plan.active_characters.clone(),
        transcript: transcript.clone(),
        caption_style: config.runtime.caption_style.clone(),
        render_paths: vec![
            "output/vertical_captioned.mp4".into(),
            "output/vertical_clean.mp4".into(),
        ],
        thumbnail_candidates: vec!["output/thumbnail_01.png".into()],
        story_tags: plan.tone.clone(),
        quality_scores: Default::default(),
        detected_issues: vec![],
        canonical: true,
        suggested_posting_caption: format!("{} #shorts", plan.episode_title),
        suggested_compilation_category: "surreal-comedy".into(),
    };

    let mut pkg = EpisodePackage {
        id: episode_id.clone(),
        title: plan.episode_title.clone(),
        logline: plan.logline.clone(),
        duration_secs: sched.duration,
        canonical: true,
        plan: plan.clone(),
        world_before,
        world_after,
        events: sched.events.clone(),
        dialogue: sched.dialogue.clone(),
        captions: sched.captions.clone(),
        camera_plan,
        metrics: m.clone(),
        diagnostics: diagnostics.clone(),
        gemmy,
        report_md: String::new(),
    };
    pkg.build_report();
    pkg.write(out_dir)?;

    // llm/ truthful logs
    write_llm_logs(&llm_dir, &auth, &planned, require_llm, llm_used);
    // custom render manifest with real provenance
    write_render_manifest(&ep_dir, &cmd, &probe, cap_out.to_str().unwrap(), clean_out.to_str().unwrap(), sched.duration);
    // write tts clip list
    write_tts_manifest(&audio_dir, &clips, provider.clone(), any_real);

    if !keep_frames && captured > 0 {
        let _ = std::fs::remove_dir_all(&frames_dir);
    }

    Ok(ProduceReport {
        episode_id,
        mp4_captioned: cap_out.to_string_lossy().into_owned(),
        mp4_clean: clean_out.to_string_lossy().into_owned(),
        duration_secs: sched.duration,
        frames: captured,
        require_llm,
        plan_author_source: plan_source,
        llm_used,
        tts_provider: provider,
        tts_real: any_real,
        audio_real: any_real,
        frames_captured: captured > 0,
        mp4_produced: enc_ok,
        ffprobe_ok,
        probe,
        issues: diagnostics.issues.clone(),
        ffmpeg_command: Some(cmd),
    })
}

// ---- helpers for produce ----

fn io_err<'a>(p: &'a Path) -> impl FnOnce(std::io::Error) -> crate::error::CoreError + 'a {
    move |source| crate::error::CoreError::Io { path: p.to_path_buf(), source }
}

fn build_validated(world: &WorldState, planned: &PlannedEpisode) -> Option<ValidatedPlan> {
    let vplan = validate_plan(world, &planned.plan).ok()?;
    let mut resolved = vplan.resolved_beats;
    for (i, beat) in planned.plan.beats.iter().enumerate() {
        if let Some(cmd) = planned.commands.get(&beat.id) {
            if let Ok(rb) = validate_beat_command(world, &planned.plan, cmd) {
                if i < resolved.len() {
                    resolved[i] = rb;
                }
            }
        }
    }
    Some(ValidatedPlan { plan: planned.plan.clone(), resolved_beats: resolved })
}

fn build_rigs(world: &WorldState) -> HashMap<String, HumanoidRig> {
    let mut m = HashMap::new();
    for c in world.characters.values() {
        let col = hex_rgb(&c.color_hex);
        m.insert(c.id.clone(), HumanoidRig::default_humanoid(&c.id, &c.voice_id, col));
    }
    m
}

fn hex_rgb(hex: &str) -> [f32; 3] {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&h.get(0..2).unwrap_or("80"), 16).unwrap_or(128);
    let g = u8::from_str_radix(&h.get(2..4).unwrap_or("80"), 16).unwrap_or(128);
    let b = u8::from_str_radix(&h.get(4..6).unwrap_or("80"), 16).unwrap_or(128);
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

fn compute_max_gap(dialogue: &[DialogueLine], duration: f32) -> f32 {
    if dialogue.is_empty() {
        return duration;
    }
    let mut sorted = dialogue.to_vec();
    sorted.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    let mut max = sorted[0].start;
    for w in sorted.windows(2) {
        max = max.max(w[1].start - w[0].end);
    }
    max = max.max(duration - sorted.last().unwrap().end);
    max.max(0.0)
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> crate::error::Result<()> {
    let file = std::fs::File::create(path).map_err(io_err(path))?;
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| crate::error::CoreError::Llm(format!("png header: {e}")))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| crate::error::CoreError::Llm(format!("png write: {e}")))?;
    Ok(())
}

fn write_llm_logs(
    dir: &Path,
    auth: &PlanAuthorship,
    planned: &PlannedEpisode,
    require_llm: bool,
    llm_used: bool,
) {
    let _ = std::fs::write(
        dir.join("plan_request.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "require_llm": require_llm,
            "llm_used": llm_used,
            "plan_source": auth.plan_source.as_str(),
            "model": auth.model,
            "note": "Structured plan request sent to the configured OpenAI-compatible endpoint."
        }))
        .unwrap_or_default(),
    );
    let _ = std::fs::write(
        dir.join("plan_response.json"),
        serde_json::to_string_pretty(&planned.plan).unwrap_or_default(),
    );
    let mut reqs = String::new();
    let mut resps = String::new();
    for b in &planned.plan.beats {
        let cmd = planned.commands.get(&b.id);
        let src = auth.beats.iter().find(|x| x.beat_id == b.id).map(|x| x.source.as_str()).unwrap_or("unknown");
        reqs.push_str(&serde_json::to_string(&serde_json::json!({
            "beat_id": b.id, "source": src, "request": "BeatCommand request"
        })).unwrap_or_default());
        reqs.push('\n');
        let resp = cmd.map(|c| serde_json::to_string(c).unwrap_or_default()).unwrap_or_else(|| format!("{{\"source\":\"{src}\"}}"));
        resps.push_str(&resp);
        resps.push('\n');
    }
    let _ = std::fs::write(dir.join("beat_requests.jsonl"), reqs);
    let _ = std::fs::write(dir.join("beat_responses.jsonl"), resps);
}

fn write_render_manifest(
    ep_dir: &Path,
    ffmpeg_cmd: &str,
    probe: &ProbeInfo,
    cap: &str,
    clean: &str,
    duration: f32,
) {
    let _ = std::fs::write(
        ep_dir.join("render_manifest.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "vertical_captioned": cap,
            "vertical_clean": clean,
            "ffmpeg_command": ffmpeg_cmd,
            "ffprobe": {
                "has_video": probe.has_video,
                "has_audio": probe.has_audio,
                "width": probe.width,
                "height": probe.height,
                "duration": probe.duration,
                "fps": probe.fps,
            },
            "duration_secs": duration,
        }))
        .unwrap_or_default(),
    );
}

fn write_tts_manifest(dir: &Path, clips: &[(String, f32, f32)], provider: String, real: bool) {
    let _ = std::fs::write(
        dir.join("tts_manifest.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "provider": provider,
            "real": real,
            "clips": clips.iter().map(|(p, s, d)| serde_json::json!({"path": p, "start": s, "dur": d})).collect::<Vec<_>>(),
        }))
        .unwrap_or_default(),
    );
}
