# SFX provenance

These are deterministic in-project one-shot effects authored for Infinite Backlot. They contain no third-party samples and may be distributed under the repository's license.

The renderer creates a file only when the exact semantic/gain/sample-rate cache key is absent. Source implementation: `backlot_core::render::ensure_semantic_sfx`.

| File | Purpose | Recipe | Duration | SHA-256 |
|---|---|---|---:|---|
| `elevator_ding_042_22050.wav` | Elevator arrival | Decaying 880 Hz fundamental plus 1320 Hz partial; short attack/release envelope; gain 0.42 | 0.90 s | `0b86474755f60ef544b18377cc5761b9865613a0284a8dc7c50f2917ee254527` |
| `panel_beep_036_22050.wav` | Panel activation | 1180 Hz sine one-shot; short attack/release envelope; gain 0.36 | 0.28 s | `97e569aae65328678161bd4adfa97d7c58f3f1bf5c50000fb5c519ed72b752f9` |
| `door_motor_028_22050.wav` | Elevator door movement | 82–100 Hz swept motor tone plus seeded low-level noise; short attack/release envelope; gain 0.28 | 1.60 s | `2956558225f61a53134a7f847abe6906ea4113d4cbd2eb0a8ab4a39f800937a3` |
| `door_motor_030_22050.wav` | Episode 0001 elevator opening | 74 Hz motor tone, 116 Hz harmonic, and deterministic low-level mechanical texture; gain 0.30 | 1.20 s | `d334f0404167e2f0b9d928ef6eaf306be1119d8fadcb72ee73885ad0772d00d3` |
| `door_slam_044_22050.wav` | Episode 0001 closing payoff | Short deterministic low-frequency impact plus decaying seeded transient; gain 0.44 | 0.35 s | `e2c920279f79fb1e8c08560cbd6e67d70d0f468a6e81292a545ad19df3c1db13` |

All listed effects are mono 22,050 Hz PCM WAV files. They are scheduled as occasional synchronized effects, not used as an always-on bed. The renderer rejects the former `impossible_floor_ambience` semantic; silence is the default background when no licensed music or room tone is available.
