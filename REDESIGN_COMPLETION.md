# Authoring Redesign — Completion Report

## What changed (evidence-based, per the accepted diagnostic)

The authoring stage was collapsed from the old **plan + per-beat + duration-restart**
graph into **one whole-episode structured call** (`AuthoredEpisode`) with at most one
**direction-aware** revision call. Root cause of the prior 83.3s failure
(`duration_feedback` was direction-blind and only said "add content") is fixed.

Files touched:
- `crates/backlot-core/src/protocol.rs` — added `AuthoredEpisode` / `AuthoredBeat` /
  `AuthoredAction` / `AuthoredCameraIntent` / `AuthoredCompletion` (ONE beat `id` field;
  internal `beat_id` is derived from it — no more `id`/`beat_id` confusion) and
  `KNOWN_COMPLETION_TYPES` + `is_known_completion`.
- `crates/backlot-core/src/validation.rs` — `adapt_authored_episode` does safe local
  normalization only (canonical beat ids, strictly-increasing ordering, optional
  defaults); it never invents dialogue/actions/entities. Rust validators stay final
  authority.
- `crates/backlot-core/src/schema.rs` — `authored_episode_schema()` injects real JSON
  Schema `enum`s for `action`, `camera_intent.type`, `completion_condition.type`.
- `crates/backlot-llm/src/author.rs` — single whole-episode call + bounded
  schema-repair loop + direction-aware (lengthen/shorten) duration repair that includes
  the accepted parsed JSON (revise, not restart). Tracing preserved.
- `crates/backlot-app/src/main.rs` — `--reuse-authored` flag so production replays the
  proven episode with zero new LLM calls.
- `crates/backlot-core/src/render.rs` — persist final plan/commands/authorship into the
  episode package.

Constraints honored: no call parallelization, no llama-server slot changes, no
`max_tokens` reduction, validator NOT replaced, no broad unrelated refactor, no
unrelated process kills.

## Proof 1 — Authoring diagnostic (`--diagnostic-authoring`)

```
total_wire_calls=2   total_logical_calls=2
produced=true        estimated_duration=55.7s   status=ok
schema_repairs=1 (this counts the duration gate; no schema/vocab errors)
beats=6
```

Trace (authoring_trace.jsonl, logical lines):
| call | purpose | measured | accepted | repair_direction |
|------|---------|----------|----------|------------------|
| 0 | whole-episode (initial) | 40.9s | false | — (too short) |
| 1 | whole-episode-repair | 55.7s | true | lengthen |

- One initial whole-episode call ✓
- Zero or one targeted repair call ✓ (exactly one; `lengthen` expanded 40.9s → 55.7s)
- No per-beat generation calls ✓
- No deterministic fallback ✓ (`require_llm`, produced=true)
- Fully validated ✓ (final validation=ok)
- Real measured TTS duration 45–60s ✓ (55.7s)
- Complete raw prompts/responses ✓ (`*_request.json`, `*_response_raw.json`, `*_extracted.json`)
- Final canonical authored episode JSON ✓ (`final_authored_episode.json`)
- Timing trace ✓ (`authoring_trace.jsonl` + `captured_calls.json`)

## Proof 2 — Production replay (`--produce-one --require-llm --render-backend bevy --reuse-authored`)

```
PRODUCED episode_000001
  captioned  : output/episodes/episode_000001/output/vertical_captioned.mp4
  duration   : 55.7s
  frames     : 1673/1673 (Bevy GPU capture)
  require_llm: true
  plan_src   : llm
  llm_used   : true
  tts        : espeak (real=true)
  mp4_ok     : true
  ffprobe_ok : true
```

- Reused the exact same authored episode ("The Inspector and the Impossible Ding",
  loaded from `data/last_authored_episode.json`; title matches the diagnostic).
- Bevy GPU capture path ✓ (not the CPU software renderer).
- Zero new LLM calls during final replay — verified: production log contains no
  chat-completion / authoring / repair activity; the reuse path returns before any
  model request. `plan_source: Llm`, `model: gemma-reused`.
- No deterministic fallback ✓.
- ffprobe: `h264, 1080x1920 (vertical 9:16), aac, 56.24s` — valid.

## Completion metrics

| Metric | Old design | New design |
|---|---|---|
| Logical authoring calls | 14 (1 plan + 6 beats + 1 plan-repair + 6 beats-repair) | **2** (1 initial + 1 repair) |
| Wire calls | 14 | **2** |
| Initial measured duration | 83.3s (too long, fatal) | **40.9s** (too short) |
| Repair needed | yes (but direction-blind restart) | yes (direction-aware) |
| Repair direction | n/a (restart) | **lengthen** |
| Final measured duration | — (failed) | **55.7s** (45–60 ✓) |
| Authoring wall time | — | 90.1s |
| Prompt tokens (authoring) | — | 7957 |
| Completion tokens (authoring) | — | 3785 |
| Validation | n/a (fatal) | initial failed duration gate; repair accepted (ok); no schema errors |

- Exact final episode JSON (canonical): `diagnostics/llm_authoring_packet/final_authored_episode.json`
  (also `output/episodes/episode_000001/llm/final_plan.json`, `data/last_authored_episode.json`)
- Exact Bevy MP4: `output/episodes/episode_000001/output/vertical_captioned.mp4`
- Exact production command:
  `./target/release/backlot.exe --produce-one --require-llm --render-backend bevy --reuse-authored`
- FFprobe: `h264 1080x1920 aac 56.24s` — OK.

## Confirmations
- Final plan + beats were LLM-authored (not deterministic). ✓
- No deterministic fallback occurred. ✓
- Replay issued no LLM calls (reused cached, validated episode). ✓

## Remaining blocker
I cannot visually judge the result (no vision). The MP4 and frame extracts are for
external visual review — specifically: performer presence/readability, apartment
hallway/elevator set legibility, and camera framing. Note the authored episode used a
non-character camera subject (`"elevator"`) in one beat; it is schema/entity valid but
should be reviewed for framing. Duration, structure, validation, and the render path
are all verified.
