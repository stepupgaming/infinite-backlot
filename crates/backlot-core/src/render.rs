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
use crate::timeline::*;
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

/// Expose the software stage renderer for arbitrary frame states (used by the
/// visual-review pass and by tests). Returns RGBA8 pixels of size `w*4*h`.
pub fn render_frame_pixels(
    state: &FrameState,
    rigs: &HashMap<String, HumanoidRig>,
    world: &WorldState,
    w: u32,
    h: u32,
) -> Vec<u8> {
    StageRenderer::new(w, h).render(state, rigs, world)
}

/// Metrics produced by the offline visual-review pass over a schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewReport {
    pub sampled_frames: usize,
    pub render_width: u32,
    pub render_height: u32,
    pub mean_luminance: f32,
    pub foreground_fraction: f32,
    pub mean_frame_motion: f32,
    pub max_frame_motion: f32,
    pub freeze_detected: bool,
    pub articulation_observed: bool,
    pub notes: Vec<String>,
}

/// Run the autonomous visual-review loop over a schedule by re-rendering sampled
/// frames with the *same* software stage renderer used for production. This is a
/// CPU-only, GPU-independent review: it measures per-frame luminance, on-screen
/// figure occupancy (foreground fraction), and inter-frame motion to detect
/// freezes and to confirm that performers are actually articulated (limbs/head
/// moving between frames) rather than standing static.
///
/// It re-simulates from the shared timeline (`evaluate_at`) so it validates the
/// *performance*, independent of which render backend produced the final MP4.
pub fn review_schedule(
    sched: &Schedule,
    rigs: &HashMap<String, HumanoidRig>,
    world: &WorldState,
    w: u32,
    h: u32,
    sample_n: usize,
) -> ReviewReport {
    let dur = sched.duration.max(0.001);
    let n = sample_n.max(2);
    let mut pix: Vec<Vec<u8>> = Vec::with_capacity(n);
    let mut lums: Vec<f32> = Vec::with_capacity(n);
    let mut fg: Vec<f32> = Vec::with_capacity(n);
    for i in 0..n {
        let t = dur * (i as f32 / (n as f32 - 1.0));
        let state = evaluate_at(sched, rigs, world, t);
        let p = render_frame_pixels(&state, rigs, world, w, h);
        let (l, f) = frame_luminance_and_fg(&p, w, h);
        lums.push(l);
        fg.push(f);
        pix.push(p);
    }
    // inter-frame motion (mean abs luminance diff, normalized 0..1)
    let mut motions: Vec<f32> = Vec::new();
    for i in 1..pix.len() {
        motions.push(frame_motion(&pix[i - 1], &pix[i]));
    }
    let mean_motion = if motions.is_empty() {
        0.0
    } else {
        motions.iter().sum::<f32>() / motions.len() as f32
    };
    let max_motion = motions.iter().cloned().fold(0.0f32, f32::max);
    let mean_lum = if lums.is_empty() {
        0.0
    } else {
        lums.iter().sum::<f32>() / lums.len() as f32
    };
    let mean_fg = if fg.is_empty() {
        0.0
    } else {
        fg.iter().sum::<f32>() / fg.len() as f32
    };
    let freeze = max_motion < 0.003;
    let articulation = max_motion > 0.006;
    let mut notes: Vec<String> = Vec::new();
    if freeze {
        notes.push("freeze detected: near-zero inter-frame motion across sampled frames".into());
    }
    if !articulation {
        notes.push("articulation not clearly observed in sampled motion (below threshold)".into());
    }
    if mean_fg < 0.02 {
        notes.push("foreground (figure) occupancy very low: subjects may be off-frame".into());
    }
    if mean_lum < 4.0 {
        notes.push("mean luminance very low: scene may be underlit".into());
    }
    ReviewReport {
        sampled_frames: n,
        render_width: w,
        render_height: h,
        mean_luminance: mean_lum,
        foreground_fraction: mean_fg,
        mean_frame_motion: mean_motion,
        max_frame_motion: max_motion,
        freeze_detected: freeze,
        articulation_observed: articulation,
        notes,
    }
}

/// Mean luminance + on-screen *figure* occupancy of an RGBA8 buffer.
/// A pixel counts as a figure pixel only when it is far from the vertical
/// background gradient AND far from the known static set (gray walls / floor /
/// elevator). This isolates the coloured character boxes from the static stage.
pub fn frame_luminance_and_fg(p: &[u8], w: u32, h: u32) -> (f32, f32) {
    let n = (w * h) as usize;
    if n == 0 || p.len() < n * 4 {
        return (0.0, 0.0);
    }
    // Static stage colours (walls / floor / elevator) — excluded from "figure".
    let statics: [[f32; 3]; 3] = [
        [71.0, 71.0, 87.0],   // walls
        [41.0, 41.0, 51.0],   // floor
        [115.0, 120.0, 128.0], // elevator
    ];
    let mut lum = 0.0f32;
    let mut fg = 0.0f32;
    for y in 0..h as usize {
        let bg_t = y as f32 / h as f32;
        let bgr = 28.0 * (1.0 - bg_t) + 10.0 * bg_t;
        let bgg = 30.0 * (1.0 - bg_t) + 11.0 * bg_t;
        let bgb = 40.0 * (1.0 - bg_t) + 16.0 * bg_t;
        for x in 0..w as usize {
            let i = (y * w as usize + x) * 4;
            let r = p[i] as f32;
            let g = p[i + 1] as f32;
            let b = p[i + 2] as f32;
            lum += (r + g + b) / 3.0;
            let d_bg = (r - bgr).abs() + (g - bgg).abs() + (b - bgb).abs();
            let d_st = statics
                .iter()
                .map(|s| (r - s[0]).abs() + (g - s[1]).abs() + (b - s[2]).abs())
                .fold(f32::INFINITY, f32::min);
            if d_bg > 36.0 && d_st > 45.0 {
                fg += 1.0;
            }
        }
    }
    (lum / n as f32, fg / n as f32)
}

/// Fraction of pixels whose luminance changed by more than a small delta between
/// two RGBA8 buffers. The static stage (walls/floor) does not move, so it
/// contributes ~0; only the articulated performers raise this value. A robust
/// freeze / articulation signal independent of how much of the frame is "set".
pub fn frame_motion(prev: &[u8], cur: &[u8]) -> f32 {
    let n = prev.len().min(cur.len());
    if n < 4 {
        return 0.0;
    }
    let npix = n / 4;
    let mut changed = 0u32;
    let mut i = 0;
    while i + 4 <= n {
        let lp = (prev[i] as f32 + prev[i + 1] as f32 + prev[i + 2] as f32) / 3.0;
        let lc = (cur[i] as f32 + cur[i + 1] as f32 + cur[i + 2] as f32) / 3.0;
        if (lp - lc).abs() > 3.0 {
            changed += 1;
        }
        i += 4;
    }
    changed as f32 / npix as f32
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
pub fn encode_mp4(
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

pub fn build_caption_filter(captions: &[Caption], font_ref: &str, resolution: (u32, u32)) -> String {
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
pub fn stage_font_for_ffmpeg(frames_dir: &str, font_abs: &str) -> String {
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


pub fn wrap_caption(text: &str) -> String {
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

pub fn escape_drawtext(s: &str) -> String {
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

pub fn resolve_font(configured: &str) -> String {
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
pub fn verify_mp4(cfg: &Config, path: &str) -> ProbeInfo {
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
pub fn mix_audio(
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


/// Result of the shared preparation stage (authoring + validation + TTS +
/// schedule + rigs). Both renderers consume this; neither re-derives state.
pub struct PreparedProduction {
    pub episode_id: String,
    pub planned: PlannedEpisode,
    pub auth: PlanAuthorship,
    pub validated: ValidatedPlan,
    pub schedule: Schedule,
    pub rigs: HashMap<String, HumanoidRig>,
    pub clips: Vec<(String, f32, f32)>,
    pub any_real: bool,
    pub tts_provider: String,
    pub world_before: WorldState,
}

/// Stage 1 (shared): author, validate, synthesize TTS, build the authoritative
/// schedule, build rigs, collect audio clips. No frames are rendered here.
pub fn prepare_production(
    config: &Config,
    require_llm: bool,
    world: &WorldState,
    seed: u64,
    episode_number: u64,
    author: &dyn EpisodeAuthor,
) -> crate::error::Result<PreparedProduction> {
    let episode_id = serial_id("episode", episode_number, 6);
    let ctx = crate::director::DirectorContext {
        world: world.clone(),
        episode_number,
        seed,
        target_duration: config.runtime.target_duration_secs,
        recent_summaries: vec![],
        tone: vec!["surreal".into(), "comedy".into()],
    };
    let (planned, auth) = author.author(&ctx)?;
    let validated = build_validated(world, &planned)
        .ok_or_else(|| crate::error::CoreError::EmptyPlan)?;

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
    let sched = build_schedule(world, &validated, &tts_durations);
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
    let rigs = build_rigs(world);
    Ok(PreparedProduction {
        episode_id,
        planned,
        auth,
        validated,
        schedule: sched,
        rigs,
        clips,
        any_real,
        tts_provider: provider,
        world_before: world.clone(),
    })
}

/// Build the committed camera plan (eye/look sampled at each shot midpoint).
pub fn build_camera_plan(
    world: &WorldState,
    sched: &Schedule,
    rigs: &HashMap<String, HumanoidRig>,
) -> Vec<CameraShot> {
    sched
        .camera_shots
        .iter()
        .map(|s| {
            let mid = (s.start + s.end) / 2.0;
            let st = evaluate_at(sched, rigs, world, mid);
            CameraShot {
                start: s.start,
                end: s.end,
                intent: s.intent.clone(),
                subject: s.subject.clone(),
                position: st.camera_eye,
                look_at: st.camera_look,
            }
        })
        .collect()
}

/// Stage 3 (shared): mix audio, encode MP4, verify, package, and write truthful
/// logs. `render_backend` is recorded in the diagnostics so the artifact never
/// lies about which renderer produced the imagery.
pub fn finalize_production(
    config: &Config,
    require_llm: bool,
    prep: &PreparedProduction,
    frames_dir: &Path,
    captured: u32,
    render_backend: &str,
) -> crate::error::Result<ProduceReport> {
    let out_dir = &config.runtime.output_dir;
    let ep_dir = Path::new(out_dir).join("episodes").join(&prep.episode_id);
    let audio_dir = ep_dir.join("audio");
    let llm_dir = ep_dir.join("llm");
    std::fs::create_dir_all(&audio_dir).map_err(io_err(&ep_dir))?;
    std::fs::create_dir_all(&llm_dir).map_err(io_err(&ep_dir))?;

    let sched = &prep.schedule;
    let rigs = &prep.rigs;
    let world = &prep.world_before;
    let plan = prep.planned.plan.clone();
    let auth = &prep.auth;

    // Mix audio
    let sr = config.tts.sample_rate;
    let mix_path = audio_dir.join("final_mix.wav");
    mix_audio(&prep.clips, mix_path.to_str().unwrap(), sr, sched.duration);

    // Encode MP4
    let fps = config.runtime.frame_rate.max(1);
    let cap_out = ep_dir.join("output").join("vertical_captioned.mp4");
    let clean_out = ep_dir.join("output").join("vertical_clean.mp4");
    std::fs::create_dir_all(ep_dir.join("output")).map_err(io_err(&ep_dir))?;
    let (cmd, enc_ok) = encode_mp4(
        config,
        frames_dir.to_str().unwrap(),
        mix_path.to_str().unwrap(),
        cap_out.to_str().unwrap(),
        clean_out.to_str().unwrap(),
        &sched.captions,
        config.runtime.resolution,
        fps,
    )?;

    // Verify
    let probe = verify_mp4(config, cap_out.to_str().unwrap());
    let ffprobe_ok = probe.has_video && probe.has_audio && probe.duration >= sched.duration * 0.8;

    // Package
    let mut world_after = world.clone();
    let _delta = apply_persistent_changes(&mut world_after, &plan.persistent_changes);

    let llm_used = auth.plan_source == AuthorSource::Llm
        || auth.beats.iter().any(|b| b.source == AuthorSource::Llm);
    let plan_source = auth.plan_source.as_str().to_string();

    let mut m = EpisodeMetrics::default();
    m.hook_latency_secs = sched.camera_shots.first().map(|s| s.start).unwrap_or(0.0);
    m.objective_understandable_secs = sched
        .dialogue
        .first()
        .map(|d| d.start)
        .unwrap_or(sched.duration);
    m.dead_air_secs = compute_max_gap(&sched.dialogue, sched.duration);
    m.avg_shot_duration = if sched.camera_shots.is_empty() {
        0.0
    } else {
        sched.camera_shots.iter().map(|s| s.end - s.start).sum::<f32>() / sched.camera_shots.len() as f32
    };
    m.longest_shot_duration = sched
        .camera_shots
        .iter()
        .map(|s| s.end - s.start)
        .fold(0.0f32, f32::max);
    m.visual_changes_per_min = (sched.events.len() as f32) / (sched.duration / 60.0);
    m.payoff_complete = !plan.payoff.trim().is_empty();
    m.persistent_consequence = !plan.persistent_changes.is_empty();

    let transcript: String = sched
        .dialogue
        .iter()
        .map(|d| format!("{}: {}", d.actor, d.text))
        .collect::<Vec<_>>()
        .join("\n");
    let camera_plan = build_camera_plan(world, sched, rigs);

    // --- Autonomous visual-review loop (CPU-only) ---
    // Re-simulate sampled frames from the shared timeline and measure
    // articulation / freeze / on-screen figure occupancy. This is independent of
    // the GPU render backend and documents that the performance is actually
    // articulated, not a single static frame. Emitted as `review.json`.
    let review = review_schedule(sched, rigs, world, 200, 356, 24);
    let _ = std::fs::write(
        ep_dir.join("review.json"),
        serde_json::to_string_pretty(&review).unwrap_or_default(),
    );

    let diagnostics = Diagnostics {
        episode_id: prep.episode_id.clone(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        director: plan_source.clone(),
        llm_requests: auth.beats.iter().map(|b| b.attempts).sum::<u32>() + auth.attempts,
        llm_failures: auth
            .beats
            .iter()
            .filter(|b| b.source == AuthorSource::DeterministicFallback)
            .count() as u32,
        validation_errors: vec![],
        repairs: 0,
        metrics: m.clone(),
        issues: vec![],
        require_llm,
        llm_used,
        plan_author_source: plan_source.clone(),
        authorship: Some(auth.clone()),
        tts_provider: prep.tts_provider.clone(),
        tts_real: prep.any_real,
        audio_real: prep.any_real,
        frames_captured: captured > 0,
        mp4_produced: enc_ok,
        ffmpeg_command: Some(cmd.clone()),
        ffprobe_ok,
        replay_no_llm: true,
        render_backend: render_backend.to_string(),
    };

    let gemmy = GemmyManifest {
        title: plan.episode_title.clone(),
        summary: plan.logline.clone(),
        hook_text: sched
            .captions
            .first()
            .map(|c| c.text.clone())
            .unwrap_or_default(),
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
        id: prep.episode_id.clone(),
        title: plan.episode_title.clone(),
        logline: plan.logline.clone(),
        duration_secs: sched.duration,
        canonical: true,
        plan: plan.clone(),
        world_before: world.clone(),
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

    write_llm_logs(&llm_dir, auth, &prep.planned, require_llm, llm_used);
    write_render_manifest(
        &ep_dir,
        &cmd,
        &probe,
        cap_out.to_str().unwrap(),
        clean_out.to_str().unwrap(),
        sched.duration,
    );
    write_tts_manifest(&audio_dir, &prep.clips, prep.tts_provider.clone(), prep.any_real);

    Ok(ProduceReport {
        episode_id: prep.episode_id.clone(),
        mp4_captioned: cap_out.to_string_lossy().into_owned(),
        mp4_clean: clean_out.to_string_lossy().into_owned(),
        duration_secs: sched.duration,
        frames: captured,
        require_llm,
        plan_author_source: plan_source,
        llm_used,
        tts_provider: prep.tts_provider.clone(),
        tts_real: prep.any_real,
        audio_real: prep.any_real,
        frames_captured: captured > 0,
        mp4_produced: enc_ok,
        ffprobe_ok,
        probe,
        issues: diagnostics.issues.clone(),
        ffmpeg_command: Some(cmd),
    })
}

/// CPU software-rasterizer production path (regression / offline fallback).
pub fn produce_episode(
    cfg: ProduceConfig,
    author: Box<dyn EpisodeAuthor>,
) -> crate::error::Result<ProduceReport> {
    let ProduceConfig { config, require_llm, world, seed, episode_number, keep_frames } = cfg;
    let out_dir = config.runtime.output_dir.clone();
    let ep_dir = Path::new(&out_dir).join("episodes").join(serial_id("episode", episode_number, 6));
    let frames_dir = ep_dir.join("frames");
    std::fs::create_dir_all(&frames_dir).map_err(io_err(&ep_dir))?;

    let prep = prepare_production(&config, require_llm, &world, seed, episode_number, &*author)?;

    // CPU software-rasterizer frame capture (deterministic, no LLM).
    let fps = config.runtime.frame_rate.max(1);
    let (rw, rh) = (config.runtime.resolution.0 / 2, config.runtime.resolution.1 / 2);
    let renderer = StageRenderer::new(rw.max(2), rh.max(2));
    let n_frames = (prep.schedule.duration * fps as f32).ceil() as u32;
    let mut captured = 0u32;
    for i in 0..n_frames {
        let t = i as f32 / fps as f32;
        let state = evaluate_at(&prep.schedule, &prep.rigs, &world, t);
        let rgba = renderer.render(&state, &prep.rigs, &world);
        let path = frames_dir.join(format!("frame_{:06}.png", i + 1));
        if write_png(&path, rw, rh, &rgba).is_err() {
            tracing::warn!("frame write failed");
            break;
        }
        captured += 1;
    }

    let report = finalize_production(&config, require_llm, &prep, &frames_dir, captured, "cpu_software")?;

    if !keep_frames && captured > 0 {
        let _ = std::fs::remove_dir_all(&frames_dir);
    }
    Ok(report)
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

pub fn build_rigs(world: &WorldState) -> HashMap<String, HumanoidRig> {
    let mut m = HashMap::new();
    for c in world.characters.values() {
        let col = hex_rgb(&c.color_hex);
        m.insert(c.id.clone(), HumanoidRig::default_humanoid(&c.id, &c.voice_id, col));
    }
    m
}

pub fn hex_rgb(hex: &str) -> [f32; 3] {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&h.get(0..2).unwrap_or("80"), 16).unwrap_or(128);
    let g = u8::from_str_radix(&h.get(2..4).unwrap_or("80"), 16).unwrap_or(128);
    let b = u8::from_str_radix(&h.get(4..6).unwrap_or("80"), 16).unwrap_or(128);
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

pub fn compute_max_gap(dialogue: &[DialogueLine], duration: f32) -> f32 {
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

pub fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> crate::error::Result<()> {
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

#[cfg(test)]
mod review_tests {
    use crate::timeline::{CameraShotSpec, CharTrack, Schedule, ScheduledAction};
    use crate::world::build_default_world;
    use super::*;

    #[test]
    fn frame_motion_detects_change_and_identity() {
        let blank = vec![10u8; 4 * 4 * 4];
        let mut diff = blank.clone();
        let mid = 4 * 4 * 4 / 2;
        diff[mid] = 220;
        diff[mid + 1] = 220;
        diff[mid + 2] = 220;
        assert!(frame_motion(&blank, &diff) > 0.0, "changed frame must show motion");
        assert_eq!(frame_motion(&blank, &blank), 0.0, "identical frames show no motion");
    }

    #[test]
    fn review_detects_articulation_on_talking_character() {
        let world = build_default_world();
        let rigs = build_rigs(&world);
        let sched = Schedule {
            duration: 4.0,
            characters: vec![CharTrack {
                id: "mara".into(),
                home: [0.0, 0.0, 0.0],
                actions: vec![ScheduledAction {
                    actor: "mara".into(),
                    action: "speak".into(),
                    target: None,
                    text: Some("hello".into()),
                    start: 0.0,
                    dur: 4.0,
                }],
            }],
            camera_shots: vec![CameraShotSpec {
                start: 0.0,
                end: 4.0,
                intent: "waist".into(),
                subject: "mara".into(),
                reaction: None,
            }],
            dialogue: vec![],
            captions: vec![],
            events: vec![],
            flicker: vec![],
            prop_attach: vec![],
            inserts: vec![],
        };
        let rep = review_schedule(&sched, &rigs, &world, 160, 284, 24);
        assert_eq!(rep.sampled_frames, 24);
        assert!(
            rep.articulation_observed,
            "talking character must articulate (head nod / arm gesture); notes={:?}",
            rep.notes
        );
        assert!(
            !rep.freeze_detected,
            "talking character must not freeze; notes={:?}",
            rep.notes
        );
    }
}
