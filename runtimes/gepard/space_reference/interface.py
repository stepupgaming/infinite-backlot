"""Gradio UI for the GEPARD TTS Space — layout, cheetah theme, event wiring.

The interface is deliberately free of model code: it receives a fully wired
``synthesize_fn`` callable (already wrapped with the ZeroGPU decorator by
``app.py``) and only handles presentation.
"""

from __future__ import annotations

from typing import Callable, List

import gradio as gr

from gepard_inference.engine import AppConfig

MODE_PRESET = "Preset speaker"
MODE_CLONE = "Clone a voice"

# Cheetah palette: sunlit fur (yellow→orange), hunting-red accents, savanna green.
_CSS = """
.gepard-header {
    text-align: center;
    padding: 1.1rem 0 0.4rem 0;
}
.gepard-header h1 {
    font-size: 2.6rem;
    font-weight: 800;
    margin: 0;
    background: linear-gradient(90deg, #eab308 0%, #f59e0b 30%, #ea580c 65%, #dc2626 100%);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    letter-spacing: 0.04em;
}
.gepard-header p {
    margin: 0.35rem 0 0 0;
    font-size: 1.02rem;
    color: var(--body-text-color-subdued);
}
.gepard-header .stripe {
    height: 4px;
    width: 240px;
    margin: 0.8rem auto 0 auto;
    border-radius: 2px;
    background: repeating-linear-gradient(
        90deg, #eab308 0 24px, #ea580c 24px 48px, #dc2626 48px 72px, #16a34a 72px 96px);
}
#generate-btn {
    background: linear-gradient(90deg, #f59e0b 0%, #ea580c 60%, #dc2626 100%);
    color: white;
    font-weight: 700;
    border: none;
}
#generate-btn:hover { filter: brightness(1.07); }

/* Slimmer sliders in Generation settings: thinner track + smaller thumb. */
input[type="range"] {
    height: 4px;
}
input[type="range"]::-webkit-slider-runnable-track { height: 4px; }
input[type="range"]::-moz-range-track { height: 4px; }
input[type="range"]::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    height: 13px;
    width: 13px;
    border-radius: 50%;
    margin-top: -4.5px;
}
input[type="range"]::-moz-range-thumb {
    height: 13px;
    width: 13px;
    border: none;
    border-radius: 50%;
}
"""


def build_theme() -> gr.themes.Base:
    """Bright cheetah-colored theme: orange primary, green secondary, warm neutrals."""
    return gr.themes.Soft(
        primary_hue=gr.themes.colors.orange,
        secondary_hue=gr.themes.colors.green,
        neutral_hue=gr.themes.colors.stone,
        font=[gr.themes.GoogleFont("Inter"), "ui-sans-serif", "system-ui", "sans-serif"],
    ).set(
        slider_color="*primary_500",
        checkbox_background_color_selected="*secondary_500",
        button_secondary_background_fill="*secondary_100",
        block_title_text_color="*primary_600",
        accordion_text_color="*primary_600",
    )


class GepardInterface:
    """Builds the Gradio Blocks app for GEPARD.

    Args:
        config: Application config (slider defaults, speaker names, limits).
        speaker_names: Preset speaker display names for the dropdown.
        synthesize_fn: Callable with the signature
            ``(mode, speaker, ref_audio, text, temperature, top_k,
              max_frames, repetition_penalty, repetition_window)
              -> (sr, np.ndarray)``. CFG and stop-threshold are not UI knobs;
            they are driven by config defaults (+ the runner's CFG length gate).
            Wrapped with the ZeroGPU decorator by the caller.
    """

    def __init__(
        self,
        config: AppConfig,
        speaker_names: List[str],
        synthesize_fn: Callable,
    ):
        self.config = config
        self.speaker_names = speaker_names
        self.synthesize_fn = synthesize_fn

    # ------------------------------------------------------------------
    # Layout
    # ------------------------------------------------------------------

    def build(self) -> gr.Blocks:
        """Assemble and return the Blocks app (call ``.launch()`` on it)."""
        d = self.config.defaults

        # Preset examples come from config (examples: section), already
        # validated against the speaker roster at load time. The table shows
        # the human-readable label; the label maps back to the speaker key.
        example_rows = [[ex.label, ex.text] for ex in self.config.examples]
        label_to_speaker = {ex.label: ex.speaker for ex in self.config.examples}

        def run_example(example_label: str, example_text: str):
            """Synthesize a preset example row with the default generation params.

            Clicking a row runs this once (in a ZeroGPU request context) and
            Gradio caches the audio, so later clicks just replay it. Returns the
            audio plus the resolved speaker key so the dropdown stays in sync.
            """
            speaker_name = label_to_speaker.get(example_label, example_label)
            audio = self.synthesize_fn(
                MODE_PRESET, speaker_name, None, example_text,
                d.temperature, d.top_k,
                d.max_frames, d.repetition_penalty, d.repetition_window,
            )
            return audio, speaker_name

        with gr.Blocks(theme=build_theme(), css=_CSS, title="GEPARD TTS") as demo:
            gr.HTML(
                """
                <div class="gepard-header">
                    <h1>🐆 GEPARD</h1>
                    <p>Fast, spotted text-to-speech model with voice cloning</p>
                    <div class="stripe"></div>
                </div>
                """
            )

            with gr.Row():
                with gr.Column(scale=5):
                    mode = gr.Radio(
                        choices=[MODE_PRESET, MODE_CLONE],
                        value=MODE_PRESET,
                        label="Voice source",
                        show_label=False,
                        info="Pick a bundled voice, or clone one from your own recording.",
                    )
                    speaker = gr.Dropdown(
                        # (label, value): show the human-readable label, keep
                        # the speaker key as the selected value.
                        choices=[
                            (self.config.speaker_labels.get(k, k), k)
                            for k in self.speaker_names
                        ],
                        value=self.speaker_names[0] if self.speaker_names else None,
                        label="Preset speaker",
                        show_label=False,
                        visible=True,
                    )
                    ref_audio = gr.Audio(
                        sources=["microphone", "upload"],
                        type="filepath",
                        label=f"Reference voice (≤ {int(self.config.max_ref_seconds)}s used)",
                        visible=False,
                    )
                    text = gr.Textbox(
                        label="Text",
                        show_label=False,
                        placeholder="Type what the cheetah should say…",
                        lines=4,
                    )

                    with gr.Accordion("Generation settings", open=False):
                        temperature = gr.Slider(
                            0.05, 1.0, value=d.temperature, step=0.05,
                            label="Temperature",
                            info="Sampling temperature; lower = more stable, higher = more varied.",
                        )
                        top_k = gr.Slider(
                            0, 200, value=d.top_k, step=1,
                            label="Top-k",
                            info="Keep only the k most likely tokens per head (0 = disabled).",
                        )
                        max_frames = gr.Slider(
                            43, 1075, value=d.max_frames, step=43,
                            label="Max frames",
                            info="Cap on generated frames (~21.5 fps; 43 frames ≈ 2 s, 1075 ≈ 50 s max).",
                        )
                        repetition_penalty = gr.Slider(
                            1.0, 1.5, value=d.repetition_penalty, step=0.01,
                            label="Repetition penalty",
                            info="Penalize recently used tokens (1.0 = disabled).",
                        )
                        repetition_window = gr.Slider(
                            0, 128, value=d.repetition_window, step=4,
                            label="Repetition window",
                            info="Recent frames tracked by the penalty (0 = full history).",
                        )

                with gr.Column(scale=4):
                    output_audio = gr.Audio(
                        label="Generated speech",
                        type="numpy",
                        interactive=False,
                    )
                    generate_btn = gr.Button(
                        "🐆 Generate speech", variant="primary", elem_id="generate-btn",
                    )

            if example_rows:
                # Hidden carrier: holds the clicked row's label so the examples
                # table can display labels while run_example maps them to keys.
                example_label = gr.Textbox(visible=False, label="Speaker")
                gr.Examples(
                    label="Examples",
                    examples=example_rows,
                    inputs=[example_label, text],
                    outputs=[output_audio, speaker],
                    fn=run_example,
                    cache_examples="lazy",
                )

            # Preset mode shows the dropdown; clone mode shows the recorder.
            mode.change(
                fn=self._toggle_mode,
                inputs=[mode],
                outputs=[speaker, ref_audio],
                show_progress="hidden",
            )

            generate_btn.click(
                fn=self.synthesize_fn,
                inputs=[
                    mode, speaker, ref_audio, text,
                    temperature, top_k,
                    max_frames, repetition_penalty, repetition_window,
                ],
                outputs=[output_audio],
            )

        return demo

    # ------------------------------------------------------------------
    # Events
    # ------------------------------------------------------------------

    @staticmethod
    def _toggle_mode(mode: str):
        """Show the speaker dropdown in preset mode, the recorder in clone mode."""
        is_preset = mode == MODE_PRESET
        return (
            gr.update(visible=is_preset),
            gr.update(visible=not is_preset),
        )
