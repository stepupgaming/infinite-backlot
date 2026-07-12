# Infinite Backlot

A local-first, autonomous 3D content world. A showrunner "director" (an
OpenAI-compatible LLM, or a built-in deterministic fallback) continuously
authors **watchable, short-form narrative episodes** from a persistent 3D
world rendered in [Bevy](https://bevyengine.org) 0.19. Each episode is
validated, rehearsed, rendered, and committed as a machine-readable package
that can be replayed, inspected, or exported.

This repository is the **runnable foundation** described in the PRD's
*Content Proof* (Phase 0) and the core of the *Episode Engine* (Phase 1):

- An OpenAI-compatible LLM client (`/v1/chat/completions`, structured output).
- A bounded, schema-validated protocol so the model can never emit an
  instruction the world cannot execute.
- A deterministic director fallback so the product **runs with zero external
  services**.
- A greybox Bevy world with 4 characters, a camera rig, and a watchability
  governor.
- A committed episode package (JSONL events, dialogue, captions, camera plan,
  world deltas, diagnostics, a Gemmy-style manifest, and a human report).

---

## Architecture

A Cargo workspace with three crates:

```
backlot-core/    protocol, world model, validation, deterministic director,
                 story application, episode packaging, seeded RNG, config
backlot-llm/     OpenAI-compatible async client + LLM-backed episode author
                 (graceful per-piece fallback to the deterministic director)
backlot-app/     Bevy 0.19 application: state machine, greybox scene,
                 navigation, beat execution, camera direction, captions,
                 watchability governor, operator HUD, commit + replay
```

### Bounded agency (safety by construction)

The LLM decides **narrative** only. Every response is parsed, schema-validated,
semantically validated, and capability-checked before a single byte reaches the
world (`backlot-core/src/validation.rs`). The vocabulary of allowed actions
(`KNOWN_ACTIONS`), camera intents (`KNOWN_CAMERA_INTENTS`), and entities is
closed; invalid commands are rejected or deterministically repaired. The model
never moves geometry, allocates memory, or performs I/O outside the protocol.

### Episode pipeline

```
director.author(ctx)
   └─ structured EpisodePlan (beats) + per-beat BeatCommand
validate_plan(world, plan)  → ValidatedPlan
validate_beat_command(...)  → ResolvedBeat (ResolvedAction[])
   └─ committed to the Bevy player, which:
        • navigates characters (kinematic)
        • fires actions on a deterministic schedule
        • eases the camera rig toward per-beat intents
        • emits captions + logs events/dialogue
        • runs the watchability governor (forces a beat end on > max_dead_air)
apply_persistent_changes(world, plan.persistent_changes)  → world delta
EpisodePackage::write(output_dir)  → episodes/<id>/...
```

### State machine (`backlot-app/src/state.rs`)

`Boot → AssetLoading → Idle → EpisodeSelecting → EpisodePlanning →
PlanValidation → Rehearsing → EpisodeReady → Rendering → Committing →
Reviewing → (loop)`. The LLM request runs on a dedicated worker thread so the
Bevy main thread never blocks.

---

## Running

```bash
# build
cargo build --release

# run (open the operator window; press keys at the review screen)
cargo run -p backlot-app
```

Configuration lives in `data/config.toml` (auto-created defaults if missing).

### Operator controls (at the Review screen)

| Key | Action |
|-----|--------|
| `N` | next episode (auto-advances if `episodes_to_run > 0`) |
| `R` | replay the render pass of the last episode (no new LLM, no commit) |
| `Q` | quit |

Set `episodes_to_run` in `data/config.toml` to a positive number for an
unattended demo run (it auto-advances and exits when the count is reached).

---

## Configuring the LLM (OpenAI-compatible)

The LLM section is intentionally an OpenAI-compatible shape — point it at any
server that speaks `/v1/chat/completions`:

```toml
[llm]
base_url = "http://localhost:1234/v1"   # LM Studio / llama.cpp default
model    = "gemma-4-26b-a3b"            # or any chat model
api_key  = ""                           # local servers usually accept empty
timeout_secs = 120
temperature   = 0.4
max_tokens    = 2048
max_retries   = 2
stream        = false
```

Examples of compatible endpoints:

- **LM Studio** — `http://localhost:1234/v1`, model = the loaded model id.
- **Ollama** — `http://localhost:11434/v1`.
- **vLLM / OpenAI / OpenRouter / Together** — set `base_url`, `model`, and a real `api_key`.

Structured output is requested via `response_format` JSON schema (strict, with
a `json_object` retry). On any malformed, slow, or unavailable response the
author **falls back per piece** to the deterministic director, so the product is
always runnable.

- `force_fallback = true`  → never touch the network; always deterministic.
- `force_fallback = false` → attempt the real LLM first, fall back on failure.

The deterministic fallback is also the safety net if no model is loaded.

---

## Committed episode package

Each episode is written to `<output_dir>/episodes/<id>/`:

| File | Purpose |
|------|---------|
| `episode.json` | id / title / logline / duration |
| `plan.json` | the full structured `EpisodePlan` |
| `world_before.json`, `world_after.json` | canonical world before/after |
| `events.jsonl` | timed, typed events (move, speak, flicker, …) |
| `dialogue.json` | spoken lines with voice ids + timings |
| `captions.json` | caption cues for burn-in / subtitles |
| `camera_plan.json` | per-shot camera intent + transform |
| `render_manifest.json` | output asset paths (mp4 / thumbnail) |
| `diagnostics.json` | metrics, LLM request/failure counts, repairs |
| `gemmy_manifest.json` | downstream export manifest |
| `report.md` | human-readable quality report |

---

## What is stubbed / next

This foundation focuses on the autonomous *engine*. Intentionally left for later
phases (and clearly separated so they can be dropped in):

- **Real TTS audio** — `backlot-core/src/tts.rs` defines a `Tts` trait with an
  `EstimatingTts` that predicts durations; swap in a real synthesizer to also
  write `audio/*.wav`.
- **Caption burn-in / MP4** — `captions.json` + `camera_plan.json` are the
  inputs; an ffmpeg/encoder pass turns them into `vertical_captioned.mp4`,
  `vertical_clean.mp4`, `horizontal_clean.mp4`, and a thumbnail.
- **On-screen captions in the 3D view** — captions are currently data + an
  operator HUD bar; rendering burned subtitles onto the rendered frames is a
  later pass.
- **GPU frame capture** — `capture_frames` in config gating; currently the
  window is the operator view.

## Tests

```bash
cargo test
```

`backlot-core` includes tests for the deterministic director (valid, executable,
reproducible) and a headless end-to-end pipeline test that authors, validates,
applies persistent changes, and writes a full episode package to disk.
