# Truthful Status Report — Infinite Backlot autonomous episode pipeline

Generated 2026-07-13. Author: ZCode agent (session handed off at user request).

## Bottom line
**GOAL NOT MET.** No successful 45–60s, vertical 9:16, LLM-authored, Bevy-GPU-rendered
MP4 was ever produced. A full `--produce-one --require-llm --render-backend bevy` run
**has never completed**. The longest run reached ~12 minutes and was still in the LLM
authoring phase when it was killed; it never rendered, never wrote an MP4, never
validated duration.

The agent CANNOT see images/video (hard constraint), so even a produced MP4 could not
be visually verified here — that requires the user or a vision-capable agent.

---

## What is PROVEN (real evidence on disk)

1. **async-openai SDK migration is done and works.**
   - Replaced hand-rolled reqwest with `async-openai` 0.41 SDK
     (`crates/backlot-llm/src/client.rs`).
   - Smoke-tested against the live local server: returns valid chat completions.
   - `max_tokens` was NOT reduced.

2. **Real Bevy GPU frame capture is proven.**
   - `examples/bevy_min.rs` captures 5/5 frames from the actual **NVIDIA RTX 5060 Ti**
     (Vulkan, driver 591.86), exits 0.
   - Evidence: `bevy_min.log`
     - line 113: `AdapterInfo { name: "NVIDIA GeForce RTX 5060 Ti", ... backend: Vulkan }`
     - line 117: `[bevy_min] RenderDevice ready=true after 0 updates`
     - line 123: `[bevy_min] CAPTURED 5/5 frames`
     - line 124: `[bevy_min] OK`
   - The fix that makes this work: `crates/backlot-app/src/bevy_capture.rs` now calls
     `app.finish(); app.cleanup();` before the capture loop and gates on the main-world
     `RenderDevice` (Bevy 0.19 readiness). A targeted `FallbackErrorHandler` tolerates
     the transient "Resource does not exist" startup error.

3. **Release binary builds** with all current changes (`cargo build --release -p backlot-app`).

---

## What is NOT proven (do not assume)

- A full `require-llm` Bevy production run has **never completed**.
- **The model's actual generated plan/beats have never been inspected.** Stdout was
  buffered to a file, so the only log output from the killed run was two startup lines.
  The agent was effectively blind during the run.
- No evidence exists that: an episode lands in 45–60s, ~1500 frames are captured,
  the MP4 muxes to vertical 9:16, captions are readable, performers look humanoid,
  or the set reads as a hallway/elevator.
- The structured-output schema-repair loop and duration-repair loop in `author.rs`
  (lines 160–383) have **never been observed running to completion**.

---

## Why it takes 12+ minutes — the real cause

This is NOT model speed. At 100+ tok/s, generation of a few-thousand-token response is
~20–80s. The cost is **call-count × single-slot server serialization**:

- The local llama-server is **single-slot**. Each chat completion occupies the slot for
  the entire generation; subsequent calls queue serially.
- Authoring makes **many sequential calls**: 1 plan call + **1 call per beat** (5–6
  beats) + a duration-repair loop that re-authors the **entire plan + all beats again**
  (`max_repairs`). At `max_repairs = 1` that is still ~12–14 serialized calls =
  ~12–19 min wall-clock. That is the 12 minutes observed.
- Mitigations applied (config `timeout_secs = 1800`, `max_repairs = 1`, stronger
  duration prompt demanding 5–6 beats) reduce but do **not remove** the serialization.
  The fundamental N-calls-on-one-slot design is unchanged.

---

## On "the validator is dog shit"

- `crates/backlot-core/src/validation.rs`:
  - `validate_plan` (line 74): checks active characters exist, primary location exists,
    beats non-empty, payoff non-empty, required entities exist. Sound structural checks.
  - `validate_beat_command` (line 536): checks known camera intents, known actions,
    actor is a known character, targets exist, speak actions have text. Sound.
- These are NOT the cause of the slowness. BUT they have **never been exercised
  end-to-end on a real completed run**, so claiming they "work" in practice is
  unsubstantiated.
- The model self-correction path (schema errors + duration feedback fed back to the
  model) exists in `author.rs` lines 160–383 and fails clearly (`Err`) in `require_llm`
  mode if the episode stays out of the 45–60s band after repairs. It was never seen
  completing.

---

## The agent's own failures (so the next agent is not misled)

1. Too slow to name **call-count × single-slot serialization** as the dominant time cost.
2. Ran a 12+ minute job with **no progress visibility** (buffered logs, no per-call
   tracing to a file). Blind the whole time.
3. Never achieved one clean end-to-end success, so it has no basis to claim the
   pipeline works.

---

## Recommended fixes for the next agent (in priority order)

1. **Collapse authoring into 1–2 calls.** One structured call that returns the whole
   episode (plan + all beat commands embedded) instead of plan + 1-per-beat. Cuts
   wall-clock from ~15 min to ~2–3 min. Keep schema/duration self-correction only as a
   bounded retry, not a full re-author of everything.
2. **Or run llama-server with multiple slots** (`--slots N`) so calls parallelize.
3. **Add per-call logging flushed to a file** (unbuffered stderr or explicit flush) so
   progress is visible during long runs.
4. **Keep the proven Bevy capture path** (`bevy_capture.rs` fix + `bevy_min` pattern).
   It demonstrably works on the real GPU.
5. **Verify actual model output**: dump the raw JSON the model returns and inspect it;
   do not trust the loop's "ok" status.
6. **Honor the persisted hard constraints** (below) when implementing.

---

## Persisted hard constraints (must not be violated by any fix)

- Final proof must run with LLM-required mode enabled. If Gemma fails after bounded
  repair attempts, FAIL CLEARLY rather than substituting a deterministic episode.
- The CPU software rasterizer may remain for tests/debug/headless regression only. It
  must NOT count as the final production renderer.
- Block-based cuboid characters may remain as debug actors only. They must NOT be the
  final visible performers.
- The final video must come from the ACTUAL Bevy scene.
- Do NOT pad a short episode with silence, static poses, slow camera, or meaningless
  walking.
- Do NOT claim visual quality merely because tests passed.
- Do NOT claim completion with another software-rasterized block-character episode.
- The agent cannot use vision; visual verification requires the user or a vision agent.

---

## Evidence file pointers

- `bevy_min.log` — proves real GPU capture (5/5 frames, RTX 5060 Ti).
- `bevy_prod.log` — partial production run (only 2 startup lines; buffered).
- `crates/backlot-app/src/bevy_capture.rs` — Bevy readiness/capture fix.
- `crates/backlot-llm/src/author.rs` — authoring + duration-repair loop (lines 160–383).
- `crates/backlot-llm/src/client.rs` — async-openai SDK client.
- `crates/backlot-core/src/validation.rs` — validators (lines 74, 536).
- `data/config.toml` — `timeout_secs = 1800`, `max_repairs = 1`, `target_duration_secs = 50`,
  `resolution = [1080, 1920]`, `frame_rate = 30`.
- `Cargo.lock` / `Cargo.toml` / `crates/backlot-llm/Cargo.toml` — dependency changes
  (async-openai, reqwest 0.13).
