"""High-level TTS engine: model + codec + speakers behind one facade.

Designed for a Hugging Face ZeroGPU Space: everything heavy (checkpoint
download, weight loading, codec init) happens ONCE at process startup in
:meth:`GepardEngine.load`; per-request work is only the autoregressive
generation itself. On ZeroGPU the ``.to("cuda")`` calls issued at startup are
intercepted by the ``spaces`` package and replayed when a GPU is attached, so
the model is never reloaded between requests.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, NamedTuple, Optional, Tuple

import numpy as np
import torch
import yaml

from .runner import GepardRunner
from .speakers import SpeakerLibrary


class Example(NamedTuple):
    """A demo row for the UI examples table.

    Attributes:
        speaker: Preset speaker key (must exist under ``speakers:``); used for
            the codec lookup and to sync the dropdown.
        label: Human-readable text shown in the examples table (e.g.
            "Den from California"). Defaults to ``speaker`` when not given.
        text: Phrase to synthesize.
    """

    speaker: str
    label: str
    text: str


@dataclass
class GenerationParams:
    """User-tunable generation arguments (mirrors ``GepardRunner.generate``).

    ``cfg_frames == 0`` means "guide the whole utterance" and is translated to
    ``None`` for the runner.
    """

    temperature: float = 0.3
    top_k: int = 0
    cfg_scale: float = 3.0
    cfg_frames: int = 0
    stop_threshold: float = 0.5
    max_frames: int = 2000
    repetition_penalty: float = 1.0
    repetition_window: int = 32

    def to_generate_kwargs(self) -> dict:
        """Translate UI-facing values into ``GepardRunner.generate`` kwargs."""
        return {
            "temperature": float(self.temperature),
            "top_k": int(self.top_k),
            "cfg_scale": float(self.cfg_scale),
            "cfg_frames": int(self.cfg_frames) if int(self.cfg_frames) > 0 else None,
            "stop_threshold": float(self.stop_threshold),
            "max_frames": int(self.max_frames),
            "repetition_penalty": float(self.repetition_penalty),
            "repetition_window": int(self.repetition_window),
        }


@dataclass
class AppConfig:
    """Application configuration loaded from ``config.yaml``.

    Attributes:
        checkpoint: HF repo id (or local dir) of the self-describing checkpoint.
        attn_implementation: Backbone attention implementation for inference.
        codec_id: HF repo id of the NeMo audio codec.
        sample_rate: Codec sample rate (Hz).
        speakers: ``{speaker_key: path_to_pt}`` preset voices.
        speaker_labels: ``{speaker_key: display_label}`` human-readable names
            for the dropdown / examples (defaults to the key).
        examples: ``[Example(speaker, label, text), ...]`` demo rows for the UI
            (each speaker must be a key in ``speakers``).
        defaults: Default generation parameters (slider initial values).
        max_ref_seconds: Recorded/uploaded reference audio is truncated to
            this many seconds before encoding.
        gpu_duration: ZeroGPU time budget per request (seconds).
        cfg_max_text_tokens: Disable text-CFG when the input has more than this
            many text tokens (CFG helps short phrases but rushes/flattens long
            ones). None = never gate.
        root: Directory of the config file; relative paths resolve against it.
    """

    checkpoint: str
    attn_implementation: str = "eager"
    codec_id: str = "nvidia/nemo-nano-codec-22khz-1.89kbps-21.5fps"
    sample_rate: int = 22050
    speakers: Dict[str, str] = field(default_factory=dict)
    speaker_labels: Dict[str, str] = field(default_factory=dict)
    examples: List[Example] = field(default_factory=list)
    defaults: GenerationParams = field(default_factory=GenerationParams)
    max_ref_seconds: float = 60.0
    gpu_duration: int = 120
    cfg_max_text_tokens: Optional[int] = None
    root: Path = field(default_factory=Path.cwd)

    @classmethod
    def from_yaml(cls, path: str | Path) -> "AppConfig":
        """Load and validate the application config from a YAML file."""
        path = Path(path)
        with open(path) as f:
            raw = yaml.safe_load(f) or {}
        model = raw.get("model") or {}
        codec = raw.get("codec") or {}
        app = raw.get("app") or {}
        generation = raw.get("generation") or {}
        if not model.get("checkpoint"):
            raise ValueError(f"{path}: model.checkpoint is required")
        cfg_max = generation.get("cfg_max_text_tokens")
        speakers, speaker_labels = cls._parse_speakers(raw.get("speakers"), path)
        examples = cls._parse_examples(raw.get("examples"), speakers, speaker_labels, path)
        return cls(
            checkpoint=str(model["checkpoint"]),
            attn_implementation=str(model.get("attn_implementation", "eager")),
            codec_id=str(codec.get("id", cls.codec_id)),
            sample_rate=int(codec.get("sample_rate", 22050)),
            speakers=speakers,
            speaker_labels=speaker_labels,
            examples=examples,
            defaults=GenerationParams(**(raw.get("defaults") or {})),
            max_ref_seconds=float(app.get("max_ref_seconds", 60.0)),
            gpu_duration=int(app.get("gpu_duration", 120)),
            cfg_max_text_tokens=int(cfg_max) if cfg_max is not None else None,
            root=path.parent.resolve(),
        )

    @staticmethod
    def _parse_speakers(raw_speakers, path) -> Tuple[Dict[str, str], Dict[str, str]]:
        """Normalize the ``speakers:`` section into ``(paths, labels)``.

        Each value may be either a path string (label defaults to the key) or a
        mapping with ``path`` and an optional human-readable ``label``:

            speakers:
              Den:                       # mapping form, custom label
                path: speakers/den.pt
                label: Den from California
              Nurisa: speakers/nurisa.pt  # string form, label = "Nurisa"
        """
        paths: Dict[str, str] = {}
        labels: Dict[str, str] = {}
        for key, val in (raw_speakers or {}).items():
            if isinstance(val, dict):
                p = val.get("path")
                label = val.get("label") or key
            else:
                p = val
                label = key
            if not p:
                raise ValueError(f"{path}: speaker {key!r} has no path")
            paths[str(key)] = str(p)
            labels[str(key)] = str(label)
        return paths, labels

    @staticmethod
    def _parse_examples(
        raw_examples, speakers: Dict[str, str], speaker_labels: Dict[str, str], path
    ) -> List[Example]:
        """Validate the ``examples:`` section into ``[Example, ...]``.

        Each entry must be a mapping with ``speaker`` (a known preset) and
        ``text``; an optional ``label`` overrides the string shown in the UI
        (defaults to the speaker's label). Malformed or unknown-speaker rows are
        skipped with a warning rather than failing the Space — examples are
        cosmetic.
        """
        examples: List[Example] = []
        for i, item in enumerate(raw_examples or []):
            if not isinstance(item, dict):
                print(f"[AppConfig] {path}: examples[{i}] is not a mapping; skipped")
                continue
            speaker = item.get("speaker")
            text = item.get("text")
            if not speaker or not text:
                print(f"[AppConfig] {path}: examples[{i}] missing speaker/text; skipped")
                continue
            if speaker not in speakers:
                print(f"[AppConfig] {path}: examples[{i}] unknown speaker {speaker!r}; skipped")
                continue
            label = item.get("label") or speaker_labels.get(speaker, speaker)
            examples.append(Example(str(speaker), str(label), str(text)))
        return examples


def runtime_device() -> torch.device:
    """The device inference will run on.

    On a ZeroGPU Space CUDA is not initialized in the main process, but the
    ``spaces`` package intercepts ``.to("cuda")`` calls — so "cuda" is the
    right answer whenever the Space is ZeroGPU-backed OR a real GPU is
    visible. Falls back to CPU for local GPU-less runs.
    """
    if os.environ.get("SPACES_ZERO_GPU"):
        return torch.device("cuda")
    return torch.device("cuda" if torch.cuda.is_available() else "cpu")


class GepardEngine:
    """Facade over the Gepard runner, the NeMo codec and the speaker library.

    Lifecycle:
        engine = GepardEngine(AppConfig.from_yaml("config.yaml"))
        engine.load()                       # once, at process startup
        ...
        sr, wave = engine.synthesize(text, ref_codes, params)   # per request
    """

    def __init__(self, config: AppConfig):
        self.config = config
        self.device = runtime_device()
        self.runner: Optional[GepardRunner] = None
        self.codec = None
        self.speakers = SpeakerLibrary(config.root, config.speakers)

    # ------------------------------------------------------------------
    # Startup
    # ------------------------------------------------------------------

    def load(self) -> "GepardEngine":
        """Load the model + codec once and move them to the runtime device."""
        from .codec_wrapper import UnfoldedCodecModel  # NeMo import is heavy

        self.runner = GepardRunner.from_checkpoint(
            self.config.checkpoint,
            device="cpu",  # weights land on CPU; moved below (ZeroGPU-friendly)
            attn_implementation=self.config.attn_implementation,
        )
        self.codec = UnfoldedCodecModel.from_pretrained(self.config.codec_id).eval()

        self.runner.model.to(self.device)
        self.codec.to(self.device)
        self.runner.device = self.device

        self.speakers.preload()
        print(f"[GepardEngine] ready on {self.device} | speakers: {self.speakers.names}")
        return self

    # ------------------------------------------------------------------
    # Per-request work (runs inside the GPU context on ZeroGPU)
    # ------------------------------------------------------------------

    @property
    def fsq_levels(self) -> list:
        """FSQ levels per codebook dimension, read from the loaded model."""
        return list(self.runner.model.ref_compressor.fsq_levels)

    def encode_reference(self, audio_path: str) -> torch.Tensor:
        """Encode a reference clip into unfolded codec codes ``[1, T, C_total]``.

        The clip is loaded mono at the codec sample rate and truncated to
        ``config.max_ref_seconds``.
        """
        import librosa

        wave_np, _ = librosa.load(audio_path, sr=self.config.sample_rate, mono=True)
        max_samples = int(self.config.max_ref_seconds * self.config.sample_rate)
        wave_np = wave_np[:max_samples]
        if wave_np.size == 0:
            raise ValueError("reference audio is empty")

        from .codec_ops import unfold_tokens

        wave = torch.from_numpy(wave_np).unsqueeze(0).to(self.device)
        wave_len = torch.tensor([wave.shape[-1]], device=self.device)
        with torch.inference_mode():
            tokens, _ = self.codec.encode(audio=wave, audio_len=wave_len)  # [1, C, T]
        ref_codes = (
            unfold_tokens(tokens.cpu(), self.fsq_levels).permute(0, 2, 1).contiguous()
        )  # [1, T, C_total]
        return ref_codes

    def synthesize(
        self,
        text: str,
        ref_codes: Optional[torch.Tensor],
        params: GenerationParams,
    ) -> Tuple[int, np.ndarray]:
        """Generate speech for ``text`` conditioned on ``ref_codes``.

        Returns ``(sample_rate, waveform)`` with the waveform as float32
        numpy — the format ``gr.Audio`` consumes directly.
        """
        if self.runner is None:
            raise RuntimeError("GepardEngine.load() must be called before synthesize()")
        text = (text or "").strip()
        if not text:
            raise ValueError("text is empty")

        tokens = self.runner.generate(
            text,
            ref_codes=ref_codes.to(self.device) if ref_codes is not None else None,
            cfg_max_text_tokens=self.config.cfg_max_text_tokens,
            **params.to_generate_kwargs(),
        )  # (num_heads, T)

        codes = tokens.unsqueeze(0).to(self.device)
        codes_len = torch.tensor([codes.shape[-1]], device=self.device)
        with torch.inference_mode():
            audio, _ = self.codec.decode_from_codes(codes, codes_len)
        wave = audio.float().cpu().detach().flatten().numpy()
        return self.config.sample_rate, wave
