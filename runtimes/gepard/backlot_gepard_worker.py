"""Batch Gepard worker owned by Infinite Backlot.

Loads Gepard and NanoCodec once, renders every request, writes one durable JSON
response file, and exits so the Rust runtime manager can reclaim VRAM.
"""

from __future__ import annotations

import argparse
import json
import random
import time
from pathlib import Path

import numpy as np
import soundfile as sf
import torch

from gepard_inference import GepardSession, SessionConfig


def emit(phase: str, **fields) -> None:
    print(json.dumps({"phase": phase, **fields}, separators=(",", ":")), flush=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model-root", required=True)
    parser.add_argument("--requests", required=True)
    parser.add_argument("--responses", required=True)
    parser.add_argument("--device", default=None)
    args = parser.parse_args()

    requests = json.loads(Path(args.requests).read_text(encoding="utf-8"))
    if not isinstance(requests, list):
        raise ValueError("request file must contain a JSON array")

    config = SessionConfig(
        checkpoint=str(Path(args.model_root).resolve()),
        defaults={
            "temperature": 0.3,
            "top_k": 0,
            "cfg_scale": 1.0,
            "cfg_frames": 0,
            "stop_threshold": 0.5,
            "max_frames": 2000,
            "repetition_penalty": 1.0,
            "repetition_window": 32,
        },
    )
    load_started = time.perf_counter()
    emit("tts.load.started", request_count=len(requests))
    session = GepardSession(config, device=args.device).load()
    emit("tts.load.completed", elapsed_ms=round((time.perf_counter() - load_started) * 1000))

    responses = []
    for request in requests:
        started = time.perf_counter()
        seed = int(request.get("seed", 0))
        random.seed(seed)
        np.random.seed(seed & 0xFFFFFFFF)
        torch.manual_seed(seed)
        if torch.cuda.is_available():
            torch.cuda.manual_seed_all(seed)
        preset = dict(request.get("preset") or {})
        preset.pop("seed", None)
        output = Path(request["output"])
        output.parent.mkdir(parents=True, exist_ok=True)
        sample_rate, waveform = session.synthesize(
            request["text"],
            reference=request.get("reference_audio"),
            **preset,
        )
        sf.write(output, waveform, sample_rate, format="WAV")
        duration = float(len(waveform)) / float(sample_rate)
        response = {
            "id": request["id"],
            "output": str(output.resolve()),
            "sample_rate": int(sample_rate),
            "duration": duration,
            "elapsed_ms": round((time.perf_counter() - started) * 1000),
            "success": True,
        }
        responses.append(response)
        emit("tts.line.completed", **response)

    response_path = Path(args.responses)
    response_path.parent.mkdir(parents=True, exist_ok=True)
    response_path.write_text(json.dumps(responses, indent=2), encoding="utf-8")
    emit("tts.complete", response_count=len(responses))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

