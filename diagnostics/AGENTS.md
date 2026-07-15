# Purpose

Own committed, reproducible evidence for model-runtime smoke tests and authoring behavior.

# Ownership

- `gepard_smoke_requests.json`, `kimodo_smoke_requests.json`, and `parakeet_smoke_requests.json` are batch-worker inputs.
- `gepard-smoke/` and `kimodo-smoke/` contain retained smoke outputs needed to inspect runtime contracts.
- `llm_authoring_packet/` is a self-contained structured-authoring evidence bundle with a child contract.

# Local Contracts

- Diagnostic artifacts must identify the code path, inputs, outputs, and outcome they support; do not commit unverifiable success claims.
- Keep request/response JSON synchronized with the corresponding worker structs and schemas.
- Gepard smoke evidence includes a real WAV, response JSON, pure JSONL trace, and stderr log; exclude model weights, caches, and unrelated runtime output.
- Raw model responses and traces must not contain credentials, tokens, or unrelated private data.
- Replace a diagnostic bundle atomically when regenerating it so summaries, raw calls, extracted payloads, schemas, and final artifacts agree.

# Work Guidance

- Prefer small committed fixtures that reproduce an interface boundary over large transient logs.
- Store ordinary run output under ignored `output/`; promote only durable evidence into this tree.

# Verification

- Re-run the producer or runtime worker that owns any changed fixture and inspect its durable response file.
- Run the child verification when changing the authoring packet.

# Child DOX Index

- `llm_authoring_packet/AGENTS.md` — exact prompts, raw calls, schemas, traces, duration analysis, and final authoring results.

Runtime smoke fixtures remain directly owned by this file.
