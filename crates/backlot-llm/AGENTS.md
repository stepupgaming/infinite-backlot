# Purpose

Own OpenAI-compatible structured requests and the LLM-backed implementation of the core episode-authoring contract.

# Ownership

- `client.rs` owns SDK configuration, structured response formats, retries, capture, and request metrics.
- `author.rs` owns whole-episode prompting, schema repair, duration-directed revision, adaptation, reuse/repair, and authorship metrics.
- `tests/require_llm.rs` guards explicit failure semantics; `tests/llm_smoke.rs` is the opt-in live structured-call smoke.

# Local Contracts

- Return only core `EpisodeAuthor` results and pass every model result through core schema and semantic validation before execution.
- Keep bounded action, camera, completion, entity, and duration guidance synchronized with the core protocol and schema.
- In require-LLM mode, initialization, request, parse, validation, or repair failure is fatal and must never be relabeled as model-authored success.
- Keep retries and repairs bounded, direction-aware, observable, and attributable. Do not hide extra wire calls or restart an episode without recording why.
- Diagnostic capture must preserve useful raw response and timing evidence without credentials.

# Work Guidance

- Prefer a coherent whole-episode transaction and targeted revision over multiplying independent model calls.
- Keep prompts factual about executable capabilities and test failure behavior without requiring a live service.

# Verification

- Run `cargo test -p backlot-llm`.
- Run `cargo test -p backlot-llm --test llm_smoke -- --ignored` only with the intended owned Gemma runtime available.
- Regenerate the authoring diagnostic packet after material prompt, schema, call-graph, or duration changes.

# Child DOX Index

No child DOX files yet.
