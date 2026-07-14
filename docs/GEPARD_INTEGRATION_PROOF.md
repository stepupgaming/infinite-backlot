# Gepard Production TTS Integration Proof

Episode 0001 was replayed from the frozen authored fixture at
`data/cached_episode_set_proof.json`. The replay made zero new LLM requests and
submitted all 15 dialogue lines to one `gepard_batch` worker invocation.

## Measured result

- Gepard worker invocations: 1
- Gepard model loads: 1
- Submitted/successful dialogue lines: 15 / 15
- Cache hits/misses for the proof run: 0 / 15 (`--tts-cache-bypass`)
- Espeak/estimating lines: 0 / 0
- Model loading: 13.616 seconds
- Dialogue generation: 29.589 seconds
- Per-line generation: 1.109 seconds minimum, 1.9726 seconds mean, 3.373 seconds maximum
- Managed TTS lifecycle: 55.262920 seconds
- Timeline rebuild: 0.000296 seconds
- Final measured schedule: 59.652866 seconds
- Final encoded duration: 59.666667 seconds
- Bevy capture: 1,790 native 1080×1920 frames at 30 fps

The worker trace was valid JSONL with one load-start event, one load-complete
event, and 15 successful line-complete events. Every returned WAV opened
successfully and matched the worker-reported sample rate and duration.

## Durable contracts

- Production provider: `gepard_batch`
- Runtime: `runtimes/gepard`
- Worker: `runtimes/gepard/backlot_gepard_worker.py`
- Batch request fixture: `diagnostics/gepard_smoke_requests.json`
- Episode proof manifest: `diagnostics/gepard_episode_0001_proof.json`
- Voice policy: `docs/BACKLOT_VOICE_REGISTRY.md`
- Runtime instructions: `runtimes/gepard/BACKLOT_RUNTIME.md`
- Cache identity includes provider, runtime/model revision, preset, voice ID,
  reference hash, deterministic seed, and normalized text.
- A failed or incomplete Gepard batch is fatal; production never falls back to
  espeak or estimated timing.
- Worker-measured WAV durations rebuild dialogue, caption, gesture, reaction,
  and final episode timing without re-authoring the frozen story.

## Final review artifact

The generated captioned review video is intentionally excluded from Git:

`output/blender-set-gepard/episodes/episode_000001/output/vertical_captioned_gepard.mp4`

- SHA-256: `236be56842ab7d026444483d9dbb39a63730331f563e418bf0002d66fd1f6132`
- Video: H.264, yuv420p, 1080×1920, 30 fps
- Audio: AAC, 48 kHz stereo

Generated WAVs, frames, videos, traces, caches, model weights, and machine-local
output packages are not durable repository content and are deliberately omitted
from the integration commit.
