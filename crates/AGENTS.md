# Purpose

Own the Rust workspace that authors, validates, renders, and packages Infinite Backlot episodes and supervises its model processes.

# Ownership

- Each direct child crate has its own AGENTS.md and owns its source, tests, examples, and package manifest.
- Shared dependency versions, workspace membership, release profile, Rust edition, and toolchain floor remain owned by the root `Cargo.toml` under the repository-root contract.

# Local Contracts

- Preserve the current dependency direction: `backlot-app` composes the system; `backlot-llm` integrates through `backlot-core`; `backlot-motion` and `backlot-runtime` remain reusable lower-level crates; `backlot-core` remains Bevy-independent.
- Put cross-crate data and behavioral contracts in the lowest reusable crate rather than duplicating them in the app.
- Keep serialized schemas, runtime request formats, asset manifests, and committed diagnostic fixtures synchronized with their Rust producers and consumers.
- Use workspace dependencies for libraries shared across crates.

# Work Guidance

- Prefer focused public APIs and keep process ownership, model inference, domain validation, and Bevy presentation in their existing crates.
- Treat warnings, source attribution, and external-tool results as production behavior, not incidental logging.

# Verification

- Run `cargo fmt --all -- --check` for Rust edits.
- Run `cargo test` for workspace-wide behavioral changes; use the nearest crate command while iterating.

# Child DOX Index

- `backlot-app/AGENTS.md` — Bevy application, operator modes, state machine, and GPU capture.
- `backlot-core/AGENTS.md` — engine-independent episode domain, validation, production, and packaging.
- `backlot-llm/AGENTS.md` — OpenAI-compatible structured authoring and diagnostics.
- `backlot-motion/AGENTS.md` — motion import, processing, retargeting, compilation, and library formats.
- `backlot-runtime/AGENTS.md` — owned external-process lifecycle and runtime telemetry.
