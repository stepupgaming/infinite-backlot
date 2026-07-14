"""NeMo audio-codec wrapper used at inference time.

``UnfoldedCodecModel`` extends NeMo's ``AudioCodecModel`` with direct decoding
from per-dimension discrete FSQ codes (the format the Gepard model produces),
bypassing mixed-radix composition/decomposition.
"""
import torch
from omegaconf import open_dict
from nemo.collections.tts.models import AudioCodecModel


class UnfoldedCodecModel(AudioCodecModel):
    """AudioCodecModel + decoding from unfolded per-dimension FSQ codes.

    Works with any GroupFiniteScalarQuantizer configuration — the number of
    groups, dimensions per group, and FSQ levels are read from the model's
    vector_quantizer at runtime.
    """

    def __init__(self, cfg, trainer=None):
        # SLMDiscriminator downloads microsoft/wavlm-base-plus (~360MB) and is
        # only used during training — strip it from the config before init.
        with open_dict(cfg):
            disc = cfg.get("discriminator", None)
            if disc is not None and "discriminators" in disc:
                disc.discriminators = [
                    d for d in disc.discriminators if "SLM" not in d._target_
                ]
        super().__init__(cfg, trainer)

    def decode_from_codes(self, codes: torch.Tensor, codes_len: torch.Tensor):
        """Decode audio from unfolded per-dimension discrete codes.

        Args:
            codes: (B, D, T) — per-dimension discrete values, where
                   D = num_groups * dims_per_group.
            codes_len: (B,) — valid frame count per batch element.

        Returns:
            audio: (B, T_audio) — decoded waveform
            audio_len: (B,) — valid audio lengths in samples
        """
        num_levels = self.vector_quantizer.fsqs[0].num_levels.squeeze()
        scale = (num_levels // 2).float().to(codes.device)

        groups = codes.chunk(self.vector_quantizer.num_groups, dim=1)
        dequantized = torch.cat(
            [(g - scale[None, :, None]) / scale[None, :, None] for g in groups],
            dim=1,
        )

        return self.decode_audio(inputs=dequantized, input_len=codes_len)
