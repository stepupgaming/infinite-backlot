# Purpose

Own the executable Bevy application, operator flow, episode state machine, real-time scene playback, and GPU capture path.

# Ownership

- `main.rs` parses CLI modes, chooses authoring/runtime ownership, wires resources, and assembles the Bevy app.
- `state.rs` owns application states and shared Bevy resources.
- `pipeline.rs` owns state transitions from loading through review and commit.
- `player.rs` executes resolved beats, navigation, camera, captions, and watchability behavior.
- `scene.rs` owns the interactive scene and HUD.
- `bevy_capture.rs` owns the real-GPU one-shot production/capture backend.
- `backlot_scene.rs` owns the shared Blender-set manifest, semantic GLB binding, authored camera/cutaway selection, dynamic set state, and explicit greybox mode.
- `examples/bevy_min.rs` is the minimal Bevy diagnostic scene.

# Local Contracts

- Keep model and authoring work off the Bevy main thread; communicate through explicit resources/channels and state transitions.
- Preserve explicit CLI mode semantics, especially production, diagnostic, reuse/repair, render backend, and require-LLM behavior.
- Normal `--produce-one` runs require `gepard_batch`; `--diagnostic-tts` is the only explicit espeak preview path, and `--tts-cache-bypass` forces diagnostic regeneration without changing authored content.
- GPU phases are sequential: frozen replay, complete Gepard batch and worker exit, artifact verification/timeline rebuild, ASR alignment and worker exit, then Bevy initialization/capture.
- Production success must reflect real authored, audio, render, package, and probe outcomes; do not report placeholder artifacts as complete.
- Keep state transitions deterministic and make error recovery visible rather than silently skipping failed work.
- Use `backlot-core` domain types and validators instead of recreating protocol or world rules in Bevy systems.

# Work Guidance

- Register new systems in the narrowest state and keep resource ownership obvious at the composition root.
- Treat capture, operator UI, and offline production as consumers of the same validated episode contract.
- Interactive playback and offline GPU capture must consume the same `BacklotScenePlugin` set contract. `assets/scenes/apartment_floor_03.glb` is the default; the procedural set is available only through `BACKLOT_SET_MODE=greybox`.
- Imported Blender cameras are semantic anchors only. Each consumer owns one active Bevy camera and validates/replaces shots before applying an authored anchor; character shots retain the authored eye/lens but retarget to the current subject position for portrait-safe framing.
- GPU capture applies interaction contact targeting in world space after retargeting; panel and prop actions must be checked in encoded frames rather than inferred from action metadata.

# Verification

- Run `cargo check -p backlot-app` for application changes.
- Run `cargo test` when shared behavior changes.
- Run `cargo run -p backlot-app --example bevy_min` for scene/render initialization changes and the exact production or diagnostic CLI mode for affected paths.

# Child DOX Index

No child DOX files yet.
