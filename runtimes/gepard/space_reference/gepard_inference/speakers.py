"""Preset speaker library.

Preset voices are stored as pre-encoded codec tokens (``.pt`` files), not as
audio: each file carries the unfolded reference codes ``[1, T_ref, C_total]``
that the voice-cloning compressor consumes directly. This skips the codec
encode pass entirely for preset speakers.

File format (torch.save):
    {
        "name":        str,            # display name
        "ref_codes":   LongTensor,     # [1, T_ref, C_total] unfolded codes
        "sample_rate": int,            # source sample rate (metadata)
        "source":      str,            # original audio filename (metadata)
    }
A bare tensor is also accepted for forward compatibility.
"""

from __future__ import annotations

from pathlib import Path
from typing import Dict, List

import torch


class SpeakerLibrary:
    """Loads and caches preset speaker reference codes from ``.pt`` files.

    Args:
        root: Directory that relative speaker paths are resolved against.
        mapping: ``{display_name: path_to_pt}`` from the app config.
    """

    def __init__(self, root: Path, mapping: Dict[str, str]):
        self._root = Path(root)
        self._paths: Dict[str, Path] = {
            name: (self._root / p if not Path(p).is_absolute() else Path(p))
            for name, p in mapping.items()
        }
        self._cache: Dict[str, torch.Tensor] = {}

    @property
    def names(self) -> List[str]:
        """Display names in config order."""
        return list(self._paths.keys())

    def codes(self, name: str) -> torch.Tensor:
        """Reference codes ``[1, T_ref, C_total]`` (long, CPU) for a speaker."""
        if name not in self._paths:
            raise KeyError(f"unknown speaker {name!r}; available: {self.names}")
        if name not in self._cache:
            payload = torch.load(self._paths[name], map_location="cpu", weights_only=True)
            codes = payload["ref_codes"] if isinstance(payload, dict) else payload
            if codes.dim() != 3:
                raise ValueError(
                    f"speaker {name!r}: expected ref_codes [1, T, C], got {tuple(codes.shape)}"
                )
            self._cache[name] = codes.long()
        return self._cache[name]

    def preload(self) -> None:
        """Eagerly load every speaker (fail fast at startup)."""
        for name in self.names:
            self.codes(name)
