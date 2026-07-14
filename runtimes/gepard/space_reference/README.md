---
title: GEPARD
emoji: 🐆
colorFrom: yellow
colorTo: red
sdk: gradio
# 5.49.x is the "bridge" release: it accepts huggingface-hub >=0.33.5,<2.0,
# so it co-resolves with the NeMo stack (hub 0.x) at build time AND keeps
# working after create_env.py re-pins transformers==5.3.0 (hub 1.x) at runtime.
# gradio 6.x requires hub>=1.2 and cannot be co-installed with nemo-toolkit.
sdk_version: 5.49.1
python_version: '3.12'
app_file: app.py
pinned: false
license: apache-2.0
short_description: Spotted text-to-speech with voice cloning and text-CFG
---

# 🐆 GEPARD TTS

Inference Space for the **Gepard** autoregressive speech model
(`nineninesix/gepard-1.0` — a Qwen3.5 backbone with 32 FSQ audio
heads, a Q-Former voice-cloning compressor, and DPO post-training).

## Features

- **Preset speakers** — bundled voices stored as pre-encoded codec tokens
  (`speakers/*.pt`), no codec pass needed at request time.
- **Voice cloning** — record with the microphone or upload a clip; the
  reference is encoded with the NeMo nano codec and compressed into 8 speaker
  prefix tokens.
- **Text classifier-free guidance** — `cfg_scale`/`cfg_frames` sharpen text
  and voice adherence (defaults: `temperature=0.3`, `cfg_scale=3`).
- All generation knobs are exposed under **Generation settings**.

## Configuration

`config.yaml` selects the checkpoint, codec, preset speakers and default
generation parameters — re-point the Space without touching code.

## Notes

- The model repo is private: set the `HF_TOKEN` Space secret.
- ZeroGPU: the model is loaded once at startup; each request only runs
  generation inside the GPU context.
- `create_env.py` re-pins `transformers==5.3.0` at startup (NeMo downgrades
  it at build time) — it must run before any ML import in `app.py`.
