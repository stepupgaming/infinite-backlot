## Implementation plan

I will preserve the proven architecture:
- one whole-episode Gemma call plus at most one duration-aware revision
- validated 45–60s authored episode
- cached replay with zero replay-time LLM calls
- Bevy GPU final render path
- no deterministic fallback for the final candidate
- no CPU-rendered final output substitution

### What the audit shows

#### 1) The current limitation is split across authoring and runtime
Authoring is not pure dialogue, but it is still thin on physical staging.
- The latest authored episode already includes some movement and interaction (`approach`, `move_to`, `inspect`, `activate`, `write_note`, `look_at`) in `diagnostics/llm_authoring_packet/final_authored_episode.json:8-319`.
- But each beat still has only ~4 actions, many targets are broad (`floor_3_hallway`), and there is no authored notion of blocking, reaction intent, camera purpose, or bounded gesture timing.
- The whole-episode prompt is strongly dialogue-weighted: it explicitly says to hit runtime with “VOLUME of dialogue” and asks for roughly 14–20 spoken lines as the main source of length in `crates/backlot-llm/src/author.rs:1181-1187`.

Runtime likely flattens requested actions into long static states.
- In the shared timeline, movement is time-windowed, but performance states are selected by scanning past actions and keeping the last matching state with no end-bound check for react/gesture/look/listen in `crates/backlot-core/src/timeline.rs:477-528`.
- That is a strong root-cause match for “gestures becoming stuck poses” and “animation states lasting too long”.
- The Bevy player also sets `emote` strings on action fire without a visible bounded recovery path in `crates/backlot-app/src/player.rs:357-437`.

Camera direction is also contributing to the flat feel.
- The packaged camera plan spends long spans in repeated closeups: e.g. `speaker_closeup` from 2.8–15.0s and 19.0–34.3s in `output/episodes/episode_000001/camera_plan.json:19-65`.
- The plan also emits zero-length tail shots in `output/episodes/episode_000001/camera_plan.json:163-225`.
- Framing diagnostics already reject multiple Voss shots due to 84% occlusion in `output/episodes/episode_000001/review/framing_report.json:161-201`.
- The packaged report still says there are no issues in `output/episodes/episode_000001/report.md:13-26`, so diagnostics are not yet being surfaced truthfully into the review handoff.

Captions and TTS are only partially there.
- Captions use ASS already and have better normalization/wrapping than the older drawtext path in `crates/backlot-core/src/render.rs:1189-1222`, but there is no rendered-bounds validation yet; diagnostics fields like caption safety remain zeroed in `output/episodes/episode_000001/diagnostics.json:20-24`.
- TTS currently supports only `estimating` and `espeak`, with `espeak` as the only real local provider in `crates/backlot-core/src/tts.rs:43-188` and `crates/backlot-core/src/config.rs:112-142`.
- The production artifact confirms the latest run used `espeak` in `output/episodes/episode_000001/audio/tts_manifest.json:1-66` and `output/episodes/episode_000001/diagnostics.json:100-108`.

### Planned changes

## 1. Fix character performance in the shared timeline first
Primary goal: make gestures and reactions temporary, and make movement / turning / gaze visibly resolve.

Changes:
- Refactor state selection in `crates/backlot-core/src/timeline.rs` so non-movement actions only affect a character during their own active time window instead of latching forever.
- Introduce bounded performance phases for gesture-like actions: preparation, active, short hold, recovery.
- Map authored actions into clearer runtime states:
  - neutral idle
  - walk
  - turn/look
  - speak-neutral / speak-emphatic
  - listen
  - react
  - point
  - inspect/press/interact
  - return-to-idle
- Preserve stable floor contact and current shared schedule architecture.
- Keep Bevy consuming the same shared `FrameState`; do not fork a second interpretation path.

Likely touch points:
- `crates/backlot-core/src/timeline.rs` for action windowing, state transitions, gaze/turn behavior, and any interaction-derived state.
- `crates/backlot-core/src/avatar.rs` / related pose logic for better separation of idle, talk, react, point, inspect.
- `crates/backlot-app/src/player.rs` and `crates/backlot-app/src/state.rs` only as needed so Bevy-side event emotes recover cleanly during live playback/rehearsal.

Expected result:
- speaking gestures begin and end
- both arms do not remain raised through scenes
- listeners visibly react during or after a line
- characters return to believable neutral poses

## 2. Improve authored visual action without reopening the one-call architecture
Primary goal: bias the single whole-episode call toward visible blocking and reactions, not just dialogue length.

Changes:
- Keep `AuthoredEpisode` as the single-call response shape.
- Extend the authored schema minimally with optional authoring-only fields that help express what the user wants, such as:
  - beat-level blocking / visible action / reaction / camera purpose / performance intent
  - possibly an action-level lightweight timing hint for gesture windows
- Update the schema generation in `crates/backlot-core/src/schema.rs` and structs in `crates/backlot-core/src/protocol.rs`.
- Preserve `adapt_authored_episode` in `crates/backlot-core/src/validation.rs:553-668` as the single adaptation point.
- Rebalance the whole-episode prompt in `crates/backlot-llm/src/author.rs:1075-1210` so every beat is asked to include:
  - a concrete blocking or staging change when appropriate
  - one visible action
  - a reaction
  - a specific object/environment focus where relevant
  - camera purpose
  - concise dialogue rather than dialogue volume as the main runtime driver
- Update the duration repair prompt so “lengthen” does not only demand more spoken lines in `crates/backlot-llm/src/author.rs:1213+`; instead it should allow added visible business and short back-and-forth.

Expected result:
- final authored episode is more likely to include walking, turning, interaction, reaction, escalation, and payoff beats explicitly
- still only one initial call plus at most one revision

## 3. Improve camera direction and shot rejection
Primary goal: reduce repeated closeups, add purposeful coverage, and stop bad candidate shots from silently persisting.

Changes:
- Audit and refine shot expansion logic in the shared timeline/director path, especially where beat intents become `camera_shots`.
- Enforce stronger rejection or replacement for:
  - repeated framing too similar to previous shot
  - large occlusion
  - tiny performer framing
  - missing active speaker/reaction subject
  - bad prop visibility on insert/interaction shots
  - zero-length shots
- Expand coverage patterns so authored conversation and interaction beats can produce:
  - two-character context shots
  - medium speaker shots
  - listener reaction shots
  - OTS / side angles
  - prop/elevator inserts
  - wider blocking shots
  - payoff/final hold
- Surface rejected shots and camera issues into the report/review handoff instead of leaving `Issues: none`.

Likely touch points:
- `crates/backlot-core/src/timeline.rs` camera framing and shot offsets
- `crates/backlot-core/src/director.rs` shot planning
- `crates/backlot-core/src/render.rs` camera analysis packaging and report generation

Expected result:
- fewer nearly identical front-on closeups
- no zero-duration camera entries
- bad occluded Voss-style shots replaced or called out truthfully

## 4. Improve set readability and lighting in the Bevy scene, minimally
Primary goal: keep the same hallway/elevator but make it readable and better separated.

Changes:
- Adjust ambient, key, and fill lighting in the Bevy scene.
- Improve elevator interior visibility and control-panel/indicator readability.
- Improve material/value separation between floor, walls, doors, trim, and performers.
- Add only a small number of intentional hallway details if needed (door frames, trim, indicator, panel emphasis, signage, subtle practical lights), not a large world-art expansion.

Likely touch points:
- `crates/backlot-app/src/scene.rs`
- possibly `crates/backlot-app/src/bevy_capture.rs` only if render setup affects visibility

Expected result:
- less “mostly black/dark gray” readability
- better depth cues and performer/background contrast

## 5. Add a configurable higher-quality local HTTP TTS provider, while keeping espeak fallback
Primary goal: improve final production voice when a better local provider is available, without breaking timing or caching.

Changes:
- Extend `TtsConfig` in `crates/backlot-core/src/config.rs` to support a local HTTP-based provider configuration:
  - provider id
  - endpoint/base URL
  - per-character voice IDs
  - output format expectations
  - timeouts / error reporting fields as needed
- Implement a new `Tts` provider in `crates/backlot-core/src/tts.rs` that:
  - calls a local HTTP endpoint
  - writes WAV/PCM output to cache
  - measures returned duration exactly like current real providers do
  - preserves per-character voice persistence
  - reports clear failures
- Keep `espeak` available for tests and emergency fallback.
- Keep the existing timing architecture and schedule measurement flow unchanged.
- If no better provider is actually reachable in this environment, the code will still be ready, and the final report will say exactly what endpoint/config is required instead of pretending espeak is polished.

Expected result:
- production can use a better local engine when available
- no change to authoring/replay architecture
- honest provenance for which provider actually rendered the final candidate

## 6. Improve final mix modestly
Primary goal: add restrained ambience and interaction sounds without burying dialogue.

Changes:
- Inspect current mix pipeline and add lightweight SFX support for hallway ambience, ding, door, panel/button, and optional electrical effect.
- Keep dialogue dominant.
- Record audio provenance in the handoff.

Likely touch points:
- `crates/backlot-core/src/render.rs` audio mixing path
- possibly package/report structures if provenance needs to mention added effects

## 7. Fix captions and add rendered-bounds validation
Primary goal: keep captions fully inside safe bounds and away from critical action.

Changes:
- Keep the ASS subtitle path as the primary production path.
- Increase safe horizontal margins and raise placement to lower-middle if needed by adjusting `build_ass_subtitles` in `crates/backlot-core/src/render.rs:1192-1222`.
- Improve wrapping constraints beyond raw string length heuristics where needed.
- Add rendered caption bounds validation against actual subtitle layout / frame safe region, then write the result into diagnostics instead of leaving caption safety at zero.
- Feed caption safety failures into report issues.

Expected result:
- no clipped text
- better margins for 9:16 phone readability
- diagnostics reflect actual caption safety, not placeholder zeros

## 8. Add phase-level timing instrumentation and truthful packaging
Primary goal: report wall-clock durations per requested phase and effective Bevy FPS.

Changes:
- Add a production timing collector spanning:
  - LLM authoring
  - TTS generation
  - timeline preparation
  - Bevy frame capture
  - audio mixing
  - FFmpeg encoding
  - artifact packaging
  - total end-to-end
- Record effective rendered FPS as captured frames / Bevy capture wall time.
- Persist these metrics into diagnostics and the review handoff.
- Update report generation so real issues from framing/caption/TTS/voice limitations appear honestly instead of “none”.

Likely touch points:
- `crates/backlot-llm/src/author.rs` for authoring timing hookup
- `crates/backlot-core/src/render.rs` and `crates/backlot-app/src/bevy_capture.rs` / `main.rs` for production-stage timing
- package/report structures in `crates/backlot-core/src/package.rs`

## 9. Produce one improved candidate through the real pipeline
After code changes, I will run the real pipeline only:
- `--produce-one --require-llm --render-backend bevy`
- whole-episode Gemma authoring
- at most one duration revision
- cached replay check with zero replay-time LLM calls
- no deterministic fallback
- no CPU final render

Artifacts to generate / verify:
- final captioned MP4
- clean MP4
- extracted frames every two seconds
- frames per shot / reaction / interaction / caption set if the repository already has or can minimally add that packaging path
- contact sheet
- camera diagnostics
- animation-state timeline
- TTS provider + voice provenance
- review handoff document
- phase timing report

### Execution order
1. Fix shared timeline performance state windowing and recovery.
2. Improve shot planning/rejection so runtime output changes are immediately visible.
3. Rebalance authored schema/prompt minimally for action-first beats.
4. Add configurable HTTP TTS provider and preserve espeak fallback.
5. Tighten ASS caption placement and add bounds diagnostics.
6. Improve Bevy scene lighting/material readability.
7. Add phase timing + truthful issue surfacing.
8. Run one real LLM-authored Bevy candidate and package artifacts.

### Why this plan is minimal and aligned
- It does not reopen the authoring architecture.
- It treats the root cause as mixed: authored episodes need stronger physical direction, but runtime currently latches states and camera coverage is repetitive.
- It keeps the shared timeline as the single authority, which is the safest way to improve both CPU review and Bevy rendering together.
- It adds a better TTS path without hard-coupling to one engine or lying if that engine is unavailable.

### Expected permissions during implementation
- Read/update Rust source files in the repo
- Run cargo tests/builds
- Run the production pipeline and ffmpeg-based packaging
- Possibly call a configured local HTTP TTS endpoint if the environment is set up for it
