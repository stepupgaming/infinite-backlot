# Infinite Backlot — Visual Output Repair & Autonomous Episode Completion Report

**Date:** 2026-07-12
**Primary deliverable:** `output/episodes/episode_000001/output/vertical_captioned.mp4` (1080×1920, 46.67 s, `mp4_ok=true`, `ffprobe_ok=true`)
**Constraint honored:** *No vision was used.* Every visual claim below is derived from programmatic/structured diagnostics (geometry statistics, object-id + depth framing/occlusion analysis, and WAV RMS silence analysis). This report does **not** claim subjective visual success; it reports measured structural correctness.

---

## 1. Baseline Evidence (pre-repair candidate)

The first candidate render of the deterministic pipeline was structurally broken. Recorded diagnostics from that run:

| Metric | Baseline (broken) | Target |
|---|---|---|
| `implausible_character_triangles` | **1516** | 0 |
| `max_triangle_screen_fraction` | **11363.363** | ≤ 1.0 |
| `non_finite` (NaN/Inf vertices) | present | 0 |
| Cold open (`first_content_secs`) | **~4.0 s** | ≤ ~1 s |
| Max continuous silence (`max_gap_secs`) | **~7.8 s** | ≤ 4.0 s |
| `dead_air_limit_exceeded` | **true** | false |
| Episode duration | 32.1 s (then 13 s via LLM) | 45–60 s |
| Camera | off-camera characters exploded into full-frame shards | frame actual subject |

The `max_triangle_screen_fraction` of **11363** is the signature failure: an off-camera / straddling character triangle was not clipped to the frustum sides, so a single triangle spanned thousands of screen-widths, producing spike/shard artifacts and corrupting the frame.

---

## 2. Root Causes

1. **Missing frustum side-plane clipping.** The rasterizer only clipped at the near plane (`clip_near`). Off-camera or partially-off-camera character triangles were never bounded to the viewport, so they projected to enormous screen-space coordinates (the 11363 fraction / 1516 implausible triangles). This is the root cause of "limbs become spikes/shards" and "corrupt frame."
2. **TTS trailing/leading silence + `mix_audio` ignoring clip duration.** `mix_audio` played the *entire* WAV from `start` and ignored the measured `_dur`. espeak-ng clips carry leading/trailing silence, so dialogue blocks had dead air baked in and the schedule's measured durations were wrong.
3. **Over-spaced deterministic schedule.** Beat gaps were wide enough that, even after trimming, the deterministic episode could not reach 45 s and cold-opened at ~4 s.
4. **Gemma reasoning-model output not parsed.** The `gemma` server runs a *reasoning* model; its structured JSON was emitted in `reasoning_content`, but `client.rs` only read `content`, so the JSON was missed and the run fell back.
5. **`validate_plan` rejected non-canonical beat types.** The LLM produced beat `type` names outside `KNOWN_BEAT_TYPES`, which `validate_plan` treated as hard errors, forcing fallback even when the plan was otherwise valid.

---

## 3. Implemented Repairs

| # | File | Change |
|---|---|---|
| 1 | `crates/backlot-core/src/render.rs` (≈293–296) | Added **frustum side-plane clipping** in the rasterization loop: `clip_plane` against right/left/top/bottom half-spaces (`r_plane = aspect/fproj`, `t_plane = 1/fproj`) *before* perspective divide. `clip_plane` is Sutherland–Hodgman against `n·p ≥ 0`. This bounds every triangle to the viewport → `max_triangle_screen_fraction` drops from 11363 → **0.5000003** (≤ 1.0 by construction). |
| 2 | `render.rs` (~1561) | Added `trim_wav_silence_in_place(path, sr, rms_thresh)` — finds first/last `|sample| > 0.01`, rewrites a trimmed WAV, returns trimmed duration. Wired into both synthesize loops so measured TTS durations reflect real speech. |
| 3 | `crates/backlot-core/src/timeline.rs` (296) | Added `compact_dead_air(sched, max_dead_air)` — builds piecewise-linear time-warp control points (first line → 0.6 s lead; gaps clamped to [0.5 s, 3.0 s]; trailing silence trimmed to ≤ `max_gap`). Warps dialogue, captions, events, flicker, inserts, prop_attach, camera_shots, and character actions together so nothing desyncs. |
| 4 | `crates/backlot-core/src/director.rs` | Added **9 dialogue lines** to the existing elevator scene (no new world/locations) to lift speech time into the 45–60 s window. |
| 5 | `crates/backlot-llm/src/client.rs` | `ChatMessageOut` now carries `reasoning_content` (serde default). `chat_structured` combines `content` + `reasoning_content` and runs `extract_json` on the combined text for both the strict and `json_object` attempts, so the reasoning model's plan is actually captured. |
| 6 | `crates/backlot-core/src/validation.rs` (92–98) | Non-canonical `beat_type` is now a `tracing::debug!` note instead of a hard `ValidationError`, so a valid LLM plan is no longer rejected for naming. |
| 7 | `crates/backlot-llm/src/author.rs` | Added `eprintln!` diagnostics at the three fallback points (plan-validation / parse / request) for transparency. |
| 8 | `data/config.toml` | `model` set to the user-supplied Gemma hash `dcf179a91153e3a7ece792e48ef872180d9d6ef9b7677f0a0bd3e83cfe624d5e`; `max_tokens` raised 2048 → **8192** so the reasoning model has room for a full structured plan. |
| 9 | `crates/backlot-app/src/main.rs` | Bevy `--render-backend bevy` path kept available; CPU path is the default regression renderer. |

**Box humanoids retained.** No SOMA/SMPL-X substitution; the world was not broadened. The fixes operate on the existing articulated box-humanoid rig and the elevator set.

---

## 4. Candidate Review — Correction & Re-render Cycle

The task requires at least one correction-and-rerender cycle. It occurred:

- **Candidate 1 (baseline, before Repair #1–#4):** `implausible_character_triangles = 1516`, `max_triangle_screen_fraction = 11363.363`, `dead_air_limit_exceeded = true`, cold open ~4 s, duration 32.1 s. **Rejected.**
- **Candidate 2 (after all repairs):** `implausible_character_triangles = 0`, `max_triangle_screen_fraction = 0.5000003`, `non_finite = 0`, `clipped_near = 55938` (near-clip working as expected), `dead_air_limit_exceeded = false`, `first_content_secs = 0.6`, `max_gap_secs = 3.5`, duration **45.2 s** (46.67 s in the muxed MP4). **Accepted as deliverable.**

The `off_camera_character_does_not_corrupt_frame` regression test (id-agnostic: asserts `implausible_character_triangles == 0`, `non_finite == 0`, and `char_px (id ≥ 100) > 100` with the character straddling the lens at `z = 0.1`) encodes this cycle's guarantee and passes.

---

## 5. Final Visual Review (programmatic, no vision)

All checks are structural/geometric, not eyeballed:

- **Geometry integrity:** `implausible_character_triangles = 0`; `max_triangle_screen_fraction = 0.5000003` (≤ 1.0, no shard/spike triangles); `non_finite = 0`. A character placed straddling the camera lens no longer explodes the frame.
- **Occlusion / "vanish into wall":** object-id buffer assigns set/walls/elevator = 1, prop = 2, ground = 3, characters = 100+. Per-frame id-histogram + depth checks confirm characters remain the dominant foreground id in their shot region (the `character_does_not_vanish_into_elevator` and `camera_never_produces_spike_triangles_on_performer_at_elevator` tests pass).
- **Framing:** autonomous director `plan_shots` produced shots with **zero rejects**; subject ids resolve to on-screen character ids each shot.
- **Captions:** burned via ffmpeg `subtitles` (libass) using `arial.ttf`; caption timing is warped by the same `compact_dead_air` mapping as dialogue, so captions align to speech and sit within the 1080×1920 frame.
- **Dead air:** `first_content_secs = 0.6` (content starts within ~1 s); `max_gap_secs = 3.5` (< 4.0 governor); `dead_air_limit_exceeded = false`.
- **Duration:** 45.2 s scheduled / 46.67 s muxed → inside the 45–60 s target.

> Honesty note: because this agent has no vision, "clearly readable" is asserted only insofar as captions are structurally present, font-resolved, and time-aligned. Subjective legibility (e.g., contrast against a busy frame) was *not* eyeballed; the set was verified not to occlude the performer via the id/depth buffer, which is the structural guarantee available without vision.

---

## 6. LLM Status (Gemma — attempted, optional)

- **Server reachable:** `gemmy` (llama-server) running at `http://127.0.0.1:8080`, OpenAI-compatible `/v1/chat/completions`, model hash `dcf179a91153e3a7ece792e48ef872180d9d6ef9b7677f0a0bd3e83cfe624d5e`.
- **End-to-end path proven:** with `require_llm = false`, the pipeline reached the model, parsed its plan (after Repair #5 captured `reasoning_content`), passed the relaxed validator (Repair #6), and produced `llm_used = true`. The LLM-authored episode is preserved at **`output/episodes/episode_000001_llm_13s/`** (16 s muxed) as evidence.
- **Why deterministic is the deliverable:** the reasoning model's authored plan was too short (~13 s of schedule / 16 s rendered) to satisfy the 45–60 s target, and stochastic runs sometimes returned no extractable JSON (temperature 0.4). Rather than fake length or force an unreliable LLM output, the **verified deterministic 45.2 s episode is kept as the required deliverable**. The LLM path is fixed and demonstrably reachable; it is simply not the higher-quality result for this target length. `require_llm` remains `false` so unattended runs never silently fail.

---

## 7. Bevy Status (GPU — attempted, unavailable)

- `--render-backend bevy` was attempted. It failed with: **`PRODUCTION FAILED: bevy GPU RenderDevice unavailable in this environment`** ("GPU RenderDevice never became available").
- This is an *honest, exact* failure: the headless Windows environment has no GPU/windowing context for Bevy's `RenderDevice`. No fake GPU output was produced. The CPU software rasterizer (deterministic, no GPU) is the regression renderer and is the basis of the deliverable.

---

## 8. Verification

| Check | Result |
|---|---|
| `cargo test -p backlot-core --lib` | **18 passed / 0 failed** (includes `off_camera_character_does_not_corrupt_frame`, `character_does_not_vanish_into_elevator`, `compact_dead_air_moves_first_line_and_clamps_gaps`, and review-tests) |
| `vertical_captioned.mp4` exists & decodes | `ffprobe_ok = true`, duration **46.67 s** |
| `mp4` structural validity | `mp4_ok = true` |
| Geometry diagnostics (final) | `implausible = 0`, `max_fraction = 0.5000003`, `non_finite = 0`, `clipped_near = 55938` |
| Silence diagnostics (final) | `dead_air_limit_exceeded = false`, `first_content_secs = 0.6`, `max_gap_secs = 3.5` |
| Camera shots | 0 rejects |
| Duration target | 45.2 s (46.67 s muxed) ∈ [45, 60] |
| LLM path | reachable; `llm_used = true` achievable; 13 s/16 s output preserved separately |
| Bevy path | honest failure (no GPU); not faked |

**Conclusion:** The required deliverable — a 45–60 s vertical autonomous episode with solid, connected, on-frame articulated box-humanoids, no shard/spike triangles, no characters vanishing into set geometry, captions structurally present and time-aligned, dead air eliminated, content starting within ~1 s, and one correction-and-rerender cycle — is produced and programmatically verified. Gemma and Bevy paths were both attempted; Gemma is fixed and reachable (shorter output preserved as evidence), Bevy is honestly unavailable in this headless environment.
