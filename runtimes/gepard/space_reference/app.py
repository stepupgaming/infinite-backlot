"""GEPARD — text-to-speech inference Space (ZeroGPU).

Startup order is load-bearing:
  1. ``create_env.setup_dependencies()`` re-pins transformers BEFORE any
     ML import — the Space image ships whatever NeMo pinned at build time,
     but the Gepard checkpoint requires transformers 5.3.0.
  2. ``spaces`` is imported before torch so ZeroGPU can patch CUDA calls.
  3. The engine (model + codec + speakers) is built ONCE at module level;
     ZeroGPU replays the recorded ``.to("cuda")`` moves when a GPU is
     attached, so no per-request reloading happens.
"""

from create_env import setup_dependencies

setup_dependencies()

try:  # ZeroGPU runtime (present on HF Spaces)
    import spaces  # noqa: E402

    _gpu_decorator = spaces.GPU
except ImportError:  # local run — no-op stand-in
    def _gpu_decorator(*args, **kwargs):
        if args and callable(args[0]):
            return args[0]

        def _wrap(fn):
            return fn

        return _wrap

from pathlib import Path  # noqa: E402

import gradio as gr  # noqa: E402

from gepard_inference.engine import AppConfig, GenerationParams, GepardEngine  # noqa: E402
from interface import MODE_PRESET, GepardInterface  # noqa: E402

CONFIG_PATH = Path(__file__).parent / "config.yaml"

config = AppConfig.from_yaml(CONFIG_PATH)
engine = GepardEngine(config).load()


@_gpu_decorator(duration=config.gpu_duration)
def synthesize(
    mode: str,
    speaker: str,
    ref_audio: str,
    text: str,
    temperature: float,
    top_k: float,
    max_frames: float,
    repetition_penalty: float,
    repetition_window: float,
):
    """Gradio handler: resolve the reference voice, then generate speech.

    Runs inside the ZeroGPU context — both the reference encoding (clone
    mode) and the autoregressive generation share one GPU session.
    """
    if not (text or "").strip():
        raise gr.Error("Please enter some text to synthesize.")

    if mode == MODE_PRESET:
        if not speaker:
            raise gr.Error("Please choose a preset speaker.")
        ref_codes = engine.speakers.codes(speaker)
    else:
        if not ref_audio:
            raise gr.Error("Please record or upload a reference clip.")
        ref_codes = engine.encode_reference(ref_audio)

    params = GenerationParams(
        temperature=temperature,
        top_k=int(top_k),
        # CFG is not exposed in the UI: strength comes from config defaults and
        # the runner's length gate decides whether it applies at all.
        cfg_scale=config.defaults.cfg_scale,
        cfg_frames=config.defaults.cfg_frames,
        stop_threshold=config.defaults.stop_threshold,
        max_frames=int(max_frames),
        repetition_penalty=repetition_penalty,
        repetition_window=int(repetition_window),
    )
    return engine.synthesize(text, ref_codes, params)


demo = GepardInterface(
    config=config,
    speaker_names=engine.speakers.names,
    synthesize_fn=synthesize,
).build()

if __name__ == "__main__":
    demo.launch()
