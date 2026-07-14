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
use crate::avatar::{part_corners, HumanoidRig, PerformanceState};
use crate::config::{Config, TtsConfig};
use crate::package::{
    CameraShot, Caption, Diagnostics, DialogueLine, EpisodeMetrics, EpisodePackage, GemmyManifest,
};
use crate::serial_id;
use crate::story::apply_persistent_changes;
use crate::timeline::*;
use crate::tts::{build_tts, TtsRequest, TtsResult};
use crate::validation::{validate_beat_command, validate_plan, ValidatedPlan};
use crate::world::WorldState;
use crate::{BeatCommand, EpisodePlan};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Object-id for the ground/floor plane. It is kept distinct from the `set`
/// id (1, walls/elevator shell) so that, in the subject-only occlusion pass,
/// the floor a performer *stands on* is never counted as an occluder — only
/// solid structure (walls, props, other characters) can occlude.
const GROUND_ID: u32 = 3;

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

/// Renderer-owned timing known before shared mixing/encoding begins.
#[derive(Debug, Clone)]
pub struct ProductionTimingContext {
    pub started_at: String,
    pub elapsed_before_finalize_secs: f32,
    pub bevy_capture_secs: f32,
    pub effective_fps: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProduceReport {
    pub episode_id: String,
    pub mp4_captioned: String,
    pub mp4_clean: String,
    pub mp4_muted: String,
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

/// A single scene triangle in *world* space, with a flat base color and a
/// stable object id used by the diagnostic / occlusion passes.
///   id == 0  -> background (never emitted)
///   id == 1  -> static set (walls / floor / elevator shell)
///   id == 2  -> prop
///   id >= 100 + k -> character `k` (index into the rig order)
#[derive(Clone)]
struct SceneTri {
    v: [[f32; 3]; 3],
    col: [f32; 3],
    id: u32,
}

/// Per-frame geometry-correctness statistics. These are the objective,
/// structured "visual review" numbers (no image interpretation required):
/// they prove that no triangle exploded into a full-frame spike, that nothing
/// was drawn behind the camera, and that no non-finite projection occurred.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GeometryStats {
    pub total_triangles: u32,
    /// Triangles fully (or nearly) behind the camera.
    pub behind_camera: u32,
    /// Triangles that contained a non-finite vertex.
    pub non_finite: u32,
    /// Triangles actually cut by the near plane (clipped into a valid polygon).
    pub clipped_near: u32,
    /// Character body-part triangles that filled an implausibly large fraction
    /// of the frame (a sure sign of stretched geometry).
    pub implausible_character_triangles: u32,
    /// Largest single-triangle screen-area fraction observed (frame units).
    pub max_triangle_screen_fraction: f32,
    pub notes: Vec<String>,
}

struct StageRenderer {
    w: u32,
    h: u32,
    fov_y: f32,
}

struct Buffers {
    color: Vec<u8>,
    depth: Vec<f32>,
    /// Object id at each pixel (0 = background, 1 = set, 2 = prop, 100+k = char).
    id: Vec<u32>,
    /// Geometry-correctness diagnostics for this frame.
    stats: GeometryStats,
    w: u32,
    h: u32,
}

impl StageRenderer {
    fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            fov_y: 45.0_f32.to_radians(),
        }
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
            id: vec![0u32; n],
            stats: GeometryStats::default(),
            w: self.w,
            h: self.h,
        }
    }

    /// Render one frame to an RGBA buffer (color only). Geometry-correctness
    /// diagnostics are captured by [`StageRenderer::render_buffers`].
    fn render(
        &self,
        state: &FrameState,
        rigs: &HashMap<String, HumanoidRig>,
        world: &WorldState,
    ) -> Vec<u8> {
        self.render_buffers(state, rigs, world, None).color
    }

    /// Render one frame, returning the color + object-id buffers and per-frame
    /// geometry statistics.
    ///
    /// Geometry pipeline (every vertex):
    ///   world -> view space (camera basis) -> homogeneous *near-plane* clip
    ///   -> perspective divide -> viewport map -> triangle setup -> depth test.
    ///
    /// The near-plane clip is the critical correctness fix: a triangle that
    /// crosses the camera near plane is clipped to a valid polygon (3..6 verts)
    /// and fanned into triangles, so geometry never explodes into a full-frame
    /// spike and never silently disappears. Vertices behind the camera or
    /// non-finite are rejected outright.
    fn render_buffers(
        &self,
        state: &FrameState,
        rigs: &HashMap<String, HumanoidRig>,
        world: &WorldState,
        mask: Option<u32>,
    ) -> Buffers {
        let mut buf = self.blank([28, 30, 40], [10, 11, 16]);
        let (eye, look) = (state.camera_eye, state.camera_look);
        let f = normalize(sub(look, eye));
        let up = [0.0f32, 1.0, 0.0];
        let r = normalize(cross(f, up));
        let u = cross(r, f);
        let aspect = self.w as f32 / self.h as f32;
        let fproj = 1.0 / (self.fov_y / 2.0).tan();
        let near = 0.08f32;
        // Side/far frustum planes (view space, vz > 0 in front). A vertex is
        // inside when |vx| <= vz * r_plane and |vy| <= vz * t_plane, which is
        // exactly the inverse of `project_view`'s ndc bounds.
        let r_plane = aspect / fproj;
        let t_plane = 1.0 / fproj;
        let light = normalize([0.4, 1.0, 0.35]);
        let total_area = (self.w * self.h) as f32;

        // ---- Collect world-space triangles with stable object ids ----
        let set = 1u32;
        let prop_id = 2u32;
        let mut tris: Vec<SceneTri> = Vec::new();

        // floor (two large triangles) — distinct id so it never reads as an
        // occluder in the subject-only pass.
        let fy = 0.0f32;
        let fx0 = -12.0;
        let fx1 = 12.0;
        let fz0 = -10.0;
        let fz1 = 8.0;
        tris.push(SceneTri {
            v: [[fx0, fy, fz0], [fx1, fy, fz0], [fx1, fy, fz1]],
            col: [0.16, 0.16, 0.2],
            id: GROUND_ID,
        });
        tris.push(SceneTri {
            v: [[fx0, fy, fz0], [fx1, fy, fz1], [fx0, fy, fz1]],
            col: [0.16, 0.16, 0.2],
            id: GROUND_ID,
        });

        // back wall + side walls
        let wall_col = [0.28, 0.28, 0.34];
        tris.push(SceneTri {
            v: [[-8.0, 0.0, -3.2], [8.0, 0.0, -3.2], [8.0, 4.0, -3.2]],
            col: wall_col,
            id: set,
        });
        tris.push(SceneTri {
            v: [[-8.0, 0.0, -3.2], [8.0, 4.0, -3.2], [-8.0, 4.0, -3.2]],
            col: wall_col,
            id: set,
        });
        for sx in [-8.0f32, 8.0] {
            tris.push(SceneTri {
                v: [[sx, 0.0, -3.2], [sx, 4.0, -3.2], [sx, 4.0, 8.0]],
                col: wall_col,
                id: set,
            });
            tris.push(SceneTri {
                v: [[sx, 0.0, -3.2], [sx, 4.0, 8.0], [sx, 0.0, 8.0]],
                col: wall_col,
                id: set,
            });
        }

        // openable, non-blocking elevator
        push_elevator(&mut tris, world, state.elevator_open);

        // props
        for pf in &state.props {
            push_box(
                &mut tris,
                [pf.pos[0], pf.pos[1], pf.pos[2]],
                [0.22, 0.22, 0.22],
                [0.9, 0.75, 0.3],
                prop_id,
            );
        }

        // characters: each rig part as a box, tagged with a stable char id
        for (k, (cf, pose)) in state.chars.iter().enumerate() {
            if let Some(rig) = rigs.get(&cf.id) {
                let wm = rig.world_matrices(&cf.root, pose);
                let cid = 100 + k as u32;
                for part in &rig.parts {
                    let w = wm
                        .get(&part.joint)
                        .cloned()
                        .unwrap_or(crate::avatar::RigWorld {
                            rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                            pos: cf.root.pos,
                        });
                    let corners = part_corners(part, &w);
                    push_box_corners(&mut tris, &corners, part.color, cid);
                }
            }
        }

        // flicker light tint: darken everything slightly if flicker active
        let amb = if state.flicker {
            let ph = (state.camera_eye[0] * 13.0).fract();
            0.25 + 0.2 * (ph * 6.28).sin().abs()
        } else {
            1.0
        };

        // ---- Project + clip + rasterize ----
        let mut stats = GeometryStats::default();
        for tri in &tris {
            let id = match mask {
                // Subject-only pass: draw *only* the subject's triangles so we can
                // measure its true (un-occluded) silhouette. Everything else is
                // skipped — this is what makes the occlusion metric honest (it is
                // `1 - visible / silhouette`, not a count of all other geometry).
                Some(subj) => {
                    if tri.id == subj {
                        tri.id
                    } else {
                        continue;
                    }
                }
                None => tri.id,
            };
            let va = view_of(tri.v[0], eye, r, u, f);
            let vb = view_of(tri.v[1], eye, r, u, f);
            let vc = view_of(tri.v[2], eye, r, u, f);
            if !finite3(va) || !finite3(vb) || !finite3(vc) {
                stats.non_finite += 1;
                continue;
            }
            let poly = clip_near(&[va, vb, vc], near);
            if poly.is_empty() {
                stats.behind_camera += 1;
                continue;
            }
            // Bound the polygon to the visible frustum. Without the side planes a
            // triangle straddling the camera (e.g. an off-screen character behind
            // the lens) would be clipped only at the near plane, leaving a vertex
            // with an arbitrarily large projected coordinate that paints a giant
            // shard across the frame. Clipping the four side planes keeps every
            // drawn triangle inside the viewport.
            let poly = clip_plane(&poly, [-1.0, 0.0, r_plane]); // right: vx <= vz*r
            let poly = clip_plane(&poly, [1.0, 0.0, r_plane]); // left:  vx >= -vz*r
            let poly = clip_plane(&poly, [0.0, -1.0, t_plane]); // top:    vy <= vz*t
            let poly = clip_plane(&poly, [0.0, 1.0, t_plane]); // bottom: vy >= -vz*t
            if poly.len() < 3 {
                continue;
            }
            if poly.len() > 3 {
                stats.clipped_near += 1;
            }
            stats.total_triangles += 1;
            // project each clipped view vertex to the screen
            let pts: Vec<(f32, f32, f32)> = poly
                .iter()
                .map(|p| project_view(*p, fproj, aspect, self.w as f32, self.h as f32))
                .collect();
            for k in 1..pts.len() - 1 {
                let (a, b, c) = (pts[0], pts[k], pts[k + 1]);
                let a3 = [a.0, a.1, a.2];
                let b3 = [b.0, b.1, b.2];
                let c3 = [c.0, c.1, c.2];
                let nrm = normalize(cross(sub(b3, a3), sub(c3, a3)));
                let shade = (0.4 + 0.6 * dot(nrm, light).max(0.0)) * amb;
                let base = [
                    (tri.col[0] * shade).clamp(0.0, 1.0),
                    (tri.col[1] * shade).clamp(0.0, 1.0),
                    (tri.col[2] * shade).clamp(0.0, 1.0),
                ];
                let frac = tri_area_2d(a, b, c) / total_area;
                if frac > stats.max_triangle_screen_fraction {
                    stats.max_triangle_screen_fraction = frac;
                }
                // A character body part must never fill most of the frame.
                if id >= 100 && frac > 0.6 {
                    stats.implausible_character_triangles += 1;
                }
                draw_triangle(&mut buf, a, b, c, base, id);
            }
        }

        buf.stats = stats;
        buf
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
    // Camera safety now pulls portrait shots farther from the performer; the
    // same joint travel consequently affects fewer pixels. Keep this strictly
    // above the freeze gate while avoiding a framing-dependent false negative.
    let articulation = max_motion > 0.004;
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
        [71.0, 71.0, 87.0],    // walls
        [41.0, 41.0, 51.0],    // floor
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

/// Per-shot objective framing / occlusion analysis. These are the
/// programmatic "camera legibility" numbers: no image interpretation, just
/// structured counts over the rendered object-id + depth buffers.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ShotAnalysis {
    pub start: f32,
    pub end: f32,
    pub intent: String,
    pub subject: String,
    pub in_frame: bool,
    /// Subject height as a fraction of frame height (0..1).
    pub subject_height_fraction: f32,
    /// Subject pixels as a fraction of the whole frame (0..1).
    pub subject_area_fraction: f32,
    /// 0 = fully visible, 1 = fully hidden / off-frame.
    pub occlusion: f32,
    /// Static-set pixels as a fraction of the frame (0..1).
    pub set_fraction: f32,
    pub rejected: bool,
    pub reject_reason: String,
    pub notes: Vec<String>,
}

/// Analyze a single frame for the given subject character id. The subject is
/// tagged `id = 100 + index_in_state.chars` by the renderer, so we locate it by
/// name. We render the full frame for size/position and a subject-only frame to
/// measure the performer's *true* (un-occluded) silhouette. Occlusion is then
/// `1 - visible / silhouette` — honest, because it only considers how much of
/// the performer's own area is hidden by foreground geometry.
pub fn analyze_frame(
    state: &FrameState,
    rigs: &HashMap<String, HumanoidRig>,
    world: &WorldState,
    w: u32,
    h: u32,
    subject_char_id: &str,
) -> ShotAnalysis {
    let total = (w * h) as f32;
    let subject_id = state
        .chars
        .iter()
        .position(|(cf, _)| cf.id == subject_char_id)
        .map(|k| 100 + k as u32);

    let full = StageRenderer::new(w, h).render_buffers(state, rigs, world, None);
    let mut full_subj = 0u32;
    let mut full_set = 0u32;
    let mut minx = u32::MAX;
    let mut maxx = 0u32;
    let mut miny = u32::MAX;
    let mut maxy = 0u32;
    for (i, &id) in full.id.iter().enumerate() {
        if id == 1 {
            full_set += 1;
        }
        if Some(id) == subject_id {
            full_subj += 1;
            let x = (i % w as usize) as u32;
            let y = (i / w as usize) as u32;
            if x < minx {
                minx = x;
            }
            if x > maxx {
                maxx = x;
            }
            if y < miny {
                miny = y;
            }
            if y > maxy {
                maxy = y;
            }
        }
    }

    // Subject-only pass: only the performer is drawn, giving its full silhouette.
    let silhouette = if let Some(sid) = subject_id {
        StageRenderer::new(w, h)
            .render_buffers(state, rigs, world, Some(sid))
            .id
            .iter()
            .filter(|&&id| id == sid)
            .count() as u32
    } else {
        0
    };
    let visible = full_subj;

    let occlusion = if silhouette > 0 {
        1.0 - visible as f32 / silhouette as f32
    } else {
        1.0
    };
    let in_frame = silhouette > 0 && full_subj > 0;
    let height_frac = if full_subj > 0 {
        (maxy - miny) as f32 / h as f32
    } else {
        0.0
    };
    let area_frac = full_subj as f32 / total;
    let set_frac = full_set as f32 / total;
    let (rejected, reason) = evaluate_shot_legibility(in_frame, height_frac, occlusion, set_frac);

    ShotAnalysis {
        start: 0.0,
        end: 0.0,
        intent: String::new(),
        subject: subject_char_id.to_string(),
        in_frame,
        subject_height_fraction: height_frac,
        subject_area_fraction: area_frac,
        occlusion,
        set_fraction: set_frac,
        rejected,
        reject_reason: reason,
        notes: Vec::new(),
    }
}

/// Hard camera-legibility rules (Phase 6). Returns (rejected, reason).
fn evaluate_shot_legibility(
    in_frame: bool,
    height_frac: f32,
    occlusion: f32,
    set_frac: f32,
) -> (bool, String) {
    if !in_frame {
        return (true, "subject off-frame / not visible".into());
    }
    if height_frac < 0.20 {
        return (true, format!("subject too small (h={height_frac:.2})"));
    }
    if occlusion > 0.30 {
        return (true, format!("subject occluded {:.0}%", occlusion * 100.0));
    }
    if set_frac > 0.65 {
        return (
            true,
            format!("blank/set dominates {:.0}%", set_frac * 100.0),
        );
    }
    (false, String::new())
}

/// Analyze every planned camera shot at its midpoint. Used to *prove* the
/// coverage is legible and to record truthful framing diagnostics.
pub fn analyze_schedule(
    sched: &Schedule,
    rigs: &HashMap<String, HumanoidRig>,
    world: &WorldState,
    w: u32,
    h: u32,
) -> Vec<ShotAnalysis> {
    let mut out = Vec::new();
    for shot in &sched.camera_shots {
        let t = (shot.start + shot.end) / 2.0;
        let state = evaluate_at(sched, rigs, world, t);
        let analyzed_subject = if shot.intent == "reaction" {
            shot.reaction.as_deref().unwrap_or(&shot.subject)
        } else {
            &shot.subject
        };
        let mut a = analyze_frame(&state, rigs, world, w, h, analyzed_subject);
        a.start = shot.start;
        a.end = shot.end;
        a.intent = shot.intent.clone();
        out.push(a);
    }
    out
}

fn push_box(tris: &mut Vec<SceneTri>, center: [f32; 3], half: [f32; 3], col: [f32; 3], id: u32) {
    let c = [
        [
            center[0] - half[0],
            center[1] - half[1],
            center[2] - half[2],
        ],
        [
            center[0] + half[0],
            center[1] - half[1],
            center[2] - half[2],
        ],
        [
            center[0] + half[0],
            center[1] + half[1],
            center[2] - half[2],
        ],
        [
            center[0] - half[0],
            center[1] + half[1],
            center[2] - half[2],
        ],
        [
            center[0] - half[0],
            center[1] - half[1],
            center[2] + half[2],
        ],
        [
            center[0] + half[0],
            center[1] - half[1],
            center[2] + half[2],
        ],
        [
            center[0] + half[0],
            center[1] + half[1],
            center[2] + half[2],
        ],
        [
            center[0] - half[0],
            center[1] + half[1],
            center[2] + half[2],
        ],
    ];
    push_box_corners(tris, &c, col, id);
}

fn push_box_corners(tris: &mut Vec<SceneTri>, c: &[[f32; 3]; 8], col: [f32; 3], id: u32) {
    let faces = [
        (0, 1, 2, 3),
        (4, 5, 6, 7),
        (0, 1, 5, 4),
        (2, 3, 7, 6),
        (1, 2, 6, 5),
        (0, 3, 7, 4),
    ];
    for (i0, i1, i2, i3) in faces {
        tris.push(SceneTri {
            v: [c[i0], c[i1], c[i2]],
            col,
            id,
        });
        tris.push(SceneTri {
            v: [c[i0], c[i2], c[i3]],
            col,
            id,
        });
    }
}

/// Push the openable elevator as a set of readable components that live *behind*
/// the performers (deeper than the staging marks), so the shell can never
/// enclose an actor or block the camera. `open` in [0,1] slides the door panels
/// apart to reveal the interior.
fn push_elevator(tris: &mut Vec<SceneTri>, world: &WorldState, open: f32) {
    let _ = world;
    let cx = crate::stage::ELEVATOR_CENTER[0];
    let front_z = crate::stage::ELEVATOR_DOORS[2];
    let back_z = crate::stage::IMPOSSIBLE_FLOOR[2];
    let half_w = 0.9;
    let height = 2.6;
    let midz = (front_z + back_z) / 2.0;
    let depth_h = (front_z - back_z) / 2.0;
    let shell = [0.4, 0.42, 0.46];
    let door_col = [0.52, 0.54, 0.58];
    let interior = [0.22, 0.23, 0.27];

    // interior back wall
    push_box(
        tris,
        [cx, height / 2.0, back_z - 0.04],
        [half_w, height / 2.0, 0.04],
        interior,
        1,
    );
    // interior side walls
    push_box(
        tris,
        [cx - half_w, height / 2.0, midz],
        [0.05, height / 2.0, depth_h],
        interior,
        1,
    );
    push_box(
        tris,
        [cx + half_w, height / 2.0, midz],
        [0.05, height / 2.0, depth_h],
        interior,
        1,
    );
    // ceiling + floor
    push_box(
        tris,
        [cx, height + 0.03, midz],
        [half_w, 0.03, depth_h],
        shell,
        1,
    );
    push_box(
        tris,
        [cx, 0.02, midz],
        [half_w, 0.02, depth_h],
        [0.12, 0.12, 0.15],
        1,
    );
    // exterior side frames (visible in the hallway)
    push_box(
        tris,
        [cx - half_w - 0.06, height / 2.0, front_z + 0.1],
        [0.06, height / 2.0, 0.18],
        shell,
        1,
    );
    push_box(
        tris,
        [cx + half_w + 0.06, height / 2.0, front_z + 0.1],
        [0.06, height / 2.0, 0.18],
        shell,
        1,
    );
    // control panel + floor indicator (semantic, on the right jamb)
    push_box(
        tris,
        [cx + half_w + 0.10, 1.15, front_z + 0.06],
        [0.05, 0.18, 0.04],
        [0.3, 0.32, 0.36],
        1,
    );
    push_box(
        tris,
        [cx, height - 0.18, front_z + 0.02],
        [0.16, 0.06, 0.03],
        [0.9, 0.8, 0.2],
        1,
    );

    // sliding door panels (slide apart as `open` -> 1), revealing the interior
    let slide = 0.9 * open.clamp(0.0, 1.0);
    let door_w = 0.45;
    let left_cx = (cx - half_w / 2.0) - slide;
    let right_cx = (cx + half_w / 2.0) + slide;
    // When fully open the panels tuck beside the jambs; while closed they cover
    // the doorway. Draw them at the front plane regardless (thin panels).
    push_box(
        tris,
        [left_cx, height / 2.0, front_z],
        [door_w, height / 2.0, 0.04],
        door_col,
        2,
    );
    push_box(
        tris,
        [right_cx, height / 2.0, front_z],
        [door_w, height / 2.0, 0.04],
        door_col,
        2,
    );
}

fn draw_triangle(
    buf: &mut Buffers,
    a: (f32, f32, f32),
    b: (f32, f32, f32),
    c: (f32, f32, f32),
    col: [f32; 3],
    id: u32,
) {
    let (ax, ay, az) = a;
    let (bx, by, bz) = b;
    let (cx, cy, cz) = c;
    let minx = (ax.min(bx).min(cx).floor() as i32)
        .max(0)
        .min(buf.w as i32 - 1);
    let maxx = (ax.max(bx).max(cx).ceil() as i32)
        .max(0)
        .min(buf.w as i32 - 1);
    let miny = (ay.min(by).min(cy).floor() as i32)
        .max(0)
        .min(buf.h as i32 - 1);
    let maxy = (ay.max(by).max(cy).ceil() as i32)
        .max(0)
        .min(buf.h as i32 - 1);
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
                    buf.id[idx] = id;
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

// ---- geometry helpers (view transform, near clipping, projection) ----

/// Transform a world point into camera/view space given the orthonormal basis
/// (r = right, u = up, f = forward). Returns [vx, vy, vz] where vz is distance
/// along the view direction (positive = in front of the camera).
fn view_of(p: [f32; 3], eye: [f32; 3], r: [f32; 3], u: [f32; 3], f: [f32; 3]) -> [f32; 3] {
    let d = sub(p, eye);
    [dot(d, r), dot(d, u), dot(d, f)]
}

/// Clip a (convex) polygon against the near plane `z >= near` using
/// Sutherland–Hodgman. Returns the clipped polygon (3..6 vertices) in view
/// space, or an empty vector if the whole polygon is behind the camera.
fn clip_near(poly: &[[f32; 3]], near: f32) -> Vec<[f32; 3]> {
    if poly.len() < 3 {
        return Vec::new();
    }
    let mut out: Vec<[f32; 3]> = Vec::new();
    let n = poly.len();
    for i in 0..n {
        let cur = poly[i];
        let nxt = poly[(i + 1) % n];
        let cur_in = cur[2] >= near;
        let nxt_in = nxt[2] >= near;
        if cur_in {
            out.push(cur);
        }
        if cur_in != nxt_in {
            let denom = nxt[2] - cur[2];
            if denom.abs() > 1e-9 {
                let t = (near - cur[2]) / denom;
                out.push([
                    cur[0] + (nxt[0] - cur[0]) * t,
                    cur[1] + (nxt[1] - cur[1]) * t,
                    near,
                ]);
            }
        }
    }
    out
}

/// Clip a view-space polygon against a half-space `n . p >= 0` (Sutherland–
/// Hodgman). Used for the four side/far frustum planes so that a triangle which
/// straddles the camera (e.g. an off-screen character *behind* the camera) is
/// bounded to the screen instead of exploding to an infinitely large polygon
/// that would otherwise corrupt the whole frame as a shard.
fn clip_plane(poly: &[[f32; 3]], n: [f32; 3]) -> Vec<[f32; 3]> {
    if poly.len() < 3 {
        return Vec::new();
    }
    let mut out: Vec<[f32; 3]> = Vec::new();
    let n = normalize(n);
    let side = |p: [f32; 3]| dot(n, p);
    let m = poly.len();
    for i in 0..m {
        let cur = poly[i];
        let nxt = poly[(i + 1) % m];
        let cur_in = side(cur) >= -1e-6;
        let nxt_in = side(nxt) >= -1e-6;
        if cur_in {
            out.push(cur);
        }
        if cur_in != nxt_in {
            let dcur = side(cur);
            let dnxt = side(nxt);
            let denom = dnxt - dcur;
            if denom.abs() > 1e-9 {
                let t = -dcur / denom;
                out.push([
                    cur[0] + (nxt[0] - cur[0]) * t,
                    cur[1] + (nxt[1] - cur[1]) * t,
                    cur[2] + (nxt[2] - cur[2]) * t,
                ]);
            }
        }
    }
    out
}

/// True if all three components are finite (not NaN, not infinite).
fn finite3(v: [f32; 3]) -> bool {
    v[0].is_finite() && v[1].is_finite() && v[2].is_finite()
}

/// Perspective-project a view-space point (already clipped to vz >= near) to
/// screen coordinates + camera-space depth.
fn project_view(p: [f32; 3], fproj: f32, aspect: f32, w: f32, h: f32) -> (f32, f32, f32) {
    let vz = p[2].max(1e-4);
    let ndc_x = (fproj / aspect) * p[0] / vz;
    let ndc_y = fproj * p[1] / vz;
    let sx = (ndc_x * 0.5 + 0.5) * w;
    let sy = (1.0 - (ndc_y * 0.5 + 0.5)) * h;
    (sx, sy, vz)
}

/// Absolute 2D area of a screen-space triangle (ignores depth).
fn tri_area_2d(a: (f32, f32, f32), b: (f32, f32, f32), c: (f32, f32, f32)) -> f32 {
    ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs() * 0.5
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
    out_muted: &str,
    captions: &[Caption],
    resolution: (u32, u32),
    fps: u32,
) -> std::result::Result<(String, bool), crate::error::CoreError> {
    // ffmpeg's image2 glob and subtitle/font filters do not cope with the mixed
    // `\`/`/` separators that Rust's Windows `Path::join` produces, so normalise
    // every path argument to forward slashes. This is the fix for the
    // "Could find no file or sequence" failure on the frame pattern.
    let frames_dir = frames_dir.replace('\\', "/");
    let audio_path = audio_path.replace('\\', "/");
    let out_captioned = out_captioned.replace('\\', "/");
    let out_clean = out_clean.replace('\\', "/");
    let out_muted = out_muted.replace('\\', "/");
    let scale = format!("scale={}:{}", resolution.0, resolution.1);
    // Prefer robust ASS subtitles (correct wrapping, outlines, safe margins,
    // line breaks, and *no* fragile drawtext string concatenation that can
    // fuse words or expose escape syntax). Falls back to drawtext only if the
    // ASS file cannot be written.
    let captions_filter = if captions.is_empty() {
        String::new()
    } else if let Some(ass_rel) =
        stage_ass_for_ffmpeg(&frames_dir, &build_ass_subtitles(captions, resolution))
    {
        format!(",subtitles='{ass_rel}'")
    } else {
        let font = resolve_font(&cfg.runtime.font_path);
        let font_ref = stage_font_for_ffmpeg(&frames_dir, &font);
        build_caption_filter(captions, &font_ref, resolution)
    };
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
        audio_path.clone(),
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
        out_captioned.clone(),
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
        audio_path.clone(),
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
        out_clean.clone(),
    ];
    let status_clean = run_ffmpeg(ff, &args_clean)?;
    // Muted review export is encoded directly from the deterministic frames so
    // it never depends on audio stream manipulation or player defaults.
    let args_muted: Vec<String> = vec![
        "-y".into(),
        "-framerate".into(),
        fps.to_string(),
        "-i".into(),
        frame_pattern.clone(),
        "-vf".into(),
        vf_clean,
        "-c:v".into(),
        "libx264".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-an".into(),
        "-movflags".into(),
        "+faststart".into(),
        out_muted,
    ];
    let status_muted = run_ffmpeg(ff, &args_muted)?;
    let ok = status_cap && status_clean && status_muted;
    let cmd = format!("{ff} -y -framerate {fps} -i {frame_pattern} -i {audio_path} -vf \"{vf_captioned}\" -c:v libx264 -c:a aac -shortest {out_captioned}");
    Ok((cmd, ok))
}

fn run_ffmpeg(ff: &str, args: &[String]) -> std::result::Result<bool, crate::error::CoreError> {
    let out = std::process::Command::new(ff).args(args).output();
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

pub fn build_caption_filter(
    captions: &[Caption],
    font_ref: &str,
    resolution: (u32, u32),
) -> String {
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
    // Phone-readable: tighter wrap for 1080 vertical with 8% horizontal margins.
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in words {
        if cur.len() + w.len() + 1 > 20 && !cur.is_empty() {
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

/// Greedy word-wrap into lines of at most `max_chars` characters each, used by
/// both ASS generation and bounds validation. The caller decides whether more
/// than two lines is acceptable; retaining every line prevents hidden overflow.
pub fn wrap_caption_lines(text: &str, max_chars: usize) -> Vec<String> {
    let cap = max_chars.max(1);
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in words {
        if cur.len() + w.len() + 1 > cap && !cur.is_empty() {
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
    lines
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

/// Canonical caption normalization.
///
/// Collapses *all* whitespace (real newlines, tabs, runs of spaces) into a
/// single space and trims. This is the direct fix for the "fused words"
/// corruption: a stray newline no longer eats the space between two words, and
/// apostrophes / punctuation are preserved verbatim. ASS then wraps the line.
pub fn normalize_caption_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_ws = false;
    for c in raw.chars() {
        if c.is_whitespace() {
            if !last_ws && !out.is_empty() {
                out.push(' ');
            }
            last_ws = true;
        } else {
            out.push(c);
            last_ws = false;
        }
    }
    out.trim().to_string()
}

/// Escape a caption string for the ASS `Text` field. ASS line breaks are `\N`;
/// backslashes and commas are reserved and must be escaped.
fn escape_ass(t: &str) -> String {
    let normalized = normalize_caption_text(t);
    let mut out = String::with_capacity(normalized.len() + 4);
    for c in normalized.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\N"),
            _ => out.push(c),
        }
    }
    out
}

/// ASS `H:MM:SS.cc` timecode.
fn ass_timecode(sec: f32) -> String {
    let s = sec.max(0.0);
    let h = (s / 3600.0) as u32;
    let m = ((s % 3600.0) / 60.0) as u32;
    let rem = s - (h as f32 * 3600.0 + m as f32 * 60.0);
    format!("{h}:{m:02}:{rem:05.2}")
}

/// Build a complete ASS subtitle file for the episode captions. ASS handles
/// font size, outline, safe-margin placement, wrapping and timing natively, so
/// the rendered captions are robust regardless of punctuation or line breaks.
///
/// Placement uses Alignment 8 (lower-middle, center) so captions sit above the
/// bottom-edge unsafe zone and away from typical face/interaction framing.
pub fn build_ass_subtitles(captions: &[Caption], resolution: (u32, u32)) -> String {
    let (w, h) = resolution;
    let fontsize = ((h as f32 * 0.027).round() as u32).clamp(42, 54);
    // Generous horizontal safe margins for 9:16 phone readability.
    let margin_h = ((w as f32 * 0.08).round() as u32).clamp(80, 140);
    let safe_w = w.saturating_sub(2 * margin_h) as f32;
    let wrap_chars = (safe_w / (fontsize as f32 * 0.55)).floor().max(1.0) as usize;
    // Vertical margin from the bottom: keep captions in the lower-middle band.
    let margin_v = ((h as f32 * 0.22).round() as u32).clamp(320, 520);
    let mut s = String::new();
    s.push_str("[Script Info]\n");
    s.push_str("ScriptType: v4.00+\n");
    s.push_str(&format!("PlayResX: {w}\n"));
    s.push_str(&format!("PlayResY: {h}\n"));
    s.push_str("WrapStyle: 2\n");
    s.push_str("ScaledBorderAndShadow: yes\n\n");
    s.push_str("[V4+ Styles]\n");
    s.push_str("Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n");
    // Alignment 8 = bottom-center but raised by MarginV into the lower-middle
    // safe region. White fill, black outline + soft shadow for contrast.
    s.push_str(&format!(
        "Style: Default,Arial,{fontsize},&H00FFFFFF,&H00000000,&H00000000,&H7F000000,-1,0,0,0,100,100,0,0,1,{outline},{shadow},2,{ml},{mr},{mv},1\n\n",
        outline = 5,
        shadow = 1,
        ml = margin_h,
        mr = margin_h,
        mv = margin_v,
    ));
    s.push_str("[Events]\n");
    s.push_str("Format: Layer, Start, End, Style, Text\n");
    for c in captions {
        let normalized = normalize_caption_text(&c.text);
        // Explicit wrapping makes the encoded layout match validation instead
        // of relying on libass to choose different break points.
        let text = wrap_caption_lines(&normalized, wrap_chars)
            .into_iter()
            .map(|line| escape_ass(&line))
            .collect::<Vec<_>>()
            .join("\\N");
        s.push_str(&format!(
            "Dialogue: 0,{},{},Default,{}\n",
            ass_timecode(c.start),
            ass_timecode(c.end),
            text
        ));
    }
    s
}

/// Stage the ASS subtitle file at a *relative* (no drive-colon) path so ffmpeg's
/// `subtitles` filter can parse it on Windows. Returns the relative path, or
/// `None` if it could not be written.
fn stage_ass_for_ffmpeg(frames_dir: &str, ass_content: &str) -> Option<String> {
    let dest = std::path::Path::new(frames_dir)
        .join("..")
        .join("subtitles")
        .join("captions.ass");
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&dest, ass_content).is_err() {
        return None;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let rel = if let Ok(stripped) = dest.strip_prefix(&cwd) {
        stripped.to_string_lossy().into_owned()
    } else {
        dest.to_string_lossy().into_owned()
    };
    Some(rel.replace('\\', "/"))
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
    let path = path.replace('\\', "/");
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
            &path,
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
    let peak = mixed
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, f32::max)
        .max(1e-6);
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
        let size =
            u32::from_le_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
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

/// Minimum continuous-silence length (seconds) that counts as "dead air" and
/// must be explained or compressed (Phase 8).
pub fn cfg_silence_min_gap() -> f32 {
    2.5
}

/// A contiguous silent span in the mixed track.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SilenceRange {
    pub start: f32,
    pub end: f32,
    pub duration: f32,
    /// Whether this gap was repaired (made intentional) or remains as dead air.
    pub repaired: bool,
}

/// Objective silence / dead-air report derived from the *actual* mixed WAV
/// (RMS energy per short window). This is the programmatic replacement for
/// "listening" — it proves where, how long, and how much dead air exists.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SilenceReport {
    pub first_content_secs: f32,
    pub max_gap_secs: f32,
    pub ranges: Vec<SilenceRange>,
}

/// Analyze a mono/stereo WAV for silent windows. `min_gap` is the shortest
/// silent span (seconds) worth reporting; `rms_thresh` is the energy floor
/// below which a window is considered silent.
pub fn detect_silence(path: &str, min_gap: f32, rms_thresh: f32) -> SilenceReport {
    let Some(samples) = read_wav_mono_f32(path) else {
        return SilenceReport {
            first_content_secs: 0.0,
            max_gap_secs: 0.0,
            ranges: vec![],
        };
    };
    let sr_est = 44100u32; // only used for windowing; exact rate not required
    let win = ((0.1f32 * sr_est as f32).round() as usize).max(1);
    let n = samples.len();
    let mut silent: Vec<bool> = Vec::with_capacity(n / win + 1);
    let mut first_content = n as f32; // index of first non-silent sample
    let mut last_content = 0usize;
    let mut i = 0;
    while i < n {
        let end = (i + win).min(n);
        let mut sum = 0.0f32;
        for s in &samples[i..end] {
            sum += s * s;
        }
        let rms = (sum / (end - i) as f32).sqrt();
        let is_silent = rms < rms_thresh;
        silent.push(is_silent);
        if !is_silent {
            if i < first_content as usize {
                first_content = i as f32;
            }
            last_content = end;
        }
        i = end;
    }
    let step = win as f32 / sr_est as f32;
    // Build silent runs (in seconds).
    let mut ranges: Vec<SilenceRange> = Vec::new();
    let mut run_start: Option<usize> = None;
    for (k, &s) in silent.iter().enumerate() {
        if s && run_start.is_none() {
            run_start = Some(k);
        } else if !s && run_start.is_some() {
            let a = run_start.unwrap() as f32 * step;
            let b = k as f32 * step;
            if b - a >= min_gap {
                ranges.push(SilenceRange {
                    start: a,
                    end: b,
                    duration: b - a,
                    repaired: false,
                });
            }
            run_start = None;
        }
    }
    if let Some(k) = run_start {
        let a = k as f32 * step;
        let b = silent.len() as f32 * step;
        if b - a >= min_gap {
            ranges.push(SilenceRange {
                start: a,
                end: b,
                duration: b - a,
                repaired: false,
            });
        }
    }

    // The lead-in (0 .. first content) and tail (last content .. duration) are
    // also dead air worth reporting.
    let first_content_secs = first_content / sr_est as f32;
    let duration = n as f32 / sr_est as f32;
    if first_content_secs >= min_gap {
        ranges.insert(
            0,
            SilenceRange {
                start: 0.0,
                end: first_content_secs,
                duration: first_content_secs,
                repaired: false,
            },
        );
    }
    let tail = duration - last_content as f32 / sr_est as f32;
    if tail >= min_gap {
        ranges.push(SilenceRange {
            start: last_content as f32 / sr_est as f32,
            end: duration,
            duration: tail,
            repaired: false,
        });
    }

    let max_gap = ranges.iter().map(|r| r.duration).fold(0.0f32, f32::max);
    SilenceReport {
        first_content_secs,
        max_gap_secs: max_gap,
        ranges,
    }
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

/// Trim leading and trailing silence from a mono WAV file in place, so the
/// cached clip's length matches the actual spoken duration. This is what keeps
/// the mixed audio free of dead air: `mix_audio` plays the *whole* clip file
/// starting at its scheduled time, so any espeak trailing/leading padding would
/// otherwise widen the gaps between lines. Returns the trimmed duration in
/// seconds (falls back to the original length if the file cannot be read).
fn trim_wav_silence_in_place(path: &str, _mix_sr: u32, rms_thresh: f32) -> f32 {
    let Some(sr) = crate::tts::wav_sample_rate(path) else {
        return 0.0;
    };
    let Some(samples) = read_wav_mono_f32(path) else {
        return 0.0;
    };
    let n = samples.len();
    if n == 0 {
        return 0.0;
    }
    let orig = n as f32 / sr.max(1) as f32;
    // Find the first/last sample whose magnitude clears the threshold.
    let first = (0..n).find(|&i| samples[i].abs() > rms_thresh);
    let last = (0..n).rev().find(|&i| samples[i].abs() > rms_thresh);
    let (Some(a), Some(b)) = (first, last) else {
        // Wholly silent clip: leave as-is.
        return orig;
    };
    if a == 0 && b == n - 1 {
        return orig; // already tight
    }
    let trimmed: Vec<i16> = samples[a..=b]
        .iter()
        .map(|s| {
            (s.clamp(-1.0, 1.0) * 32767.0)
                .round()
                .clamp(-32768.0, 32767.0) as i16
        })
        .collect();
    write_wav(path, sr, &trimmed);
    ((b - a + 1) as f32) / sr.max(1) as f32
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
    /// Phase-level wall-clock durations measured during `prepare_production`.
    pub prep_timings: PrepTimings,
    pub word_alignments: HashMap<(String, String), crate::asr::WordAlignment>,
    pub motion_plan: crate::motion::ProductionMotionPlan,
    pub runtime_telemetry: backlot_runtime::RuntimeTelemetry,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrepTimings {
    pub llm_authoring_secs: f32,
    pub tts_generation_secs: f32,
    pub speech_alignment_secs: f32,
    pub kimodo_generation_secs: f32,
    pub motion_processing_secs: f32,
    pub timeline_prep_secs: f32,
}

/// Stage 1 (shared): author, validate, synthesize TTS, build the authoritative
/// schedule, build rigs, collect audio clips. No frames are rendered here.
fn duration_acceptance_window(target_secs: f32) -> (f32, f32) {
    let target = target_secs.clamp(45.0, 60.0);
    let min = 45.0f32.min(target);
    let max = 60.0f32.max(target);
    (min, max)
}

/// Measure the real, dead-air-compacted runtime of an authored plan using the
/// configured TTS engine — the exact same path `prepare_production` uses for its
/// authoritative gate. This is exposed so the LLM author's duration-repair loop
/// can estimate against *measured* speech timing instead of a rough heuristic
/// that ignores dead-air compaction (which otherwise overestimates runtime and
/// lets a too-short episode slip past the repair loop).
pub fn measure_runtime(
    world: &WorldState,
    plan: &EpisodePlan,
    commands: &HashMap<String, BeatCommand>,
    tts_cfg: &TtsConfig,
    max_dead_air: f32,
) -> crate::error::Result<f32> {
    let planned = PlannedEpisode {
        plan: plan.clone(),
        commands: commands.clone(),
    };
    let validated =
        build_validated(world, &planned).ok_or_else(|| crate::error::CoreError::EmptyPlan)?;
    let tts = build_tts(tts_cfg);
    let speech: Vec<_> = validated
        .resolved_beats
        .iter()
        .flat_map(|b| b.resolved_actions.iter())
        .filter(|ra| matches!(action_kind(&ra.action), ActionKind::Speak))
        .collect();
    let requests: Vec<TtsRequest> = speech
        .iter()
        .map(|ra| TtsRequest {
            text: ra.text.clone().unwrap_or_default(),
            voice_id: world
                .character(&ra.actor_id)
                .map(|c| c.voice_id.clone())
                .unwrap_or_else(|| ra.actor_id.clone()),
            delivery: ra.delivery.clone(),
        })
        .collect();
    let results = tts.synthesize_batch(&requests);
    let mut tts_durations: HashMap<(String, String), f32> = HashMap::new();
    for (ra, res) in speech.iter().zip(results) {
        let text = ra.text.clone().unwrap_or_default();
        let dur = if let Some(p) = &res.audio_path {
            let t = trim_wav_silence_in_place(p, tts_cfg.sample_rate, 0.01);
            if t > 0.0 {
                t
            } else {
                res.duration
            }
        } else {
            res.duration
        };
        tts_durations.insert((ra.actor_id.clone(), text), dur);
    }
    let mut sched = build_schedule(world, &validated, &tts_durations);
    compact_dead_air(&mut sched, max_dead_air);
    Ok(sched.duration)
}

pub fn prepare_production(
    config: &Config,
    require_llm: bool,
    world: &WorldState,
    seed: u64,
    episode_number: u64,
    author: &dyn EpisodeAuthor,
) -> crate::error::Result<PreparedProduction> {
    backlot_runtime::clear_global_telemetry();
    let episode_id = serial_id("episode", episode_number, 6);
    let ctx = crate::director::DirectorContext {
        world: world.clone(),
        episode_number,
        seed,
        target_duration: config.runtime.target_duration_secs,
        recent_summaries: vec![],
        tone: vec!["surreal".into(), "comedy".into()],
    };
    let t_llm = std::time::Instant::now();
    let (planned, auth) = author.author(&ctx)?;
    let llm_authoring_secs = t_llm.elapsed().as_secs_f32();
    let t_tts_start = std::time::Instant::now();
    let validated =
        build_validated(world, &planned).ok_or_else(|| crate::error::CoreError::EmptyPlan)?;

    let tts = build_tts(&config.tts);
    let provider = tts.provider_name().to_string();
    let mut tts_durations: HashMap<(String, String), f32> = HashMap::new();
    let mut tts_results: HashMap<(String, String), TtsResult> = HashMap::new();
    let mut clips: Vec<(String, f32, f32)> = Vec::new();
    let mut any_real = false;
    let speech: Vec<_> = validated
        .resolved_beats
        .iter()
        .flat_map(|b| b.resolved_actions.iter())
        .filter(|ra| matches!(action_kind(&ra.action), ActionKind::Speak))
        .collect();
    let requests: Vec<TtsRequest> = speech
        .iter()
        .map(|ra| TtsRequest {
            text: ra.text.clone().unwrap_or_default(),
            voice_id: world
                .character(&ra.actor_id)
                .map(|c| c.voice_id.clone())
                .unwrap_or_else(|| ra.actor_id.clone()),
            delivery: ra.delivery.clone(),
        })
        .collect();
    for (ra, res) in speech.iter().zip(tts.synthesize_batch(&requests)) {
        let text = ra.text.clone().unwrap_or_default();
        if res.ok {
            any_real = true;
        }
        // Trim trailing/leading silence so the schedule duration matches the
        // real spoken length (and the cached mix clip carries no padding).
        let dur = if let Some(p) = &res.audio_path {
            let t = trim_wav_silence_in_place(p, config.tts.sample_rate, 0.01);
            if t > 0.0 {
                t
            } else {
                res.duration
            }
        } else {
            res.duration
        };
        tts_durations.insert((ra.actor_id.clone(), text.clone()), dur);
        tts_results.insert((ra.actor_id.clone(), text), res);
    }
    if require_llm {
        let failures = tts_results.values().filter(|result| !result.ok).count();
        if failures > 0 || tts_results.len() != requests.len() {
            return Err(crate::error::CoreError::Llm(format!(
                "production voice phase incomplete: {failures} failed and {} of {} lines returned",
                tts_results.len(),
                requests.len()
            )));
        }
    }
    let tts_generation_secs = t_tts_start.elapsed().as_secs_f32();
    let mut alignment_ids = HashMap::new();
    let mut wavs = Vec::new();
    for ((actor, text), result) in &tts_results {
        if let Some(path) = &result.audio_path {
            let id = blake3::hash(format!("{actor}\0{text}").as_bytes())
                .to_hex()
                .to_string();
            alignment_ids.insert(id.clone(), (actor.clone(), text.clone()));
            wavs.push((id, path.clone()));
        }
    }
    let alignment_batch = crate::asr::align_wavs(&config.asr, &wavs)?;
    let speech_alignment_secs = alignment_batch.elapsed_secs;
    let word_alignments: HashMap<(String, String), crate::asr::WordAlignment> = alignment_batch
        .alignments
        .into_iter()
        .filter_map(|(id, alignment)| alignment_ids.get(&id).cloned().map(|key| (key, alignment)))
        .collect();
    if require_llm && !wavs.is_empty() && word_alignments.len() != wavs.len() {
        return Err(crate::error::CoreError::Llm(format!(
            "Parakeet alignment phase incomplete: aligned {} of {} dialogue clips",
            word_alignments.len(),
            wavs.len()
        )));
    }
    let t_timeline = std::time::Instant::now();
    let mut sched = build_schedule(world, &validated, &tts_durations);
    // Phase 8: compress the timeline so content starts within ~1s and no
    // inter-line gap exceeds the dead-air limit.
    compact_dead_air(&mut sched, config.runtime.max_dead_air_secs);
    crate::timeline::apply_word_aligned_captions(&mut sched, &word_alignments);
    let timeline_prep_secs = t_timeline.elapsed().as_secs_f32();
    let t_motion = std::time::Instant::now();
    let mut motion_plan =
        crate::motion::compile_motion_plan(&sched, Path::new("assets/animations/library"))
            .map_err(crate::error::CoreError::Msg)?;
    let mut motion_processing_secs = t_motion.elapsed().as_secs_f32();
    let mut kimodo_generation_secs = 0.0;
    if require_llm && !motion_plan.unresolved.is_empty() {
        let generated = crate::motion::generate_unresolved_motion(
            &motion_plan.unresolved,
            Path::new("."),
            Path::new("assets/animations/library"),
            Path::new("output/cache/kimodo"),
            seed,
        )
        .map_err(crate::error::CoreError::Msg)?;
        kimodo_generation_secs += generated.generation_secs;
        motion_processing_secs += generated.processing_secs;
        motion_plan =
            crate::motion::compile_motion_plan(&sched, Path::new("assets/animations/library"))
                .map_err(crate::error::CoreError::Msg)?;
        if !motion_plan.unresolved.is_empty() {
            let manifests = generated
                .generated_manifests
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(crate::error::CoreError::Msg(format!(
                "production motion review required for semantics [{}]; Bevy previews must be approved in: {}",
                motion_plan.unresolved.join(", "),
                manifests
            )));
        }
    }
    let (min_duration, max_duration) =
        duration_acceptance_window(config.runtime.target_duration_secs);
    if sched.duration < min_duration || sched.duration > max_duration {
        let msg = format!(
            "authored episode runtime {:.1}s outside required {:.1}-{:.1}s after measured TTS and dead-air compaction",
            sched.duration,
            min_duration,
            max_duration
        );
        if require_llm {
            return Err(crate::error::CoreError::Llm(format!("require_llm: {msg}")));
        }
        tracing::warn!("{msg}");
    }
    for d in &sched.dialogue {
        if let Some(res) = tts_results.get(&(d.actor.clone(), d.text.clone())) {
            if let Some(p) = &res.audio_path {
                // Idempotent: re-trim in case this line was not seen in the action loop.
                let _ = trim_wav_silence_in_place(p, config.tts.sample_rate, 0.01);
                clips.push((p.clone(), d.start, d.end - d.start));
            }
        }
    }
    for scheduled in &sched.sounds {
        let (path, duration) = ensure_semantic_sfx(
            &scheduled.cue.sound,
            scheduled.cue.gain,
            config.tts.sample_rate,
        )?;
        clips.push((path, scheduled.start.max(0.0), duration));
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
        prep_timings: PrepTimings {
            llm_authoring_secs,
            tts_generation_secs,
            speech_alignment_secs,
            kimodo_generation_secs,
            motion_processing_secs,
            timeline_prep_secs,
        },
        word_alignments,
        motion_plan,
        runtime_telemetry: backlot_runtime::snapshot_global_telemetry(),
    })
}

fn ensure_semantic_sfx(
    semantic: &str,
    gain: f32,
    sample_rate: u32,
) -> crate::error::Result<(String, f32)> {
    let duration = match semantic {
        "elevator_ding" => 0.9,
        "door_motor" => 1.6,
        "panel_beep" => 0.28,
        "indicator_glitch" => 0.55,
        "electrical_flicker" => 0.45,
        "footsteps" => 1.2,
        "impossible_floor_ambience" => 3.0,
        "reaction_sting" => 0.7,
        other => {
            return Err(crate::error::CoreError::Msg(format!(
                "unknown semantic SFX id {other}"
            )))
        }
    };
    let root = Path::new("assets/audio/sfx");
    std::fs::create_dir_all(root).map_err(io_err(root))?;
    let gain_key = (gain.clamp(0.0, 2.0) * 100.0).round() as u32;
    let path = root.join(format!("{semantic}_{gain_key:03}_{sample_rate}.wav"));
    if path.exists() {
        return Ok((path.to_string_lossy().into_owned(), duration));
    }
    let count = (duration * sample_rate as f32).round() as usize;
    let mut pcm = Vec::with_capacity(count);
    let mut noise = 0x9E37_79B9u32;
    for index in 0..count {
        let t = index as f32 / sample_rate as f32;
        let fade_in = (t / 0.03).clamp(0.0, 1.0);
        let fade_out = ((duration - t) / 0.12).clamp(0.0, 1.0);
        let envelope = fade_in * fade_out;
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = ((noise >> 8) as f32 / 16_777_215.0) * 2.0 - 1.0;
        let sample = match semantic {
            "elevator_ding" => {
                (std::f32::consts::TAU * 880.0 * t).sin() * (-2.6 * t).exp()
                    + 0.35 * (std::f32::consts::TAU * 1320.0 * t).sin() * (-4.0 * t).exp()
            }
            "panel_beep" => (std::f32::consts::TAU * 1180.0 * t).sin(),
            "door_motor" => 0.55 * (std::f32::consts::TAU * (82.0 + 18.0 * t) * t).sin() + 0.18 * n,
            "indicator_glitch" => {
                let gate = if (t * 34.0).fract() > 0.42 { 1.0 } else { 0.0 };
                gate * ((std::f32::consts::TAU * 720.0 * t).sin() + 0.35 * n)
            }
            "electrical_flicker" => 0.7 * n * if (t * 27.0).fract() > 0.5 { 1.0 } else { 0.15 },
            "footsteps" => {
                let phase = (t * 3.4).fract();
                n * (-18.0 * phase).exp()
            }
            "impossible_floor_ambience" => {
                0.45 * (std::f32::consts::TAU * 46.0 * t).sin()
                    + 0.2 * (std::f32::consts::TAU * 71.0 * t).sin()
                    + 0.08 * n
            }
            "reaction_sting" => {
                (std::f32::consts::TAU * (260.0 + 360.0 * t) * t).sin() * (-2.0 * t).exp()
            }
            _ => 0.0,
        };
        pcm.push((sample * envelope * gain.clamp(0.0, 2.0) * 8_000.0) as i16);
    }
    write_wav(path.to_string_lossy().as_ref(), sample_rate, &pcm);
    Ok((path.to_string_lossy().into_owned(), duration))
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

#[derive(Debug, Serialize)]
struct AnimationStateSample {
    time: f32,
    characters: Vec<AnimationCharacterSample>,
    camera_intent: Option<String>,
    camera_subject: Option<String>,
    flicker: bool,
    elevator_open: f32,
}

#[derive(Debug, Serialize)]
struct AnimationCharacterSample {
    id: String,
    state: PerformanceState,
    speaking: bool,
    position: [f32; 3],
    yaw: f32,
}

fn write_animation_state_timeline(
    review_dir: &Path,
    sched: &Schedule,
    rigs: &HashMap<String, HumanoidRig>,
    world: &WorldState,
) -> crate::error::Result<()> {
    let mut times = vec![0.0f32, sched.duration];
    for track in &sched.characters {
        for action in &track.actions {
            times.push(action.start);
            times.push((action.start + action.dur).min(sched.duration));
        }
    }
    for shot in &sched.camera_shots {
        times.push(shot.start);
        times.push(shot.end.min(sched.duration));
    }
    for caption in &sched.captions {
        times.push(caption.start);
        times.push(caption.end.min(sched.duration));
    }
    let mut t = 0.0f32;
    while t <= sched.duration {
        times.push(t);
        t += 0.5;
    }
    times.sort_by(|a, b| a.total_cmp(b));
    times.dedup_by(|a, b| (*a - *b).abs() < 0.01);

    let samples: Vec<AnimationStateSample> = times
        .into_iter()
        .map(|time| {
            let state = evaluate_at(sched, rigs, world, time.min(sched.duration));
            let active_shot = sched
                .camera_shots
                .iter()
                .find(|shot| time >= shot.start && time < shot.end)
                .or_else(|| sched.camera_shots.last());
            AnimationStateSample {
                time,
                characters: state
                    .chars
                    .iter()
                    .map(|(frame, _)| AnimationCharacterSample {
                        id: frame.id.clone(),
                        state: frame.state,
                        speaking: frame.speaking,
                        position: frame.root.pos,
                        yaw: frame.root.rot[1],
                    })
                    .collect(),
                camera_intent: active_shot.map(|shot| shot.intent.clone()),
                camera_subject: active_shot.map(|shot| {
                    if shot.intent == "reaction" {
                        shot.reaction
                            .clone()
                            .unwrap_or_else(|| shot.subject.clone())
                    } else {
                        shot.subject.clone()
                    }
                }),
                flicker: state.flicker,
                elevator_open: state.elevator_open,
            }
        })
        .collect();
    let path = review_dir.join("animation_state_timeline.json");
    let json = serde_json::to_string_pretty(&samples)?;
    std::fs::write(&path, json).map_err(io_err(&path))?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct ReviewFrameEntry {
    category: String,
    timestamp: f32,
    source: String,
    file: String,
    label: String,
}

fn package_review_frames(
    config: &Config,
    ep_dir: &Path,
    frames_dir: &Path,
    sched: &Schedule,
) -> crate::error::Result<Vec<ReviewFrameEntry>> {
    let review_frames = ep_dir.join("review").join("frames");
    let sheet_frames = ep_dir.join("review").join("contact_sheet_frames");
    if review_frames.exists() {
        std::fs::remove_dir_all(&review_frames).map_err(io_err(&review_frames))?;
    }
    if sheet_frames.exists() {
        std::fs::remove_dir_all(&sheet_frames).map_err(io_err(&sheet_frames))?;
    }
    std::fs::create_dir_all(&review_frames).map_err(io_err(&review_frames))?;
    std::fs::create_dir_all(&sheet_frames).map_err(io_err(&sheet_frames))?;
    let fps = config.runtime.frame_rate.max(1);
    let mut requests: Vec<(String, f32, String, bool)> = Vec::new();

    let mut t = 0.0f32;
    while t <= sched.duration {
        requests.push(("periodic".into(), t, format!("{t:.1}s"), false));
        t += 2.0;
    }
    for (index, shot) in sched.camera_shots.iter().enumerate() {
        let mid = (shot.start + shot.end) * 0.5;
        requests.push((
            "shot".into(),
            mid,
            format!("shot {:02} {}", index + 1, shot.intent),
            false,
        ));
        if shot.intent == "reaction" {
            requests.push((
                "reaction".into(),
                mid,
                format!(
                    "reaction {}",
                    shot.reaction.as_deref().unwrap_or(&shot.subject)
                ),
                false,
            ));
        }
    }
    for (index, (time, target)) in sched.inserts.iter().enumerate() {
        requests.push((
            "interaction".into(),
            *time,
            format!("interaction {:02} {target}", index + 1),
            false,
        ));
    }
    for (index, caption) in sched.captions.iter().enumerate() {
        requests.push((
            "caption".into(),
            (caption.start + caption.end) * 0.5,
            format!("caption {:02}", index + 1),
            true,
        ));
    }

    let mut entries = Vec::new();
    let captioned_video = ep_dir.join("output").join("vertical_captioned.mp4");
    for (index, (category, timestamp, label, captioned)) in requests.into_iter().enumerate() {
        let safe_time = timestamp.clamp(0.0, (sched.duration - 0.001).max(0.0));
        let file_name = format!(
            "{:03}_{}_{}.png",
            index + 1,
            category,
            (safe_time * 100.0).round() as u32
        );
        let output = review_frames.join(&file_name);
        let copied = if captioned {
            let args = vec![
                "-y".into(),
                "-ss".into(),
                format!("{safe_time:.3}"),
                "-i".into(),
                captioned_video.to_string_lossy().replace('\\', "/"),
                "-frames:v".into(),
                "1".into(),
                output.to_string_lossy().replace('\\', "/"),
            ];
            run_ffmpeg(&config.runtime.ffmpeg_path, &args)?
        } else {
            let frame_number = ((safe_time * fps as f32).round() as u32 + 1).max(1);
            let source = frames_dir.join(format!("frame_{frame_number:06}.png"));
            std::fs::copy(&source, &output).is_ok()
        };
        if copied {
            entries.push(ReviewFrameEntry {
                category,
                timestamp: safe_time,
                source: if captioned {
                    "captioned_mp4".into()
                } else {
                    "bevy_capture_frame".into()
                },
                file: format!("review/frames/{file_name}"),
                label,
            });
        }
    }

    let periodic: Vec<&ReviewFrameEntry> = entries
        .iter()
        .filter(|entry| entry.category == "periodic")
        .collect();
    for (index, entry) in periodic.iter().enumerate() {
        let source = ep_dir.join(&entry.file);
        let target = sheet_frames.join(format!("sheet_{:03}.png", index + 1));
        std::fs::copy(source, target).map_err(io_err(&sheet_frames))?;
    }
    if !periodic.is_empty() {
        let pattern = sheet_frames
            .join("sheet_%03d.png")
            .to_string_lossy()
            .replace('\\', "/");
        let output = ep_dir
            .join("review")
            .join("contact_sheet.jpg")
            .to_string_lossy()
            .replace('\\', "/");
        let args = vec![
            "-y".into(),
            "-framerate".into(),
            "1".into(),
            "-i".into(),
            pattern,
            "-vf".into(),
            "scale=216:384,tile=5x6".into(),
            "-frames:v".into(),
            "1".into(),
            output,
        ];
        if !run_ffmpeg(&config.runtime.ffmpeg_path, &args)? {
            return Err(crate::error::CoreError::Msg(
                "failed to generate review contact sheet".into(),
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&sheet_frames);

    let index_path = ep_dir.join("review").join("frame_index.json");
    let json = serde_json::to_string_pretty(&entries)?;
    std::fs::write(&index_path, json).map_err(io_err(&index_path))?;
    Ok(entries)
}

fn write_review_handoff(
    ep_dir: &Path,
    plan: &EpisodePlan,
    provider: &str,
    render_backend: &str,
    entries: &[ReviewFrameEntry],
    issues: &[String],
) -> crate::error::Result<()> {
    let mut body = format!(
        "# External Visual Review Handoff\n\n\
         **Candidate:** {}\n\
         **Renderer:** {}\n\
         **TTS provider:** {}\n\n\
         This MP4 is a candidate for external visual review. No automated result in this package is a claim that it looks good.\n\n\
         ## Review assets\n\
         - Captioned candidate: `output/vertical_captioned.mp4`\n\
         - Clean candidate: `output/vertical_clean.mp4`\n\
         - Review-frame index: `review/frame_index.json` ({} frames)\n\
         - Periodic contact sheet: `review/contact_sheet.jpg`\n\
         - Animation timeline: `review/animation_state_timeline.json`\n\
         - Camera diagnostics: `review/framing_report.json`\n\
         - Silence diagnostics: `review/silence_report.json`\n\n\
         ## Automated issues\n",
        plan.episode_title,
        render_backend,
        provider,
        entries.len(),
    );
    if issues.is_empty() {
        body.push_str(
            "- No automated failures were reported. External visual judgment is still required.\n",
        );
    } else {
        for issue in issues {
            body.push_str(&format!("- {issue}\n"));
        }
    }
    body.push_str(
        "\n## Reviewer checklist\n\
         - Character motion, gesture recovery, gaze, and listener reactions\n\
         - Shot purpose, occlusion, continuity, and interaction readability\n\
         - Hallway/elevator exposure, material separation, and door movement\n\
         - Caption clipping, wrapping, phone readability, and action overlap\n\
         - Dialogue intelligibility, voice quality, ambience, and sync\n\
         - Escalation, payoff, and final hold\n",
    );
    let path = ep_dir.join("REVIEW_HANDOFF.md");
    std::fs::write(&path, body).map_err(io_err(&path))?;
    Ok(())
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
    timing_context: Option<&ProductionTimingContext>,
) -> crate::error::Result<ProduceReport> {
    let out_dir = &config.runtime.output_dir;
    let ep_dir = Path::new(out_dir).join("episodes").join(&prep.episode_id);
    let audio_dir = ep_dir.join("audio");
    let llm_dir = ep_dir.join("llm");
    let review_dir = ep_dir.join("review");
    std::fs::create_dir_all(&audio_dir).map_err(io_err(&ep_dir))?;
    std::fs::create_dir_all(&llm_dir).map_err(io_err(&ep_dir))?;
    std::fs::create_dir_all(&review_dir).map_err(io_err(&ep_dir))?;

    let sched = &prep.schedule;
    let rigs = &prep.rigs;
    let world = &prep.world_before;
    let plan = prep.planned.plan.clone();
    let auth = &prep.auth;

    // Persist the final authored plan + per-beat commands + authorship so the
    // exact LLM-authored episode is reproducible and auditable.
    let _ = std::fs::write(
        llm_dir.join("final_plan.json"),
        serde_json::to_string_pretty(&plan).unwrap_or_default(),
    );
    let _ = std::fs::write(
        llm_dir.join("final_commands.json"),
        serde_json::to_string_pretty(&prep.planned.commands).unwrap_or_default(),
    );
    let _ = std::fs::write(
        llm_dir.join("authorship.json"),
        serde_json::to_string_pretty(auth).unwrap_or_default(),
    );
    let alignment_records: Vec<_> = prep
        .word_alignments
        .iter()
        .map(|((actor, text), alignment)| {
            serde_json::json!({
                "actor": actor,
                "authored_text": text,
                "alignment": alignment,
            })
        })
        .collect();
    std::fs::write(
        audio_dir.join("word_alignments.json"),
        serde_json::to_vec_pretty(&alignment_records).unwrap_or_default(),
    )
    .map_err(io_err(&audio_dir))?;

    // Mix audio
    let t_mix = std::time::Instant::now();
    let sr = config.tts.sample_rate;
    let mix_path = audio_dir.join("final_mix.wav");
    mix_audio(&prep.clips, mix_path.to_str().unwrap(), sr, sched.duration);
    let audio_mixing_secs = t_mix.elapsed().as_secs_f32();

    // Encode MP4
    let t_enc = std::time::Instant::now();
    let fps = config.runtime.frame_rate.max(1);
    let cap_out = ep_dir.join("output").join("vertical_captioned.mp4");
    let clean_out = ep_dir.join("output").join("vertical_clean.mp4");
    let muted_out = ep_dir.join("output").join("vertical_muted.mp4");
    std::fs::create_dir_all(ep_dir.join("output")).map_err(io_err(&ep_dir))?;
    let (cmd, enc_ok) = encode_mp4(
        config,
        frames_dir.to_str().unwrap(),
        mix_path.to_str().unwrap(),
        cap_out.to_str().unwrap(),
        clean_out.to_str().unwrap(),
        muted_out.to_str().unwrap(),
        &sched.captions,
        config.runtime.resolution,
        fps,
    )?;
    let ffmpeg_encode_secs = t_enc.elapsed().as_secs_f32();

    // Verify
    let probe = verify_mp4(config, cap_out.to_str().unwrap());
    let ffprobe_ok = probe.has_video && probe.has_audio && probe.duration >= sched.duration * 0.8;

    // Package
    let t_pack = std::time::Instant::now();
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
        sched
            .camera_shots
            .iter()
            .map(|s| s.end - s.start)
            .sum::<f32>()
            / sched.camera_shots.len() as f32
    };
    m.longest_shot_duration = sched
        .camera_shots
        .iter()
        .map(|s| s.end - s.start)
        .fold(0.0f32, f32::max);
    m.visual_changes_per_min = (sched.events.len() as f32) / (sched.duration / 60.0);
    m.payoff_complete = !plan.payoff.trim().is_empty();
    m.persistent_consequence = !plan.persistent_changes.is_empty();

    // Caption safety: validate the exact explicit ASS wrap against the configured
    // safe band. Three short lines are permitted for long 8-16 word dialogue.
    let (cw, ch) = (config.runtime.resolution.0, config.runtime.resolution.1);
    let fontsize = ((ch as f32 * 0.027).round() as u32).clamp(42, 54) as f32;
    let margin_h = ((cw as f32 * 0.08).round() as u32).clamp(80, 140) as f32;
    let safe_w = cw as f32 - 2.0 * margin_h;
    let wrap_chars = (safe_w / (fontsize * 0.55)).floor().max(1.0) as usize;
    let mut safe_count = 0u32;
    let mut unsafe_captions: Vec<String> = Vec::new();
    for c in &sched.captions {
        let normalized = normalize_caption_text(&c.text);
        let lines = wrap_caption_lines(&normalized, wrap_chars);
        let widest =
            lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * fontsize * 0.55;
        if lines.len() <= 3 && widest <= safe_w {
            safe_count += 1;
        } else {
            unsafe_captions.push(format!(
                "[{:.1}-{:.1}] {} line(s), ~{:.0}px wide",
                c.start,
                c.end,
                lines.len(),
                widest
            ));
        }
    }
    m.caption_safe_pct = if sched.captions.is_empty() {
        1.0
    } else {
        safe_count as f32 / sched.captions.len() as f32
    } * 100.0;

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

    // --- Camera legibility + framing diagnostics (Phase 6) ---
    // Objective, image-free proof that each shot frames a real, visible,
    // un-occluded performer. Re-renders each shot midpoint through the same
    // software renderer and reads the object-id + depth buffers.
    let (rw, rh) = (
        config.runtime.resolution.0 / 2,
        config.runtime.resolution.1 / 2,
    );
    let cam_analysis = analyze_schedule(sched, rigs, world, rw, rh);
    let rejected: Vec<&ShotAnalysis> = cam_analysis.iter().filter(|a| a.rejected).collect();
    let means = |sel: fn(&ShotAnalysis) -> f32| {
        let v: Vec<f32> = cam_analysis.iter().map(sel).collect();
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f32>() / v.len() as f32
        }
    };
    let cam_quality = serde_json::json!({
        "resolution": [rw, rh],
        "shots": cam_analysis,
        "summary": {
            "total_shots": cam_analysis.len(),
            "rejected_shots": rejected.len(),
            "rejected_intents": rejected.iter().map(|a| a.intent.clone()).collect::<Vec<_>>(),
            "mean_subject_height_fraction": means(|a| a.subject_height_fraction),
            "mean_subject_area_fraction": means(|a| a.subject_area_fraction),
            "mean_occlusion": means(|a| a.occlusion),
            "mean_set_fraction": means(|a| a.set_fraction),
        }
    });
    let _ = std::fs::write(
        ep_dir.join("review").join("camera_quality.json"),
        serde_json::to_string_pretty(&cam_quality).unwrap_or_default(),
    );
    let _ = std::fs::write(
        ep_dir.join("review").join("framing_report.json"),
        serde_json::to_string_pretty(&cam_quality).unwrap_or_default(),
    );

    // --- Silence / dead-air analysis of the actual mixed audio (Phase 8) ---
    let silence = detect_silence(&mix_path.to_string_lossy(), cfg_silence_min_gap(), 0.012);
    let first_content = silence.first_content_secs;
    let max_gap = silence.max_gap_secs;
    let silence_report = serde_json::json!({
        "min_gap_secs": cfg_silence_min_gap(),
        "first_content_secs": first_content,
        "max_gap_secs": max_gap,
        "dead_air_limit_exceeded": max_gap > config.runtime.max_dead_air_secs,
        "duration_secs": sched.duration,
        "silent_ranges": silence.ranges,
    });
    let _ = std::fs::write(
        ep_dir.join("review").join("silence_report.json"),
        serde_json::to_string_pretty(&silence_report).unwrap_or_default(),
    );

    write_animation_state_timeline(&review_dir, sched, rigs, world)?;
    let review_frames = package_review_frames(config, &ep_dir, frames_dir, sched)?;

    let diagnostics = {
        let mut issues: Vec<String> = Vec::new();
        let rejected_shots: Vec<&ShotAnalysis> =
            cam_analysis.iter().filter(|a| a.rejected).collect();
        if !rejected_shots.is_empty() {
            issues.push(format!(
                "camera: {} of {} shots rejected by framing analysis ({})",
                rejected_shots.len(),
                cam_analysis.len(),
                rejected_shots
                    .iter()
                    .map(|a| format!("{} @ {:.1}s: {}", a.intent, a.start, a.reject_reason))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        let zero_len = camera_plan
            .iter()
            .filter(|s| s.end - s.start <= 0.05)
            .count();
        if zero_len > 0 {
            issues.push(format!(
                "camera: {zero_len} zero-length shots in camera plan"
            ));
        }
        if max_gap > config.runtime.max_dead_air_secs {
            issues.push(format!(
                "audio: dead-air gap {max_gap:.1}s exceeds {:.1}s limit",
                config.runtime.max_dead_air_secs
            ));
        }
        if prep.tts_provider == "estimating" || prep.tts_provider.ends_with("-failed") {
            issues.push(format!(
                "audio: tts provider '{}' is not production-quality; configure a real local engine",
                prep.tts_provider
            ));
        }
        if !ffprobe_ok {
            issues.push("mux: ffprobe verification failed".into());
        }
        if !unsafe_captions.is_empty() {
            issues.push(format!(
                "captions: {} of {} cues exceed safe bounds ({})",
                unsafe_captions.len(),
                sched.captions.len(),
                unsafe_captions.join("; ")
            ));
        }
        Diagnostics {
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
            issues,
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
            replay_no_llm: auth.validation_status.contains("reused")
                && auth.attempts == 0
                && auth.beats.iter().all(|beat| beat.attempts == 0),
            render_backend: render_backend.to_string(),
            timing: None,
        }
    };

    write_review_handoff(
        &ep_dir,
        &plan,
        &prep.tts_provider,
        render_backend,
        &review_frames,
        &diagnostics.issues,
    )?;

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
        thumbnail_candidates: review_frames
            .iter()
            .find(|entry| entry.category == "periodic")
            .map(|entry| entry.file.clone())
            .into_iter()
            .collect(),
        story_tags: plan.tone.clone(),
        quality_scores: Default::default(),
        detected_issues: diagnostics.issues.clone(),
        canonical: true,
        suggested_posting_caption: format!("{} #shorts", plan.episode_title),
        suggested_compilation_category: "surreal-comedy".into(),
    };

    // Attach one authoritative timing report before building either persisted
    // report. Renderer-owned capture timing and shared mix/encode timing must
    // never be written in separate passes because that makes the artifacts drift.
    let packaging_secs = t_pack.elapsed().as_secs_f32();
    let ended_at = chrono::Utc::now().to_rfc3339();
    let default_total = prep.prep_timings.llm_authoring_secs
        + prep.prep_timings.tts_generation_secs
        + prep.prep_timings.speech_alignment_secs
        + prep.prep_timings.kimodo_generation_secs
        + prep.prep_timings.motion_processing_secs
        + prep.prep_timings.timeline_prep_secs
        + timing_context.map(|t| t.bevy_capture_secs).unwrap_or(0.0)
        + audio_mixing_secs
        + ffmpeg_encode_secs
        + packaging_secs;
    let mut diagnostics_with_timing = diagnostics.clone();
    diagnostics_with_timing.timing = Some(crate::package::TimingReport {
        schema_version: 2,
        llm_authoring: prep.prep_timings.llm_authoring_secs,
        tts: prep.prep_timings.tts_generation_secs,
        speech_alignment: prep.prep_timings.speech_alignment_secs,
        kimodo_generation: prep.prep_timings.kimodo_generation_secs,
        motion_processing: prep.prep_timings.motion_processing_secs,
        timeline_assembly: prep.prep_timings.timeline_prep_secs,
        bevy_capture: timing_context.map(|t| t.bevy_capture_secs).unwrap_or(0.0),
        audio_mixing: audio_mixing_secs,
        encoding: ffmpeg_encode_secs,
        review_packaging: packaging_secs,
        total_production_time: timing_context
            .map(|t| {
                t.elapsed_before_finalize_secs
                    + audio_mixing_secs
                    + ffmpeg_encode_secs
                    + packaging_secs
            })
            .unwrap_or(default_total),
        llm_authoring_secs: prep.prep_timings.llm_authoring_secs,
        tts_generation_secs: prep.prep_timings.tts_generation_secs,
        timeline_prep_secs: prep.prep_timings.timeline_prep_secs,
        bevy_capture_secs: timing_context.map(|t| t.bevy_capture_secs).unwrap_or(0.0),
        audio_mixing_secs,
        ffmpeg_encode_secs,
        packaging_secs,
        total_end_to_end_secs: timing_context
            .map(|t| {
                t.elapsed_before_finalize_secs
                    + audio_mixing_secs
                    + ffmpeg_encode_secs
                    + packaging_secs
            })
            .unwrap_or(default_total),
        model_phases: prep.runtime_telemetry.phases.clone(),
        effective_fps: timing_context.and_then(|t| t.effective_fps),
        started_at: timing_context
            .map(|t| t.started_at.clone())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        ended_at,
    });

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
        diagnostics: diagnostics_with_timing.clone(),
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
    write_tts_manifest(
        &audio_dir,
        &prep.clips,
        prep.tts_provider.clone(),
        prep.any_real,
    );

    Ok(ProduceReport {
        episode_id: prep.episode_id.clone(),
        mp4_captioned: cap_out.to_string_lossy().into_owned(),
        mp4_clean: clean_out.to_string_lossy().into_owned(),
        mp4_muted: muted_out.to_string_lossy().into_owned(),
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
    let ProduceConfig {
        config,
        require_llm,
        world,
        seed,
        episode_number,
        keep_frames,
    } = cfg;
    let out_dir = config.runtime.output_dir.clone();
    let ep_dir = Path::new(&out_dir)
        .join("episodes")
        .join(serial_id("episode", episode_number, 6));
    let frames_dir = ep_dir.join("frames");
    std::fs::create_dir_all(&frames_dir).map_err(io_err(&ep_dir))?;

    let prep = prepare_production(&config, require_llm, &world, seed, episode_number, &*author)?;

    // CPU software-rasterizer frame capture (deterministic, no LLM).
    let fps = config.runtime.frame_rate.max(1);
    let (rw, rh) = (
        config.runtime.resolution.0 / 2,
        config.runtime.resolution.1 / 2,
    );
    let renderer = StageRenderer::new(rw.max(2), rh.max(2));
    let n_frames = (prep.schedule.duration * fps as f32).ceil() as u32;
    let mut captured = 0u32;
    let mut geo = GeometryStats::default();
    for i in 0..n_frames {
        let t = i as f32 / fps as f32;
        let state = evaluate_at(&prep.schedule, &prep.rigs, &world, t);
        let buf = renderer.render_buffers(&state, &prep.rigs, &world, None);
        let rgba = &buf.color;
        let fstats = &buf.stats;
        geo.total_triangles += fstats.total_triangles;
        geo.behind_camera += fstats.behind_camera;
        geo.non_finite += fstats.non_finite;
        geo.clipped_near += fstats.clipped_near;
        geo.implausible_character_triangles += fstats.implausible_character_triangles;
        geo.max_triangle_screen_fraction = geo
            .max_triangle_screen_fraction
            .max(fstats.max_triangle_screen_fraction);
        let path = frames_dir.join(format!("frame_{:06}.png", i + 1));
        if write_png(&path, rw, rh, rgba).is_err() {
            tracing::warn!("frame write failed");
            break;
        }
        captured += 1;
    }

    // Truthful geometry-correctness artifact (the objective "visual review").
    let _ = std::fs::create_dir_all(ep_dir.join("review"));
    let _ = std::fs::write(
        ep_dir.join("review").join("geometry_diagnostics.json"),
        serde_json::to_string_pretty(&geo).unwrap_or_default(),
    );

    let report = finalize_production(
        &config,
        require_llm,
        &prep,
        &frames_dir,
        captured,
        "cpu_software",
        None,
    )?;

    if !keep_frames && captured > 0 {
        let _ = std::fs::remove_dir_all(&frames_dir);
    }
    Ok(report)
}

// ---- helpers for produce ----

fn io_err<'a>(p: &'a Path) -> impl FnOnce(std::io::Error) -> crate::error::CoreError + 'a {
    move |source| crate::error::CoreError::Io {
        path: p.to_path_buf(),
        source,
    }
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
    Some(ValidatedPlan {
        plan: planned.plan.clone(),
        resolved_beats: resolved,
    })
}

pub fn build_rigs(world: &WorldState) -> HashMap<String, HumanoidRig> {
    let mut m = HashMap::new();
    for c in world.characters.values() {
        let col = hex_rgb(&c.color_hex);
        m.insert(
            c.id.clone(),
            HumanoidRig::default_humanoid(&c.id, &c.voice_id, col),
        );
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
    let mut writer = enc
        .write_header()
        .map_err(|e| crate::error::CoreError::Llm(format!("png header: {e}")))?;
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
        let src = auth
            .beats
            .iter()
            .find(|x| x.beat_id == b.id)
            .map(|x| x.source.as_str())
            .unwrap_or("unknown");
        reqs.push_str(
            &serde_json::to_string(&serde_json::json!({
                "beat_id": b.id, "source": src, "request": "BeatCommand request"
            }))
            .unwrap_or_default(),
        );
        reqs.push('\n');
        let resp = cmd
            .map(|c| serde_json::to_string(c).unwrap_or_default())
            .unwrap_or_else(|| format!("{{\"source\":\"{src}\"}}"));
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
    use super::*;
    use crate::avatar::{
        character_pose, HumanoidRig, PerformanceState, Pose, SemanticJoint, Xform,
    };
    use crate::timeline::{
        evaluate_at, CameraShotSpec, CharFrame, CharTrack, FrameState, Schedule, ScheduledAction,
    };
    use crate::world::build_default_world;
    use std::collections::HashMap;

    #[test]
    fn frame_motion_detects_change_and_identity() {
        let blank = vec![10u8; 4 * 4 * 4];
        let mut diff = blank.clone();
        let mid = 4 * 4 * 4 / 2;
        diff[mid] = 220;
        diff[mid + 1] = 220;
        diff[mid + 2] = 220;
        assert!(
            frame_motion(&blank, &diff) > 0.0,
            "changed frame must show motion"
        );
        assert_eq!(
            frame_motion(&blank, &blank),
            0.0,
            "identical frames show no motion"
        );
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
                    performance: None,
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
            environment: vec![],
            sounds: vec![],
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

    // ---- Phase 3: caption normalization (the "fused words" corruption) ----

    #[test]
    fn caption_normalization_fixes_fused_words() {
        // The original corruption fused "The elevator's" into "Thenelevator's"
        // because a stray newline ate the space. Normalization must restore it.
        assert_eq!(
            normalize_caption_text("The\nelevator's feeling warm tonight"),
            "The elevator's feeling warm tonight"
        );
        assert_eq!(
            normalize_caption_text("  too   many   spaces  "),
            "too many spaces"
        );
        // Punctuation and apostrophes are preserved verbatim.
        assert_eq!(
            normalize_caption_text("don't — it's fine."),
            "don't — it's fine."
        );
    }

    #[test]
    fn escape_ass_reserves_backslash_comma_newline() {
        assert_eq!(escape_ass("a,b"), "a\\,b");
        assert_eq!(escape_ass("a\\b"), "a\\\\b");
        // a literal newline in a caption is normalized to a space (fused-words
        // fix); it must never survive as a raw newline or an ASS \N break.
        assert_eq!(escape_ass("line1\nline2"), "line1 line2");
        // apostrophe must survive unescaped
        assert_eq!(escape_ass("it's"), "it's");
    }

    #[test]
    fn build_ass_subtitles_is_valid_and_per_caption() {
        let caps = vec![
            Caption {
                start: 0.5,
                end: 2.0,
                text: "The\nelevator's feeling warm".into(),
            },
            Caption {
                start: 2.5,
                end: 4.0,
                text: "Don't crowd the door".into(),
            },
        ];
        let ass = build_ass_subtitles(&caps, (1080, 1920));
        assert!(ass.contains("[Script Info]"));
        assert!(ass.contains("PlayResX: 1080"));
        assert!(ass.contains("PlayResY: 1920"));
        assert!(ass.contains("Style: Default"));
        // one Dialogue line per caption
        assert_eq!(ass.matches("Dialogue:").count(), 2);
        // the fused newline in the source must have been normalized to a space
        // (the original corruption produced "Thenelevator's"); the *normalized*
        // text must appear and the raw newline form must not.
        assert!(ass.contains("elevator's feeling warm"));
        assert!(!ass.contains("The\nelevator"));
        // no raw ffmpeg-style escape syntax should leak into the ASS body
        assert!(!ass.contains("\\:"));
        assert!(!ass.contains("\\%"));
    }

    // ---- Phase 3: homogeneous near-plane clipping ----

    #[test]
    fn clip_near_rejects_geometry_behind_camera() {
        // Polygon entirely behind the near plane (z < near) -> empty.
        let behind = [[-1.0, -1.0, -0.5], [1.0, -1.0, -0.5], [0.0, 1.0, -0.5]];
        assert!(clip_near(&behind, 0.08).is_empty());
    }

    #[test]
    fn clip_near_keeps_no_vertex_behind() {
        // A triangle straddling the near plane must be clipped so *every*
        // resulting vertex has z >= near (no full-frame spike, no hole).
        let straddle = [[-2.0, -2.0, 0.01], [2.0, -2.0, 0.5], [0.0, 2.0, 0.5]];
        let out = clip_near(&straddle, 0.08);
        assert!(!out.is_empty(), "clipped polygon must not vanish");
        for v in &out {
            assert!(
                v[2] >= 0.08 - 1e-5,
                "vertex behind near plane after clip: {:?}",
                v
            );
        }
    }

    #[test]
    fn clip_near_passes_geometry_in_front() {
        let front = [[-1.0, -1.0, 2.0], [1.0, -1.0, 2.0], [0.0, 1.0, 2.0]];
        let out = clip_near(&front, 0.08);
        assert_eq!(out.len(), 3);
    }

    // ---- Phase 6: camera / shot legibility rules ----

    #[test]
    fn shot_legibility_rejects_bad_framing() {
        // off-frame
        assert!(evaluate_shot_legibility(false, 0.5, 0.0, 0.3).0);
        // too small
        assert!(evaluate_shot_legibility(true, 0.10, 0.0, 0.3).0);
        // occluded > 30%
        assert!(evaluate_shot_legibility(true, 0.5, 0.6, 0.3).0);
        // set dominates > 65%
        assert!(evaluate_shot_legibility(true, 0.5, 0.0, 0.8).0);
        // good shot passes
        assert!(!evaluate_shot_legibility(true, 0.5, 0.1, 0.3).0);
    }

    // ---- Phase 3/5: full software renderer produces sane frames ----

    fn sample_world_with_char() -> (WorldState, HashMap<String, HumanoidRig>, FrameState) {
        let world = build_default_world();
        let mut rigs = HashMap::new();
        rigs.insert(
            "mara".to_string(),
            HumanoidRig::default_humanoid("mara", "en-us", [0.8, 0.3, 0.3]),
        );
        let state = FrameState {
            chars: vec![(
                CharFrame {
                    id: "mara".into(),
                    root: Xform {
                        pos: [0.0, 0.0, 0.0],
                        rot: [0.0, 0.0, 0.0],
                    },
                    state: PerformanceState::Idle,
                    walk_phase: 0.0,
                    speaking: false,
                    action_local_time: 0.0,
                    action_weight: 0.0,
                    focus_target: None,
                },
                Pose::default(),
            )],
            camera_eye: [0.0, 1.6, 3.5],
            camera_look: [0.0, 1.0, 0.0],
            props: vec![],
            flicker: false,
            elevator_open: 0.0,
            elevator_indicator: None,
            panel_active: 0.0,
            impossible_reveal: 0.0,
        };
        (world, rigs, state)
    }

    #[test]
    fn renderer_pipeline_no_nan_no_spikes() {
        let (world, rigs, state) = sample_world_with_char();
        let r = StageRenderer::new(240, 420);
        let buf = r.render_buffers(&state, &rigs, &world, None);
        assert_eq!(buf.stats.non_finite, 0, "no non-finite projections");
        assert_eq!(
            buf.stats.behind_camera, 0,
            "no whole triangles behind camera"
        );
        assert_eq!(
            buf.stats.implausible_character_triangles, 0,
            "no character triangle may fill >60% of the frame (spike/shard guard)"
        );
        let set = buf.id.iter().filter(|&&id| id == 1).count();
        let chr = buf.id.iter().filter(|&&id| id >= 100).count();
        assert!(set > 500, "set (floor/walls) must be visible");
        assert!(chr > 200, "character must be rendered and in frame");
    }

    #[test]
    fn character_does_not_vanish_into_elevator() {
        // Place the performer at the elevator door and frame them from inside the
        // hall; even with the doors CLOSED they must remain visible (the rebuilt
        // elevator is openable and non-blocking).
        let world = build_default_world();
        let mut rigs = HashMap::new();
        rigs.insert(
            "mara".to_string(),
            HumanoidRig::default_humanoid("mara", "en-us", [0.8, 0.3, 0.3]),
        );
        let state = FrameState {
            chars: vec![(
                CharFrame {
                    id: "mara".into(),
                    root: Xform {
                        pos: [3.0, 0.0, -1.0],
                        rot: [0.0, 0.0, 0.0],
                    },
                    state: PerformanceState::Idle,
                    walk_phase: 0.0,
                    speaking: false,
                    action_local_time: 0.0,
                    action_weight: 0.0,
                    focus_target: None,
                },
                Pose::default(),
            )],
            camera_eye: [3.0, 1.6, 1.6],
            camera_look: [3.0, 1.0, -1.0],
            props: vec![],
            flicker: false,
            elevator_open: 0.0,
            elevator_indicator: None,
            panel_active: 0.0,
            impossible_reveal: 0.0,
        };
        let analysis = analyze_frame(&state, &rigs, &world, 240, 420, "mara");
        assert!(analysis.in_frame, "performer at elevator must be in frame");
        assert!(
            analysis.occlusion < 0.30,
            "closed elevator must not occlude the performer (occ={:.2})",
            analysis.occlusion
        );
    }

    #[test]
    fn elevator_open_changes_geometry() {
        let (world, _, _) = sample_world_with_char();
        let mut closed = Vec::new();
        let mut open = Vec::new();
        push_elevator(&mut closed, &world, 0.0);
        push_elevator(&mut open, &world, 1.0);
        let closed_door_vertices = closed
            .iter()
            .filter(|triangle| triangle.id == 2)
            .flat_map(|triangle| triangle.v)
            .collect::<Vec<_>>();
        let open_door_vertices = open
            .iter()
            .filter(|triangle| triangle.id == 2)
            .flat_map(|triangle| triangle.v)
            .collect::<Vec<_>>();
        assert_ne!(
            closed_door_vertices, open_door_vertices,
            "elevator doors must move when opened"
        );
    }

    // ---- Phase 4: rig hierarchy + articulation ----

    #[test]
    fn rig_hierarchy_head_above_feet_and_connected() {
        let rig = HumanoidRig::default_humanoid("mara", "en-us", [0.8, 0.3, 0.3]);
        let wm = rig.world_matrices(
            &Xform {
                pos: [0.0, 0.0, 0.0],
                rot: [0.0, 0.0, 0.0],
            },
            &Pose::default(),
        );
        let head_y = wm.get(&SemanticJoint::Head).unwrap().pos[1];
        let chest_y = wm.get(&SemanticJoint::Chest).unwrap().pos[1];
        let pelvis_y = wm.get(&SemanticJoint::Pelvis).unwrap().pos[1];
        assert!(
            head_y > chest_y && chest_y > pelvis_y,
            "head above chest above pelvis"
        );
        // Exclude the Root joint (placed at the character origin y=0) and only
        // consider the actual body parts: the lowest point of any part (center
        // minus its half-height) must sit near the floor.
        let lowest = rig
            .parts
            .iter()
            .map(|p| {
                let w = wm.get(&p.joint).unwrap();
                w.pos[1] - p.half[1]
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            lowest > 0.0 && lowest < 0.20,
            "feet must sit near the floor (y={lowest:.3})"
        );
        for p in &rig.parts {
            // The Pelvis hangs ~0.92 m above the Root (ground anchor); that long
            // link is the leg length by design, not a detachment. We verify that
            // every *body* part attaches to its neighbouring body part.
            if p.parent == SemanticJoint::Root {
                continue;
            }
            let cp = wm.get(&p.joint).unwrap().pos;
            let pp = wm.get(&p.parent).map(|w| w.pos).unwrap_or([0.0; 3]);
            let d = ((cp[0] - pp[0]).powi(2) + (cp[1] - pp[1]).powi(2) + (cp[2] - pp[2]).powi(2))
                .sqrt();
            assert!(
                d < 0.8,
                "part {:?} detached from parent (d={d:.2})",
                p.joint
            );
        }
    }

    #[test]
    fn rig_articulation_states_differ() {
        let rig = HumanoidRig::default_humanoid("mara", "en-us", [0.8, 0.3, 0.3]);
        let idle = character_pose(PerformanceState::Idle, 1.0, 0.0);
        let gesture = character_pose(PerformanceState::Gesture, 1.0, 0.0);
        let react = character_pose(PerformanceState::React, 1.0, 0.0);
        let hand_idle = rig
            .world_matrices(
                &Xform {
                    pos: [0.0; 3],
                    rot: [0.0; 3],
                },
                &idle,
            )
            .get(&SemanticJoint::LeftHand)
            .unwrap()
            .pos;
        let hand_gesture = rig
            .world_matrices(
                &Xform {
                    pos: [0.0; 3],
                    rot: [0.0; 3],
                },
                &gesture,
            )
            .get(&SemanticJoint::LeftHand)
            .unwrap()
            .pos;
        let head_idle = rig
            .world_matrices(
                &Xform {
                    pos: [0.0; 3],
                    rot: [0.0; 3],
                },
                &idle,
            )
            .get(&SemanticJoint::Head)
            .unwrap()
            .pos;
        let head_react = rig
            .world_matrices(
                &Xform {
                    pos: [0.0; 3],
                    rot: [0.0; 3],
                },
                &react,
            )
            .get(&SemanticJoint::Head)
            .unwrap()
            .pos;
        let d_hand = ((hand_idle[0] - hand_gesture[0]).powi(2)
            + (hand_idle[1] - hand_gesture[1]).powi(2)
            + (hand_idle[2] - hand_gesture[2]).powi(2))
        .sqrt();
        let d_head = ((head_idle[0] - head_react[0]).powi(2)
            + (head_idle[1] - head_react[1]).powi(2)
            + (head_idle[2] - head_react[2]).powi(2))
        .sqrt();
        assert!(
            d_hand > 0.05,
            "gesture must move the arm/hand (d={d_hand:.3})"
        );
        assert!(
            d_head > 0.01,
            "react must move the head/torso (d={d_head:.3})"
        );
    }

    #[test]
    fn locomotion_translates_character_root() {
        let world = build_default_world();
        let rigs = build_rigs(&world);
        let sched = Schedule {
            duration: 3.0,
            characters: vec![CharTrack {
                id: "mara".into(),
                home: [0.0, 0.0, 0.0],
                actions: vec![ScheduledAction {
                    actor: "mara".into(),
                    action: "move_to".into(),
                    target: Some("apt_3b_door".into()),
                    text: None,
                    start: 0.0,
                    dur: 3.0,
                    performance: None,
                }],
            }],
            camera_shots: vec![],
            dialogue: vec![],
            captions: vec![],
            events: vec![],
            flicker: vec![],
            prop_attach: vec![],
            inserts: vec![],
            environment: vec![],
            sounds: vec![],
        };
        let a = evaluate_at(&sched, &rigs, &world, 0.0);
        let b = evaluate_at(&sched, &rigs, &world, 1.5);
        let pa = a.chars[0].0.root.pos;
        let pb = b.chars[0].0.root.pos;
        let d = ((pa[0] - pb[0]).powi(2) + (pa[2] - pb[2]).powi(2)).sqrt();
        assert!(
            d > 0.2,
            "move action must translate the character (d={d:.2})"
        );
    }

    #[test]
    fn camera_never_produces_spike_triangles_on_performer_at_elevator() {
        // Regression: a performer standing at the elevator door, framed by a
        // close shot while facing *away* from the hall, used to push the camera
        // inside the performer's near plane (clamp_camera_to_hallway parked it at
        // z=-0.6, only ~0.4 m in front), producing full-frame shard triangles.
        // The minimum camera-to-subject distance must prevent that.
        let world = build_default_world();
        let rigs = build_rigs(&world);
        let sched = Schedule {
            duration: 4.0,
            characters: vec![CharTrack {
                id: "mara".into(),
                home: [3.0, 0.0, -1.0],
                actions: vec![ScheduledAction {
                    actor: "mara".into(),
                    action: "turn_toward".into(),
                    target: Some("hall_center".into()),
                    text: None,
                    start: 0.0,
                    dur: 4.0,
                    performance: None,
                }],
            }],
            camera_shots: vec![CameraShotSpec {
                start: 0.0,
                end: 4.0,
                intent: "closeup".into(),
                subject: "mara".into(),
                reaction: None,
            }],
            dialogue: vec![],
            captions: vec![],
            events: vec![],
            flicker: vec![],
            prop_attach: vec![],
            inserts: vec![],
            environment: vec![],
            sounds: vec![],
        };
        let r = StageRenderer::new(200, 356);
        for i in 0..8 {
            let t = i as f32 * 0.5;
            let state = evaluate_at(&sched, &rigs, &world, t);
            let buf = r.render_buffers(&state, &rigs, &world, None);
            assert_eq!(
                buf.stats.implausible_character_triangles, 0,
                "t={t}: a character triangle filled >60% of the frame (spike/shard)"
            );
            assert_eq!(buf.stats.non_finite, 0, "t={t}: non-finite projection");
        }
    }

    #[test]
    fn off_camera_character_does_not_corrupt_frame() {
        // A character straddling the camera plane (partly in front, partly
        // behind) used to be clipped only at the near plane, leaving a vertex
        // with an enormous projected coordinate that painted a full-frame shard.
        // Frustum side-plane clipping must bound it.
        let world = build_default_world();
        let rigs = build_rigs(&world);
        let state = FrameState {
            chars: vec![
                (
                    CharFrame {
                        id: "mara".into(),
                        root: Xform {
                            pos: [0.0, 0.0, 2.0],
                            rot: [0.0, 0.0, 0.0],
                        },
                        state: PerformanceState::Idle,
                        walk_phase: 0.0,
                        speaking: false,
                        action_local_time: 0.0,
                        action_weight: 0.0,
                        focus_target: None,
                    },
                    Pose::default(),
                ),
                (
                    CharFrame {
                        // Deliberately *just in front of the lens* (root z = 0.10,
                        // only ~0.02 m beyond the near plane). Before frustum side
                        // clipping this produced a vertex with vz ~ 0.1 and a large
                        // vy, i.e. ndc_y ~ fproj*vy/vz -> an enormous projected
                        // coordinate: a full-frame shard. After side clipping the
                        // triangle is bounded to the frustum and contributes only a
                        // thin sliver, never a shard.
                        id: "ellis".into(),
                        root: Xform {
                            pos: [0.0, 0.0, 0.1],
                            rot: [0.0, 0.0, 0.0],
                        },
                        state: PerformanceState::Idle,
                        walk_phase: 0.0,
                        speaking: false,
                        action_local_time: 0.0,
                        action_weight: 0.0,
                        focus_target: None,
                    },
                    Pose::default(),
                ),
            ],
            camera_eye: [0.0, 1.6, 0.0],
            camera_look: [0.0, 1.0, 2.0],
            props: vec![],
            flicker: false,
            elevator_open: 0.0,
            elevator_indicator: None,
            panel_active: 0.0,
            impossible_reveal: 0.0,
        };
        let r = StageRenderer::new(240, 420);
        let buf = r.render_buffers(&state, &rigs, &world, None);
        assert_eq!(
            buf.stats.implausible_character_triangles, 0,
            "character at the lens must not produce shard triangles"
        );
        assert_eq!(buf.stats.non_finite, 0);
        // Character ids are assigned from a HashMap (nondeterministic order), so we
        // count all character pixels (id >= 100) rather than assuming a fixed id.
        let char_px = buf.id.iter().filter(|&&id| id >= 100).count();
        assert!(char_px > 100, "on-camera character must be visible");
        // `implausible_character_triangles == 0` already guarantees no single
        // triangle covers >60% of the frame, i.e. the lens character cannot paint
        // a full-frame shard.
    }
}
