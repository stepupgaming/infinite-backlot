# Purpose

Own Infinite Backlot's engine-independent episode domain, bounded protocol, validation, persistent world, production pipeline, media integration, and package formats.

# Ownership

- Authoring, protocol/schema, director, validation, story, world, timeline, and seeded RNG modules own narrative and execution truth.
- Avatar, motion, stage, TTS, ASR, and render modules own engine-independent performance and production behavior.
- Config, errors, and package modules own shared configuration and durable output contracts.
- `tests/` owns deterministic direction, critical boundary, and headless end-to-end package coverage.

# Local Contracts

- Keep this crate free of Bevy types so domain behavior remains headless and reusable.
- Treat `validate_plan`, `validate_beat_command`, capability vocabularies, and authored-episode adaptation as the final authority before world execution.
- Keep serialization schemas, package contents, timeline semantics, and config defaults explicit and deterministic.
- Treat `open_elevator` and `close_elevator` as persistent ordered environment state transitions; the latest authored action controls door state and receives a camera insert.
- Compile authored blocking into reserved stage-slot movement before timeline construction. Exact slot IDs take precedence over semantic fallback groups; required movement fails closed when no collision-safe, reachable destination exists.
- Keep interaction actions behind the interacting actor's arrival time, and reserve reveal/panel camera corridors through executable actor-clearing movement rather than prose-only claims.
- Preserve truthful authorship, TTS/ASR provider, repair, and render evidence in diagnostics and packages.
- `gepard_batch` is the production dialogue provider: collect the complete episode before timeline construction, run one load-once batch, preserve source WAVs, verify every response/WAV, and rebuild schedule/captions/performance timing from measured durations. Gepard failures are fatal and never fall back to espeak or estimating.
- TTS caches are provider-specific. Gepard cache identity includes normalized text, model/codec/runtime identity, voice/reference hash, deterministic seed, and every waveform-affecting preset field.
- External tools and runtimes may fail, but a reported production artifact must be real and verifiable; never synthesize success metadata.
- Coordinate public schema or package changes with `backlot-llm`, `backlot-app`, data fixtures, diagnostics, and downstream assets.

# Work Guidance

- Put cross-cutting domain behavior here only when it can remain engine-independent.
- Add focused tests around new bounded vocabularies, validation rules, output fields, and failure semantics.

# Verification

- Run `cargo test -p backlot-core`.
- Run `cargo test -p backlot-core --test critical_boundaries -- --ignored` only when validating the full external producer path.

# Child DOX Index

No child DOX files yet.
