"""Gepard inference package — a lean, self-contained extraction of the
inference-only classes from the `gepard-train` project.

Public surface:
    * :class:`~gepard_inference.engine.GepardEngine` — high-level facade that
      owns the TTS runner, the NeMo codec and the preset speaker library.
    * :class:`~gepard_inference.engine.AppConfig` — YAML-backed application
      configuration (checkpoint, codec, speakers, generation defaults).
    * :class:`~gepard_inference.runner.GepardRunner` — low-level autoregressive
      runner with text classifier-free guidance.

NOTE: importing this package pulls torch / transformers / NeMo. On a Hugging
Face Space the environment must be prepared first (``create_env.py``).
"""

from .engine import AppConfig, GepardEngine, GenerationParams
from .speakers import SpeakerLibrary

__all__ = ["AppConfig", "GepardEngine", "GenerationParams", "SpeakerLibrary"]
