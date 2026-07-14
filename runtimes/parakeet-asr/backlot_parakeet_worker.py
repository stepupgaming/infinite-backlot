#!/usr/bin/env python3
"""One-load Parakeet batch worker for Infinite Backlot word alignment."""

import argparse
import json
import os
import time
from pathlib import Path

import torch
import nemo.collections.asr as nemo_asr

from parakeet_transcribe import (
    extract_text,
    extract_words,
    prepare_audio,
    probe_duration_seconds,
)


def emit(phase: str, **fields) -> None:
    print(json.dumps({"phase": phase, **fields}, separators=(",", ":")), flush=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model-id", required=True)
    parser.add_argument("--requests", required=True)
    parser.add_argument("--responses", required=True)
    parser.add_argument("--device", default="cuda")
    args = parser.parse_args()

    requests = json.loads(Path(args.requests).read_text(encoding="utf-8"))
    load_started = time.perf_counter()
    emit("speech_alignment.load.started", request_count=len(requests))
    model = nemo_asr.models.ASRModel.from_pretrained(model_name=args.model_id)
    model = model.to(args.device)
    model.eval()
    emit(
        "speech_alignment.load.completed",
        elapsed_ms=round((time.perf_counter() - load_started) * 1000),
    )

    responses = []
    for request in requests:
        started = time.perf_counter()
        prepared, temporary = prepare_audio(request["audio"])
        try:
            with torch.inference_mode():
                outputs = model.transcribe(
                    [prepared], batch_size=1, return_hypotheses=True, timestamps=True
                )
            hypothesis = outputs[0]
            words = extract_words(hypothesis)
            duration = probe_duration_seconds(request["audio"]) or max(
                [word["end"] for word in words], default=0
            )
            payload = {
                "text": extract_text(hypothesis, words),
                "words": words,
                "wordCount": len(words),
                "durationSeconds": duration,
                "metadata": {
                    "provider": "nemo-asr",
                    "modelId": args.model_id,
                    "device": args.device,
                },
            }
            output = Path(request["output"])
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(json.dumps(payload, indent=2), encoding="utf-8")
            response = {
                "id": request["id"],
                "output": str(output.resolve()),
                "word_count": len(words),
                "elapsed_ms": round((time.perf_counter() - started) * 1000),
                "success": bool(words),
            }
            responses.append(response)
            emit("speech_alignment.line.completed", **response)
        finally:
            if temporary:
                try:
                    os.unlink(temporary)
                except OSError:
                    pass

    response_path = Path(args.responses)
    response_path.parent.mkdir(parents=True, exist_ok=True)
    response_path.write_text(json.dumps(responses, indent=2), encoding="utf-8")
    emit("speech_alignment.complete", response_count=len(responses))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
