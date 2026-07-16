"""Build deterministic one-shot production SFX for the Odd Hours vertical slice."""
from __future__ import annotations

import json
import math
import wave
from pathlib import Path

import numpy as np

RATE = 48_000
DURATION = 17.7


def add_tone(buffer: np.ndarray, time: float, duration: float, frequency: float, amplitude: float, decay: float = 5.0) -> None:
    start = int(time * RATE)
    count = min(int(duration * RATE), len(buffer) - start)
    if count <= 0:
        return
    t = np.arange(count, dtype=np.float64) / RATE
    envelope = np.sin(np.minimum(t / 0.012, 1.0) * math.pi / 2.0) * np.exp(-decay * t)
    buffer[start:start + count] += amplitude * np.sin(2.0 * math.pi * frequency * t) * envelope


def add_noise(buffer: np.ndarray, time: float, duration: float, amplitude: float, seed: int, decay: float = 18.0) -> None:
    start = int(time * RATE)
    count = min(int(duration * RATE), len(buffer) - start)
    if count <= 0:
        return
    rng = np.random.default_rng(seed)
    t = np.arange(count, dtype=np.float64) / RATE
    noise = rng.standard_normal(count)
    # Cheap low-pass removes static-like high-frequency energy; these are transient foley hits.
    filtered = np.convolve(noise, np.ones(18) / 18.0, mode="same")
    buffer[start:start + count] += amplitude * filtered * np.exp(-decay * t)


def main() -> None:
    root = Path(__file__).resolve().parents[2]
    output = root / "output" / "production-vertical-slice"
    output.mkdir(parents=True, exist_ok=True)
    plan = json.loads((output / "production_plan.json").read_text(encoding="utf-8"))
    authored = {cue["id"]: cue for cue in plan["audio_cues"]}
    latch_time = float(authored["door_latch"]["time"])
    door_start = float(authored["door_movement"]["start"])
    door_end = float(authored["door_movement"]["end"])
    chime_time = float(authored["store_chime"]["time"])
    pickup_time = float(authored["package_pickup"]["time"])
    mono = np.zeros(int(DURATION * RATE), dtype=np.float64)
    cues = []
    for index, time in enumerate([0.48, 1.05, 1.62, 2.21, 2.80, 3.39, 3.93, 7.48, 8.02, 8.58, 9.35, 10.02, 10.69, 11.36, 12.03, 12.62, 13.16]):
        add_tone(mono, time, 0.14, 78.0 + (index % 2) * 12.0, 0.055, 20.0)
        add_noise(mono, time, 0.09, 0.045, 1000 + index, 28.0)
        cues.append({"id": f"footstep_{index:02d}", "time": time})
    add_noise(mono, latch_time, 0.12, 0.12, 2201, 32.0)
    add_tone(mono, latch_time, 0.16, 320.0, 0.09, 18.0)
    cues.append({"id": "door_latch", "time": latch_time})
    for index in range(13):
        time = door_start + index * ((door_end - door_start) / 13.0)
        add_tone(mono, time, 0.13, 118.0 + index * 2.4, 0.018, 12.0)
    cues.append({"id": "door_movement", "start": door_start, "end": door_end})
    for frequency, amplitude in [(880.0, 0.10), (1320.0, 0.07), (1760.0, 0.035)]:
        add_tone(mono, chime_time, 1.05, frequency, amplitude, 3.2)
    add_tone(mono, chime_time + 0.24, 0.90, 1046.5, 0.065, 3.8)
    cues.append({"id": "entrance_chime", "time": chime_time})
    add_noise(mono, pickup_time, 0.16, 0.10, 3301, 22.0)
    add_tone(mono, pickup_time, 0.20, 210.0, 0.07, 14.0)
    cues.append({"id": "package_pickup", "time": pickup_time})
    peak = float(np.max(np.abs(mono)))
    if peak > 0.92:
        mono *= 0.92 / peak
    stereo = np.stack([mono, mono], axis=1)
    pcm = np.int16(np.clip(stereo, -1.0, 1.0) * 32767)
    wav_path = output / "odd_hours_scene_audio.wav"
    with wave.open(str(wav_path), "wb") as handle:
        handle.setnchannels(2)
        handle.setsampwidth(2)
        handle.setframerate(RATE)
        handle.writeframes(pcm.tobytes())
    (output / "audio_provenance.json").write_text(json.dumps({
        "schema_version": 1,
        "generator": "tools/audio/build_odd_hours_sfx.py",
        "description": "Deterministic transient foley only; no synthetic hum, static bed, music, or dialogue.",
        "sample_rate": RATE,
        "duration_seconds": DURATION,
        "cues": cues,
    }, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"wav": str(wav_path), "duration": DURATION, "peak": peak, "cues": len(cues)}))


if __name__ == "__main__":
    main()
