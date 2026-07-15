# Purpose

Own a self-contained evidence packet for the current whole-episode structured-authoring workflow.

# Ownership

- `*_request.json`, `*_response_raw.json`, and `*_extracted.json` preserve each captured logical call and its wire response.
- `captured_calls.json` and `authoring_trace.jsonl` aggregate outcomes and timings.
- `authored_episode_schema.json`, `duration_analysis.json`, and the `final_*` files preserve the accepted schema, measurements, authored episode, adapted plan, and commands.
- `LLM_AUTHORING_DIAGNOSTIC_PACKET.md` is the human-readable index and assessment for the same run.

# Local Contracts

- All files in the packet must describe one coherent diagnostic run; call counts, model ID, timings, accepted/rejected status, duration analysis, and final artifacts must agree.
- Retain exact prompts and complete server responses needed for diagnosis while excluding credentials and unrelated private data.
- Schema claims must be traceable to the checked-in Rust schema and validator behavior at the time of capture.
- Do not manually rewrite evidence to improve an outcome. Regenerate the packet when authoring behavior changes materially.

# Work Guidance

- Keep the Markdown summary concise enough to navigate the raw artifacts and explicit about observed facts versus assessment.
- Replace stale files that are no longer emitted; do not leave artifacts from an earlier run mixed into a refreshed packet.

# Verification

- Run `cargo run -p backlot-app -- --diagnostic-authoring` against the intended owned model runtime.
- Confirm the command completes successfully and cross-checks the packet's call counts, schema, final plan, commands, and duration status.

# Child DOX Index

No child DOX files yet.
