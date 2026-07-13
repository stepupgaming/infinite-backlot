# LLM Authoring Diagnostic Packet (redesigned single-call authoring)

Generated 1783973041

**Purpose.** Capture real evidence (exact prompts, raw model responses, per-call timing, schemas, duration logic, server config) for the REDESIGNED authoring stage ONLY — no rendering. Authoring now collapses the entire episode into ONE whole-episode structured call (`AuthoredEpisode`), with at most ONE direction-aware revision call if the measured runtime misses the 45–60s window.

## 1. Current (redesigned) authoring call graph

One episode is authored as:

1. **Initial whole-episode call** — `request_whole_episode()` → 1 structured call (schema `AuthoredEpisode`). The model returns episode metadata AND every fully-authored beat in a single JSON object.
2. **Schema-repair loop** — the call is wrapped in a `0..=max_repairs` loop. On any parse/validation failure the concrete error is fed back (`SCHEMA CORRECTION`) and the SAME whole-episode call is re-issued. This is structural, not duration. There is exactly ONE beat identifier field (`AuthoredBeat.id`); the internal `beat_id` is derived from it, so the old `id`/`beat_id` confusion can never trigger another call.
3. **Format fallback** — inside the call, `chat_structured` first tries `json_object`, then falls back to strict `json_schema` (with bounded-vocabulary `enum`s) for up to `max_retries` attempts. So each logical call can emit 1–2 wire calls, but there is only ONE logical call (or two if a duration repair is needed).
4. **Direction-aware duration repair** — after the whole episode is measured (real TTS + dead-air compaction via `measure_runtime`), if it is outside 45–60s, exactly ONE targeted whole-episode revision call is issued. The accepted parsed episode JSON is included and the model is told to LENGTHEN (if too short) or SHORTEN (if too long) — never "add more" when too long. There are no per-beat calls and no full restart.

**Configuration during this run:** `max_repairs = 1` (director + schema-repair loop bound), `llm.max_retries = 2`, `max_tokens = 8192`, `temperature = 0.4`.

**Call count (worst case):** 1 initial logical call, +1 direction-aware repair logical call if needed. Each logical call = at most 1 + `max_retries` wire calls. Observed this run: **1 wire calls**, **1 logical calls**.

## 2. Exact prompt templates

Assembled in `crates/backlot-llm/src/author.rs`. Fully rendered instances are saved per call as `*_request.json`. The static system template is reproduced below.

### 2.1 System prompt (verbatim)

```
You are the showrunner for 'Infinite Backlot', an autonomous surreal comedy set in an apartment building where impossible events are treated as ordinary maintenance problems. You author ONE complete, SHORT, WATCHABLE episode per response as a single valid JSON object matching the `AuthoredEpisode` schema. Do not output prose, markdown, or commentary outside the JSON fields. Keep dialogue short and purposeful. Aim for a hook in the first 3 seconds and a clear character goal by 10 seconds. Use ONLY these action tokens: {}. Use ONLY these camera intents: {}. Reference only entities (characters, props, locations, staging marks) that exist in the provided world.
```

Dynamic insertions: `{}` → `KNOWN_ACTIONS.join(", ")`; `{}` → `KNOWN_CAMERA_INTENTS.join(", ")`.

### 2.2 Whole-episode user prompt

The user prompt embeds the world digest, target duration, tone, recent episodes, canonical facts, the `AuthoredEpisode` field spec (with a concrete example), the duration guidance (NO "2–4 spoken lines per beat" rule), the bounded-vocabulary hard rules, and — on repair — a `DURATION REPAIR` block plus the accepted episode JSON. Rendered instances are in `*_request.json`.

## 3. Fully rendered real prompts

Saved as separate UTF-8 files in this directory:

- `NN_whole-episode_*_request.json` — initial whole-episode request (system + user).
- `NN_whole-episode-repair_*_request.json` — direction-aware repair request (stage `duration-repair`).

Each request file contains the exact `system` and `user` messages sent (no secrets; the API key is empty).

## 4. Raw model responses

For every logical call, `*_response_raw.json` holds the COMPLETE server JSON (choices, `content`, `reasoning_content` when present, `finish_reason`, `usage`, `model`, `id`). `*_extracted.json` holds only the extracted JSON object. `captured_calls.json` aggregates all of them with validation result and accept/reject flag.

_Logical calls captured: 1. (wire calls: 1)_

- `whole-episode` [initial] accepted=true validation=ok measured=Some(59.95226)s direction=None

## 5. Timing trace

Flushed per-event trace: `authoring_trace.jsonl` (one JSON object per physical HTTP request, plus a `kind:"logical"` line per structured call). Total wire calls = 1, total logical calls = 1, sum of wire latencies = 760.3s, observed span (first start → last end) = 760.3s.

| # | purpose | format | wall(s) | prompt_tok | compl_tok | finish | ok |
|---|---|---|---|---|---|---|---|
| 0 | whole-episode | json_object | 760.3 | 2879 | 2270 | Some("stop") | true |
| 1 | whole-episode (logical) | - | - | 2879 | 2270 | Some("stop") | true |

## 6. Schemas and validators

The schema is `schemars`-derived from `AuthoredEpisode` in `crates/backlot-core/src/protocol.rs` and written to `authored_episode_schema.json` (8087 bytes). Crucially, the bounded vocabularies are now real JSON Schema `enum`s injected into the schema: `actions[*].action` ∈ KNOWN_ACTIONS (59 tokens), `camera_intent.type` ∈ KNOWN_CAMERA_INTENTS (13 tokens), `completion_condition.type` ∈ KNOWN_COMPLETION_TYPES (5 tokens). So the model is constrained at the schema level; the Rust validators (`validate_plan` / `validate_beat_command`) remain the final authority.

**AuthoredEpisode required fields:** `episode_title, logline, target_duration_seconds, active_characters, primary_location, central_goal, beats, payoff`. Optional: `tone, persistent_changes, notes`.

**AuthoredBeat (one per beat, single id field) required:** `id, narrative_purpose, target_start_second, actions, camera_intent, completion_condition`. Optional: `fallback, expected_state_changes, notes`. The internal `beat_id` is derived from `id` during `adapt_authored_episode` — no second call, no `id`/`beat_id` confusion.

**Validation outcomes this run:**

- accepted logical calls: 1
- rejected (schema repair triggered): 0

## 7. Duration logic

Pipeline:

1. **Plan duration hint:** the model is told `target_duration_seconds` (~50) and a NATURAL pacing guidance: 5–6 concise beats, ~30–42s of total spoken dialogue, the rest meaningful actions/reactions/transitions, and NO padding. The old hard rule "2–4 spoken lines per beat" that caused the 83.3s overrun is GONE.
2. **Dialogue duration:** measured by REAL espeak TTS in `measure_runtime` — each `speak`/`whisper`/`shout` line is synthesized, silence-trimmed, and its true length used.
3. **Action duration:** `estimate_action_duration(action, text)` heuristic per action.
4. **Pauses/transitions:** `compact_dead_air` compresses gaps > `max_dead_air_secs`, so padding does NOT add time.
5. **Accepted range:** 45–60s.
6. **Direction-aware repair:** if the measured runtime misses the window, exactly ONE targeted whole-episode revision is issued. If too SHORT → told exact seconds missing + which beats are underdeveloped + to add content (preserve hook/premise/payoff). If too LONG → told exact seconds to remove + to cut/shorten redundant dialogue and combine actions (preserve hook/escalation/reaction/payoff, do NOT add beats). The accepted parsed episode JSON is included so the model revises rather than restarts.

**This run:** estimated duration = 60.0s. Status = `ok`. Breakdown: `{
  "beats": 6,
  "est_action_secs": "86.9",
  "est_spoken_secs_from_chars": "65.3",
  "spoken_chars": 1105,
  "spoken_lines": 15,
  "target_duration_seconds": 50.0
}`. Repair needed = false (direction: None).

The generated plan title was `The Floor That Isn't` with 6 beats. The full plan is in `final_plan.json`; the canonical authored episode is in `final_authored_episode.json`.

## 8. Model and server configuration

- **Model identifier:** `dcf179a91153e3a7ece792e48ef872180d9d6ef9b7677f0a0bd3e83cfe624d5e`
- **Base URL:** `http://127.0.0.1:8080/v1`
- **Slots:** inferred 1 (single-slot); wire calls are strictly serialized. With the redesigned 1–2 logical calls the wall time is now dominated by 1–2 generations, not 7+.
- **temperature:** 0.4
- **max output tokens:** 8192
- **timeout:** 1800s
- **retry count (effective):** `max_repairs = 1` (schema-repair loop bound).
- **streaming:** disabled

## 9. Relevant source map

- `crates/backlot-llm/src/client.rs::LlmClient::chat` — production requests.
- `crates/backlot-llm/src/client.rs::chat_structured` — json_object→json_schema fallback.
- `crates/backlot-llm/src/client.rs::chat_structured_capture` / `raw_post` — diagnostic raw path preserving `reasoning_content`, usage, model id; appends per-wire + per-logical trace lines.
- `crates/backlot-llm/src/author.rs::request_whole_episode` — the single whole-episode call + bounded schema-repair loop.
- `crates/backlot-llm/src/author.rs::author_async_inner` — measures runtime, accepts if in range, else issues ONE direction-aware repair call.
- `crates/backlot-llm/src/author.rs::direction_aware_feedback` — the direction-aware (lengthen/shorten) repair prompt.
- `crates/backlot-core/src/validation.rs::adapt_authored_episode` — safe local normalization (canonical beat ids, ordering, defaults) + structural mapping; does NOT invent content.
- `crates/backlot-core/src/validation.rs::validate_plan` / `validate_beat_command` — the final authority.
- `crates/backlot-core/src/protocol.rs` — `AuthoredEpisode` / `AuthoredBeat` (schema source, single beat id).
- `crates/backlot-core/src/schema.rs::authored_episode_schema` — schemars schema + `enum` injection.
- `crates/backlot-core/src/render.rs::measure_runtime` — real TTS duration (authoritative gate).

## 10. Initial evidence-based assessment

- **Calls consuming the most time:** ONE initial whole-episode call (at most 1+`max_repairs` wire calls) and at most ONE direction-aware repair call. Worst case wire calls ≈ 2 × (1 + `max_repairs`) = 4 (vs 28 before).
- **Excessive reasoning?** reasoning_content present in 0 of 1 captured calls — none observed.
- **Schema constrains vocabulary now?** Yes — `action`/`camera_intent.type`/`completion_condition.type` are real JSON Schema `enum`s, so out-of-vocab tokens are blocked at the schema level (Rust validation still the final authority).
- **Per-beat calls?** Eliminated. All beats arrive in one `AuthoredEpisode` response.
- **Duration repair = targeted edit or restart?** Targeted, direction-aware whole-episode revision that includes the accepted episode JSON. Never a blind restart, never "add more" when too long.
- **One whole-episode call feasible?** Yes — proven by this run (see §4/§5).

**Outcome:** produced complete valid episode = true. Estimated duration = 60.0s. Single biggest fix applied: collapsed 7+ calls into 1–2 and replaced the direction-blind duration repair (which caused the 83.3s overrun) with a direction-aware one.
