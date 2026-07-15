# Performance Stack Roadmap

## Current and intended ownership

| Layer | Responsibility | Status |
|---|---|---|
| Gemma | Story, semantic direction, scene intent, performance language | Current |
| Navigation and scene blocking | Collision-free routes, portals, floors, reservations, destinations, camera/reveal corridors | Implemented foundation |
| Kimodo | Offline full-body motion, prompt sequences, root control, full-body and end-effector constraints, candidate generation | Production backend |
| MotionBricks | Potential future real-time locomotion, in-betweening, and smart-object proxy-keyframe execution | Deferred adapter |
| ARDY | Potential future streaming text control, continuation, and long-horizon replanning | Deferred pending SOMA/Windows proof |
| Gepard | Character voice generation | Current and unchanged |
| Audio2Face-3D | Future lip sync, jaw, tongue, eyes, and emotional facial performance | Near-term future pass |
| Bevy | Authoritative world state, runtime assembly, animation playback, interactions, cameras, GPU render | Current and unchanged |

The LLM requests semantic destinations and interactions. It does not draw geometric paths. Navigation resolves and validates a route. The motion backend performs that route. Runtime validation rejects corridor, floor, obstacle, contact, or portal violations.

## Audio2Face-3D near-term pass

NVIDIA's current Audio2Face-3D stack supports prerecorded and streaming audio, regression and diffusion models, emotional conditioning, and outputs that can drive direct geometry, joint transforms, or blendshape weights. Official SDK support includes Windows and Linux, CUDA/TensorRT acceleration, CPU fallback, batch/interactive execution, skin/tongue/teeth/jaw/eye processing, and optional GPU or CPU blendshape solving.

Audio2Face-3D should enter after Infinite Backlot has one stable production character. It must not block the present body-motion delivery.

### Prerequisites

1. **Stable production topology** — locked face mesh identity and neutral pose, with a clear versioning policy.
2. **Facial rig** — facial joints and/or blendshapes sufficient for phonemes and expression.
3. **Jaw and teeth control** — transform mapping for jaw, teeth, and mouth anchors.
4. **Eye controls** — bilateral eye rotation and saccade-compatible controls.
5. **Expression mapping** — a semantic mapping from Audio2Emotion/A2F channels to the character's controls.
6. **Output mapping** — choose geometry deformation, joint transforms, or blendshape solve; record model card, neutral mesh, scale, axes, and identity index.
7. **Bevy playback** — timestamped facial tracks, interpolation, audio sync, GPU morph-target or joint playback, and deterministic shot offsets.
8. **Character/style model strategy** — initially evaluate a compatible pretrained identity; use custom training only when topology and performance direction are stable.
9. **Dataset strategy** — for custom training, collect clean 16 kHz dialogue plus frame-aligned facial geometry/blendshapes/transforms, identity metadata, emotions, languages, and held-out validation performances.
10. **Toolchain** — Windows SDK build requires Visual Studio 2022+, CMake, CUDA >=12.8/<13, TensorRT >=10.13/<11, Python 3.8–3.10 for support scripts, models, and adequate GPU memory.

### Proposed integration

`Gepard audio -> Audio2Emotion (optional) -> Audio2Face executor -> character-specific mapping -> versioned facial clip -> Bevy audio-synchronized playback`.

Store source audio checksum, model/version, identity, emotion inputs, mapping version, frame rate, post-process settings, and output checksum beside every facial clip. Body and face tracks share a shot timebase but remain replaceable artifacts.

## Next milestone

Choose and lock one production character with a facial rig, then run a single prerecorded Gepard line through the official Windows SDK. Deliver jaw/eye/blendshape tracks, a Bevy playback proof, A/V sync measurements, and a character mapping document before considering custom training.
