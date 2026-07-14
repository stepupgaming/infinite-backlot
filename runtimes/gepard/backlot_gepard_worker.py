"""Batch Gepard worker owned by Infinite Backlot.

Loads Gepard and NanoCodec once, renders every request, writes one durable JSON
response file, and exits so the Rust runtime manager can reclaim VRAM.
"""

from __future__ import annotations

import argparse
import contextlib
import json
import os
import random
import sys
import time
from pathlib import Path

import numpy as np
import soundfile as sf
import torch

from gepard_inference import GepardSession, SessionConfig


_PROGRESS_STREAM = None


def isolate_progress_stdout() -> None:
    """Reserve original stdout for JSONL and route all library output to stderr."""
    global _PROGRESS_STREAM
    sys.stdout.flush()
    sys.stderr.flush()
    progress_fd = os.dup(sys.stdout.fileno())
    os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
    _PROGRESS_STREAM = os.fdopen(
        progress_fd,
        "w",
        buffering=1,
        encoding=getattr(sys.stdout, "encoding", None) or "utf-8",
    )
    sys.stdout = sys.stderr


def emit(phase: str, **fields) -> None:
    print(
        json.dumps({"phase": phase, **fields}, separators=(",", ":")),
        file=_PROGRESS_STREAM or sys.stdout,
        flush=True,
    )


def gpu_memory(stage: str) -> None:
    if not torch.cuda.is_available():
        emit("tts.gpu_memory", stage=stage, device="cpu")
        return
    free, total = torch.cuda.mem_get_info()
    emit(
        "tts.gpu_memory",
        stage=stage,
        device=str(torch.cuda.current_device()),
        free_mb=round(free / (1024 * 1024)),
        total_mb=round(total / (1024 * 1024)),
        allocated_mb=round(torch.cuda.memory_allocated() / (1024 * 1024)),
        reserved_mb=round(torch.cuda.memory_reserved() / (1024 * 1024)),
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model-root", required=True)
    parser.add_argument("--requests", required=True)
    parser.add_argument("--responses", required=True)
    parser.add_argument("--device", default=None)
    args = parser.parse_args()
    isolate_progress_stdout()

    requests = json.loads(Path(args.requests).read_text(encoding="utf-8"))
    if not isinstance(requests, list):
        raise ValueError("request file must contain a JSON array")
    ids = [request.get("id") for request in requests]
    if any(not value for value in ids):
        raise ValueError("every request must contain a non-empty id")
    if len(ids) != len(set(ids)):
        raise ValueError("request ids must be unique")

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
    gpu_memory("before_load")
    # Keep stdout machine-readable JSONL. Upstream runner/codec diagnostics are
    # still retained by the caller in the worker stderr log.
    with contextlib.redirect_stdout(sys.stderr):
        session = GepardSession(config, device=args.device).load()
    emit("tts.load.completed", elapsed_ms=round((time.perf_counter() - load_started) * 1000))
    gpu_memory("after_load")

    responses = []
    for request in requests:
        started = time.perf_counter()
        output = Path(request.get("output", ""))
        try:
            seed = int(request.get("seed", 0))
            random.seed(seed)
            np.random.seed(seed & 0xFFFFFFFF)
            torch.manual_seed(seed)
            if torch.cuda.is_available():
                torch.cuda.manual_seed_all(seed)
            preset = dict(request.get("preset") or {})
            preset.pop("seed", None)
            output.parent.mkdir(parents=True, exist_ok=True)
            with contextlib.redirect_stdout(sys.stderr):
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
            emit("tts.line.completed", **response)
        except Exception as error:  # preserve a complete batch response before failing
            response = {
                "id": request["id"],
                "output": str(output.resolve()) if str(output) else "",
                "sample_rate": 0,
                "duration": 0.0,
                "elapsed_ms": round((time.perf_counter() - started) * 1000),
                "success": False,
                "error": f"{type(error).__name__}: {error}",
            }
            emit("tts.line.failed", **response)
        responses.append(response)

    response_path = Path(args.responses)
    response_path.parent.mkdir(parents=True, exist_ok=True)
    response_path.write_text(json.dumps(responses, indent=2), encoding="utf-8")
    failures = sum(not response["success"] for response in responses)
    gpu_memory("after_generation")
    emit("tts.complete", response_count=len(responses), failure_count=failures)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())

