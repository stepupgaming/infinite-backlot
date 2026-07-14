# Infinite Backlot Gepard Runtime

This directory contains a pinned upstream Gepard checkout plus the project-owned `backlot_gepard_worker.py` batch adapter.

## Environment

Use the checked-in `uv.lock` and local `.venv`; never install dependencies with pip.

```bash
env -u PYTHONPATH uv run --frozen --no-sync python backlot_gepard_worker.py --help
```

Hermes exports its own control-plane `PYTHONPATH`, so both smoke commands and Backlot's process specification clear `PYTHONPATH` and set `PYTHONNOUSERSITE=1`. The worker must import only this runtime's pinned environment.

The production model root is configured in `data/config.toml` and is currently:

`F:/Models/InfiniteBacklot/gepard-1.0`

The model root is machine-local and is not committed.

## Batch contract

The worker accepts:

```text
--model-root <directory>
--requests <JSON array file>
--responses <JSON response file>
--device <cuda|cpu|...>
```

Each request contains `id`, `text`, `output`, `seed`, optional `reference_audio`, and optional `preset`. The preset supports `temperature`, `top_k`, `cfg_scale`, `cfg_frames`, `stop_threshold`, `max_frames`, `repetition_penalty`, and `repetition_window`.

The worker validates unique IDs, loads Gepard once, synthesizes every request, measures each real WAV, and writes one response-array JSON file. Every response records success, duration, sample rate, elapsed time, and an error when failed. The process exits nonzero when any line fails.

Standard output is JSONL progress only. Upstream model prints and logs are redirected to standard error so Backlot can persist a machine-readable trace.

## Smoke test

From `runtimes/gepard`:

```bash
mkdir -p ../../diagnostics/gepard-smoke
env -u PYTHONPATH uv run --frozen --no-sync python backlot_gepard_worker.py \
  --model-root F:/Models/InfiniteBacklot/gepard-1.0 \
  --requests ../../diagnostics/gepard_smoke_requests.json \
  --responses ../../diagnostics/gepard-smoke/responses.json \
  --device cuda > ../../diagnostics/gepard-smoke/trace.jsonl \
  2> ../../diagnostics/gepard-smoke/stderr.log
```

Verify the response's `success`, output existence, WAV duration, sample rate, and hash. Do not treat a response JSON alone as proof.

## Production lifecycle

Backlot runs one complete dialogue batch, waits for this worker to exit, verifies every WAV, and only then starts later GPU phases. A Gepard production failure is fatal. Espeak and estimated duration are not fallbacks for `gepard_batch`.
