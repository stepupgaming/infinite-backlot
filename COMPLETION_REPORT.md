# Infinite Backlot — First Autonomous Episode: Completion Report

**Date:** 2026-07-12
**Goal:** Convert the scaffold into a first *watchable* autonomous episode: planned (LLM),
executed in the Bevy world, performed by animated humanoids, voiced with real generated
speech (≥2 distinguishable voices), covered by an autonomous camera director, captured into
real frames, muxed with synchronized audio, captioned (burned-in, vertical-safe), encoded to
a vertical 9:16 MP4, packaged with diagnostics proving authorship + rendering — with no
silent stubs.

---

## 1. Headline result (verified, not estimated)

A real, playable **vertical 1080×1920 MP4** was produced and machine-verified:

| Property | Value | How verified |
|---|---|---|
| Duration | **56.7 s** (target 45–60 s) | `ffprobe` + `episode.json` |
| Resolution | **1080×1920** (9:16) | `ffprobe` `width=1080 height=1920` |
| Video codec | h264 (yuv420p) | `ffprobe` |
| Audio codec | **aac, 44100 Hz** | `ffprobe` |
| Frames captured | 1700 PNGs @ 30 fps | `render_manifest.json` (`frames_captured: true`) |
| Captions | **burned-in** drawtext, 8 lines, vertical-safe (y≈1760/1920) | `ffmpeg_command` in `diagnostics.json` |
| Speech | **real espeak-ng TTS** (`tts_real: true`) | `diagnostics.json`, WAV files in `audio/` |
| Distinguishable voices | **2** (en-us: mara/voss; en-gb: ellis/nox) | `voice_map`, `dialogue.json` |
| mp4 produced / ffprobe ok | `true` / `true` | `diagnostics.json` |

Canonical artifact:
`output/episodes/episode_000001/output/vertical_captioned.mp4` (1,192,966 bytes)
plus `vertical_clean.mp4` (no captions).

---

## 2. The 12 PRD requirements, mapped to reality

1. **Planned by configured LLM (Gemma).** The `LlmAuthor` is fully implemented against the
   OpenAI-compatible `/v1/chat/completions` + JSON-schema structured output. **In this
   environment the configured endpoint (`http://localhost:1234/v1`, `gemma-4-26b-a3b`) only
   serves an *embeddings* model**, so it cannot return a plan. The episode was therefore
   produced by the **deterministic fallback** path — and is **truthfully labeled**
   `plan_author_source: "deterministic_fallback"`, `llm_used: false`. No "llm" claim is made.
   → See §4 for how to make the LLM path actually run.
2. **Executed in the Bevy world.** `produce_episode` builds a `WorldState`, validates the plan,
   schedules beats, and evaluates per-frame state. The live Bevy app (`backlot.exe` interactive
   window) spawns the same world via ECS.
3. **Animated humanoid performers.** Replaced capsule assumptions with a `HumanoidRig`
   (SMPL-X-compatible semantic-joint contract). The offline renderer (`StageRenderer`) rasterizes
   each rig part per frame; the live Bevy scene spawns one `CharacterAvatar` parent + child box
   meshes per `RigPart`. Motion is at the **whole-character transform** level (navigate, face,
   interact) — not per-joint skeletal animation. (Honest limitation, §5.)
4. **Real generated speech, ≥2 voices.** `EspeakTts` (espeak-ng 1.52) synthesizes real WAVs;
   `voice_map` maps characters to `en-us`/`en-gb`. Durations are **measured from the WAV header**
   (not estimated) — a real bit-depth bug was found and fixed (§6).
5. **Autonomous camera director.** `CameraDirector` proposes shots; `produce_episode` builds a
   `camera_plan.json` (5 shots) and drives camera position/look-at per frame.
6. **Captured into real video frames.** An offline pure-Rust z-buffer software rasterizer
   (flat shading) writes the 1700 PNG frames — deliberately avoiding GPU readback fragility.
7. **Synchronized audio.** Measured speech + 1 s padding per line → `final_mix.wav`, muxed with
   frames via ffmpeg `-shortest`.
8. **Burned-in vertical-safe captions.** `drawtext` filter with this build's quirks handled
   (relative no-colon font path, `=` for first option, escaped `:` inside text, two-char `\n`
   line breaks, no escaped commas in `between(t,a,b)`).
9. **Encoded to MP4.** `libx264` + `aac`, `scale=1080:1920,format=yuv420p`.
10. **Episode package + diagnostics proving authorship/rendering.** `episode_000001/` contains
    `plan.json`, `dialogue.json`, `camera_plan.json`, `captions.json`, `events.jsonl`,
    `world_before/after.json`, `render_manifest.json`, `diagnostics.json`, `report.md`,
    `gemmy_manifest.json`, `llm/*.json(l)`, and `output/*.mp4`.
11. **REQUIRE-LLM mode fails clearly, no silent fallback.** With `--require-llm` (or
    `require_llm=true`), `LlmAuthor::new` propagates the flag; an unreachable/invalid model
    returns `Err` and `main` exits `1` **without producing a video**. Unit proof:
    `backlot-llm::require_llm_fails_clearly_without_silent_fallback` **passes**.
12. **Truthful author attribution.** `PlanAuthorship` records `plan_source` and a per-beat
    `source` (`Llm` / `DeterministicFallback` / `Deterministic`). The canonical episode records
    `DeterministicFallback` for plan **and** every beat. Test
    `deterministic_authorship_is_truthful` asserts we never mislabel fallback as `Llm`.

---

## 3. Tests (all passing)

```
cargo test -p backlot-core      -> critical_boundaries: 9 passed / 1 ignored
                                    director_test:       2 passed
                                    pipeline_test:       3 passed
cargo test -p backlot-llm       -> require_llm:         1 passed
```

Focused boundary tests cover: unique semantic joints, unique rig-part joint keys, deterministic
plan validation, rejection of unknown action/actor/empty-speak, **truthful fallback authorship**,
**measured WAV duration**, and **honest TTS downgrade** when espeak is unreachable.
(`offline_producer_yields_real_mp4` is `#[ignore]` — runs the full pipeline against real
espeak/ffmpeg; the live run below is its equivalent.)

---

## 4. How to make the LLM path actually run (exact setup)

The scaffold is wired for real LLM planning; only the endpoint is missing here. To get a
`plan_author_source: "Llm"` episode:

1. Run an OpenAI-compatible server that serves **chat/structured completions** for the model
   (e.g. LM Studio or `llama.cpp` with a Gemma chat model), exposed at an `/v1` base URL.
2. Point `data/config.toml`:
   ```toml
   [llm]
   base_url = "http://<host>:<port>/v1"
   model    = "gemma-4-26b-a3b"   # any chat-capable model your server serves
   ```
3. Optional, to *require* it (fails loudly if down):
   ```toml
   [director]
   require_llm = true
   ```
   or pass `--require-llm` on the command line.
4. Produce:
   ```bash
   cargo run -p backlot-app -- --produce-one
   ```
   On success the package's `diagnostics.json` will show `plan_author_source: "Llm"` and
   `llm_used: true`; on failure (require mode) it exits `1` with **no** MP4.

If you leave `require_llm = false`, the system transparently uses the deterministic fallback
and labels it as such — exactly what the canonical artifact does.

---

## 5. Honest limitations / what is NOT claimed

- **No "LLM-authored" episode was produced here** because the only available endpoint serves
  embeddings, not chat. This is reported as a blocked external dependency, not papered over.
  The deterministic plan is real, executable, and honestly attributed.
- **Humanoids are blocky box avatars** built from the `HumanoidRig` contract (SMPL-X-compatible
  *interface*), not a downloaded SMPL-X mesh asset. No external model binaries were fetched.
- **Animation is whole-body** (position/orientation/interaction per beat), not per-joint skeletal
  posing. Per-joint animation was intentionally deferred to avoid transform conflicts with the
  navigation system.
- **Faces/eyes/lipsync** are not rendered; captions carry the dialogue. Speech timing is exact
  (measured WAV durations), so captions and audio are synchronized.

---

## 6. Notable fixes made this session

- **WAV duration was 2× too long.** `wav_duration_secs` read bit-depth at the wrong fmt-chunk
  offset, so 16-bit speech measured as double length (the canonical 65.6 s run had ~33 s of
  silence). Fixed to read `block_align` and compute `bytes_per_sample` correctly. This is now a
  passing unit test (`wav_duration_is_measured_from_header`).
- **Require-LLM silent fallback (critical).** The CLI flag was not reaching `LlmAuthor`, so
  require mode "succeeded" with a fallback plan. Fixed by propagating `dir.require_llm` into
  `LlmAuthor::new`; require mode now fails clearly (exit 1, no video).
- **ffmpeg `drawtext` on this build.** Multiple parser quirks resolved (see §2.8).

---

## 7. Reproduce

```bash
cd C:/Projects/bevy-infinite
cargo build -p backlot-app
cargo run -p backlot-app -- --produce-one        # canonical episode (deterministic fallback)
# or, to demand the LLM and fail loudly if it's unreachable:
cargo run -p backlot-app -- --produce-one --require-llm
# verify:
ffprobe -v error -show_entries stream=codec_type,width,height,codec_name \
  output/episodes/episode_000001/output/vertical_captioned.mp4
```

Outputs: `output/episodes/episode_000001/output/vertical_captioned.mp4` (+ `vertical_clean.mp4`)
and the full provenance package described in §2.10.
