# Infinite Backlot Voice Registry

This registry is the durable production mapping for recurring character voices. Runtime configuration in `data/config.toml` is authoritative; this document explains the stable identity and seed policy.

## Provider contract

- Provider: `gepard_batch`
- Runtime: `runtimes/gepard`
- Worker: `backlot_gepard_worker.py`
- Model identity: `nineninesix/gepard-1.0`
- Codec identity: `NanoCodec-pinned`
- Reference hashes: BLAKE3 of the reference WAV bytes
- Default generation preset: temperature `0.3`, top-k `0`, CFG scale `1.0`, CFG frames `0`, stop threshold `0.5`, maximum frames `2000`, repetition penalty `1.0`, repetition window `32`

## Stable seed policy

Each character has a fixed `seed_base`. Every normalized dialogue action receives a stable line ID of the form `line_NNNN_<content-hash-prefix>`. The generation seed is:

`seed_base XOR little_endian_u64(BLAKE3(line_id)[0..8])`

This makes a line deterministic while avoiding one identical random stream for every utterance. Reordering or changing a line intentionally changes its cache identity and output seed.

## Character assignments

| Character ID | Voice ID | Reference audio | BLAKE3 | Seed base | Preset | Status |
|---|---|---|---|---:|---|---|
| `mara` | `mara_gepard_nurisa_en` | `runtimes/gepard/ref_audio/nurisa_en.wav` | `df8803e3036b3f79771f8fde2cc62022d8d3c91305b589623ef1b72f0349b923` | 42001 | default | Temporary; requires performance approval |
| `ellis` | `ellis_gepard_audio_en` | `runtimes/gepard/ref_audio/audio_en.wav` | `5ffbd005cf5d871d787ebb84619d31672f2c9044b088735efb94efe66359063d` | 42002 | default | Temporary; requires performance approval |
| `voss` | `voss_gepard_ulan_emo` | `runtimes/gepard/ref_audio/ulan_emo.wav` | `182b17b8af341ce1a7b7e6f83dc5ce2268f3d4a782cf386e47201e9c900e8179` | 42003 | default | Temporary; requires performance approval |
| `nox` | `nox_gepard_audio_ru` | `runtimes/gepard/ref_audio/audio_ru.wav` | `546aae6a8339a772f5ddd77334e01b8e841fef42a38e1b2c7197fa57e35f1889` | 42004 | default | Temporary; requires performance approval |

## Change control

Changing the model, codec, runtime version, voice ID, reference file or hash, seed, normalized text, or any preset field creates a different provider-specific cache key. A production Gepard failure is fatal; production never substitutes espeak or the estimating provider. Espeak remains available only through the explicit diagnostic/preview path.
