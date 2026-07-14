#!/usr/bin/env python3
"""NeMo ASR transcription with word-level timestamps.

Defaults to nvidia/parakeet-tdt-0.6b-v2 from local cache.
Keeps nvidia/nemotron-3.5-asr-streaming-0.6b available as an explicit option.
Works around Windows path and tempfile issues.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
import warnings
from pathlib import Path

import tempfile
_original_temporarydirectory = tempfile.TemporaryDirectory
class _WindowsSafeTemporaryDirectory(_original_temporarydirectory):
    def __init__(self, suffix=None, prefix=None, dir=None):
        super().__init__(suffix=suffix, prefix=prefix, dir=dir)
        self._ignore_cleanup_errors = True
tempfile.TemporaryDirectory = _WindowsSafeTemporaryDirectory

import torch
import nemo.collections.asr as nemo_asr

warnings.filterwarnings("ignore")

DEFAULT_NEMOTRON_MODEL = "nvidia/nemotron-3.5-asr-streaming-0.6b"
PARAKEET_V2_MODEL = "nvidia/parakeet-tdt-0.6b-v2"
MODEL_ALIASES = {
    "nemotron": DEFAULT_NEMOTRON_MODEL,
    "nemotron-3.5": DEFAULT_NEMOTRON_MODEL,
    "nemotron-3.5-asr": DEFAULT_NEMOTRON_MODEL,
    "nemotron-3.5-asr-streaming-0.6b": DEFAULT_NEMOTRON_MODEL,
    "parakeet": PARAKEET_V2_MODEL,
    "parakeet-v2": PARAKEET_V2_MODEL,
    "parakeet-tdt-0.6b-v2": PARAKEET_V2_MODEL,
}


def _resolve_path(p: str) -> str:
    """Convert MSYS/POSIX paths to Windows paths for Python file ops."""
    if p.startswith("/c/"):
        return "C:\\" + p[3:].replace("/", "\\")
    if p.startswith("/d/"):
        return "D:\\" + p[3:].replace("/", "\\")
    if p.startswith("/f/"):
        return "F:\\" + p[3:].replace("/", "\\")
    if p.startswith("/") and len(p) > 2 and p[2] == "/":
        return p[1].upper() + ":" + p[2:].replace("/", "\\")
    return p


def probe_duration_seconds(input_path: str):
    try:
        raw = subprocess.check_output(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration",
             "-of", "default=nw=1:nk=1", input_path],
            text=True, timeout=10,
        )
        duration = float(raw.strip())
        return duration if duration > 0 else None
    except Exception:
        return None


def has_audio_stream(input_path: str) -> bool:
    try:
        raw = subprocess.check_output(
            [
                "ffprobe", "-v", "error",
                "-select_streams", "a",
                "-show_entries", "stream=index",
                "-of", "csv=p=0",
                input_path,
            ],
            text=True, timeout=10,
        )
        return bool(raw.strip())
    except Exception:
        return True


def write_empty_payload(output_path: str, audio_path: str, args, model_id: str, device: str, reason: str):
    media_duration = probe_duration_seconds(audio_path)
    payload = {
        "text": "",
        "words": [],
        "wordCount": 0,
        "durationSeconds": media_duration or 0,
        "metadata": {
            "provider": "nemo-asr",
            "modelId": model_id,
            "device": device,
            "targetLang": args.target_lang,
            "stripLangTags": args.strip_lang_tags,
            "timestampsRequested": args.timestamps,
            "torchVersion": torch.__version__,
            "cudaAvailable": torch.cuda.is_available(),
            "timestampScale": 1.0,
            "mediaDurationSeconds": media_duration,
            "skipped": True,
            "skipReason": reason,
        },
    }
    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(payload, f, ensure_ascii=False, indent=2)
    print(f"Wrote empty transcript to {output_path}: {reason}", file=sys.stderr)
    print(json.dumps(payload["metadata"]))


def prepare_audio(input_path: str):
    """Convert any audio/video to mono 16kHz wav for NeMo ASR."""
    path = Path(input_path)
    tmp = tempfile.NamedTemporaryFile(prefix="parakeet-", suffix=".wav", delete=False)
    tmp.close()
    subprocess.check_call(
        ["ffmpeg", "-y", "-i", str(path), "-vn", "-ar", "16000", "-ac", "1", "-f", "wav", tmp.name],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    return tmp.name, tmp.name


def resolve_model_id(model_id: str) -> str:
    return MODEL_ALIASES.get(model_id.strip().lower(), model_id)


def transcribe_with_model(model, prepared_path: str, args):
    kwargs = {
        "batch_size": args.batch_size,
        "return_hypotheses": True,
    }
    if args.timestamps:
        kwargs["timestamps"] = True

    audio_input = [prepared_path]
    if args.target_lang:
        kwargs["target_lang"] = args.target_lang
    if args.strip_lang_tags is not None:
        kwargs["strip_lang_tags"] = args.strip_lang_tags

    manifest_path = None
    if is_prompt_conditioned_model(model) and args.target_lang:
        manifest_path = write_prompt_manifest(prepared_path, args.target_lang)
        audio_input = [manifest_path]

    try:
        return model.transcribe(audio_input, **kwargs)
    except TypeError as exc:
        unsupported_prompt_args = (
            args.target_lang
            or args.strip_lang_tags is not None
        )
        if unsupported_prompt_args:
            print(
                f"Model transcribe() rejected language prompt args ({exc}); retrying without them.",
                file=sys.stderr,
            )
            kwargs.pop("target_lang", None)
            kwargs.pop("strip_lang_tags", None)
            return model.transcribe(audio_input, **kwargs)
        raise
    finally:
        if manifest_path:
            try:
                os.unlink(manifest_path)
            except OSError:
                pass


def is_prompt_conditioned_model(model) -> bool:
    prompt_dict = getattr(getattr(model, "cfg", None), "model_defaults", {}).get("prompt_dictionary")
    return bool(prompt_dict)


def write_prompt_manifest(prepared_path: str, target_lang: str) -> str:
    manifest = tempfile.NamedTemporaryFile(prefix="nemotron-manifest-", suffix=".json", delete=False, mode="w", encoding="utf-8")
    row = {
        "audio_filepath": prepared_path,
        "duration": probe_duration_seconds(prepared_path) or 0.0,
        "text": "",
        "lang": target_lang,
        "language": target_lang,
        "target_lang": target_lang,
    }
    json.dump(row, manifest, ensure_ascii=False)
    manifest.write("\n")
    manifest.close()
    return manifest.name


def extract_words(hypothesis):
    """Extract word-level timestamps from a NeMo RNNT Hypothesis object."""
    words = []
    ts = getattr(hypothesis, "timestamp", None)
    if ts is None and isinstance(hypothesis, dict):
        ts = hypothesis.get("timestamp")
    if not ts:
        return words

    raw_words = ts.get("word", ts.get("words", []))
    for idx, raw in enumerate(raw_words):
        if isinstance(raw, dict):
            text = raw.get("word", raw.get("text", ""))
            start = raw.get("start", raw.get("start_offset", raw.get("start_time", 0)))
            end = raw.get("end", raw.get("end_offset", raw.get("end_time", start)))
        else:
            text = getattr(raw, "word", getattr(raw, "text", ""))
            start = getattr(raw, "start", getattr(raw, "start_offset", 0))
            end = getattr(raw, "end", getattr(raw, "end_offset", start))
        text = str(text).strip()
        if text:
            words.append({
                "id": f"w{idx}",
                "text": text,
                "start": round(float(start), 3),
                "end": round(float(end), 3),
            })
    return words


def extract_text(hypothesis, words):
    for attr in ("text", "transcript"):
        value = getattr(hypothesis, attr, None)
        if isinstance(value, str) and value.strip():
            return value.strip()
    if isinstance(hypothesis, dict):
        for key in ("text", "transcript"):
            value = hypothesis.get(key)
            if isinstance(value, str) and value.strip():
                return value.strip()
    if isinstance(hypothesis, str):
        return hypothesis.strip()
    return " ".join(w["text"] for w in words)


def main():
    parser = argparse.ArgumentParser(description="NeMo ASR transcription")
    parser.add_argument("audio", help="Path to audio/video file")
    parser.add_argument("-o", "--output", required=True, help="Output JSON path")
    parser.add_argument("--model-id", default=PARAKEET_V2_MODEL)
    parser.add_argument("--device", default="cuda")
    parser.add_argument("--batch-size", type=int, default=1)
    parser.add_argument("--cache-dir", default="C:/Users/Steve/.cache/huggingface")
    parser.add_argument(
        "--target-lang",
        default="en-US",
        help='Nemotron target language such as en-US, es-ES, de-DE, or "auto".',
    )
    parser.add_argument(
        "--strip-lang-tags",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Strip Nemotron language tags from transcript text when supported.",
    )
    parser.add_argument(
        "--no-timestamps",
        dest="timestamps",
        action="store_false",
        help="Disable timestamp request for models that do not support it.",
    )
    args = parser.parse_args()

    audio_path = _resolve_path(args.audio)
    output_path = _resolve_path(args.output)
    model_id = resolve_model_id(args.model_id)

    if not os.path.exists(audio_path):
        print(f"Error: file not found: {audio_path}", file=sys.stderr)
        sys.exit(1)

    os.environ.setdefault("HF_HOME", args.cache_dir)
    os.environ.setdefault("HUGGINGFACE_HUB_CACHE", str(Path(args.cache_dir) / "hub"))

    device = args.device
    if device == "auto":
        device = "cuda" if torch.cuda.is_available() else "cpu"

    if not has_audio_stream(audio_path):
        write_empty_payload(output_path, audio_path, args, model_id, device, "no audio stream")
        return

    print(f"Loading model {model_id} on {device}...", file=sys.stderr)
    model = nemo_asr.models.ASRModel.from_pretrained(model_name=model_id)
    model = model.to(device)
    model.eval()

    prepared_path, tmp_path = prepare_audio(audio_path)

    print(f"Transcribing: {prepared_path}", file=sys.stderr)
    with torch.inference_mode():
        outputs = transcribe_with_model(model, prepared_path, args)

    if tmp_path:
        try:
            os.unlink(tmp_path)
        except OSError:
            pass

    hyp = outputs[0]
    words = extract_words(hyp)
    text = extract_text(hyp, words)
    media_duration = probe_duration_seconds(audio_path)
    raw_duration = max([w["end"] for w in words], default=0)
    scale = (media_duration / raw_duration) if (media_duration and raw_duration > media_duration * 1.5) else 1.0

    if scale != 1.0:
        for w in words:
            w["start"] = round(w["start"] * scale, 3)
            w["end"] = round(w["end"] * scale, 3)

    payload = {
        "text": text,
        "words": words,
        "wordCount": len(words),
        "durationSeconds": media_duration or raw_duration,
        "metadata": {
            "provider": "nemo-asr",
            "modelId": model_id,
            "device": device,
            "targetLang": args.target_lang,
            "stripLangTags": args.strip_lang_tags,
            "timestampsRequested": args.timestamps,
            "torchVersion": torch.__version__,
            "cudaAvailable": torch.cuda.is_available(),
            "timestampScale": scale,
            "mediaDurationSeconds": media_duration,
        },
    }

    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(payload, f, ensure_ascii=False, indent=2)

    print(f"Wrote {len(words)} words to {output_path}", file=sys.stderr)
    print(json.dumps(payload["metadata"]))


if __name__ == "__main__":
    main()
