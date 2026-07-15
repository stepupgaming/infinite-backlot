# Purpose

Own human-editable runtime configuration and reusable or persistent episode inputs.

# Ownership

- `config.toml` is the checked-in local production configuration consumed by `backlot-core::config`.
- `last_authored_episode.json` is the frozen authored-episode input used by explicit reuse and repair modes.
- `cached_episode_set_proof.json` is the frozen, duration-validated episode used for zero-new-LLM set-render proof through `--reuse-authored-path`.
- `episodes/`, `presets/`, `shows/`, `voices/`, and `world/` are reserved persistent-data domains and remain owned here while empty.

# Local Contracts

- Keep `config.toml` synchronized with the Rust configuration structs and defaults in `crates/backlot-core/src/config.rs`.
- Never commit credentials, access tokens, private voice material, or model weights. Machine-local executable, cache, and model paths may be recorded only when they are intentional project defaults.
- Treat `last_authored_episode.json` as structured input, not an informal example; preserve schema validity and truthful authorship data.
- Treat every `--reuse-authored-path` input as immutable authored content: replay may validate and adapt it locally but must issue zero model requests.
- Keep `cached_episode_set_proof.json` exercising both `open_elevator` and `close_elevator`, with authored elevator camera inserts that visibly prove both semantic door states.
- `runtime.render_quality` changes capture resolution only; it must not alter timeline duration, frame timestamps, authored content, or replay-call guarantees.
- `config.toml` owns the reusable Gepard runtime location, worker/device/timeout, default generation preset, cache bypass, and stable character voice registry. Keep it synchronized with `docs/BACKLOT_VOICE_REGISTRY.md`.
- Generated episode packages belong under ignored `output/`, not `data/episodes/`, unless a future workflow explicitly promotes them to curated inputs.

# Work Guidance

- Prefer explicit config fields over hidden environment-dependent behavior.
- Add a child DOX file when a reserved data domain gains a stable schema or operating workflow.

# Verification

- Run `cargo test -p backlot-core` after configuration schema/default changes.
- Exercise the exact CLI mode that consumes a changed reusable authored episode.
- Verify cached proof runs report `llm_requests: 0` and `replay_no_llm: true` in `diagnostics.json`.

# Child DOX Index

No child DOX files yet.
