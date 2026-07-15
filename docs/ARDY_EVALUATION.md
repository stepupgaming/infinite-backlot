# ARDY Evaluation

## Evidence inspected

- Official repository: `https://github.com/nv-tlabs/ardy`, commit `693f74d13b3d04a0a22ce127ee79c929dd89756b`.
- Official README, model registry, constraints, demo, setup metadata, and command-line generator.
- Source-only Windows feasibility check: `python -m compileall` passed. No checkpoint was installed and no inference claim is made.

## Current release

ARDY 0.2.0 is an autoregressive diffusion runtime for interactive motion. It supports online prompt replacement, long-horizon continuation, replanning from the current frame, and kinematic constraints. Public checkpoints released July 10, 2026 are:

| Model family | Skeleton | FPS | Prediction horizon |
|---|---|---:|---:|
| ARDY-Core-RP | Core | 20 | 8 or 40 frames |
| ARDY-G1-RP | Unitree G1 | 25 | 8 or 52 frames |

A SOMA version is explicitly listed as coming soon. Therefore, current SOMA compatibility is **not available**.

## Control surface

The official interactive demo and APIs support:

- streaming text changes during playback;
- autoregressive continuation and `Restart From Now` replanning;
- initial root position and orientation;
- sparse 2D root waypoints;
- dense interpolated root trajectories;
- target root velocity and optional heading;
- full-body keyframes;
- sparse hand and foot position/orientation constraints;
- mixed hands+feet constraints;
- history/future cropping and replan buffers;
- optional motion correction for foot skating and constraint tracking;
- generated foot-contact output;
- deterministic seeds and multiple batch samples in the CLI;
- NPZ output, plus G1 MuJoCo-qpos CSV.

Constraint sampling from the public Bones SEED dataset currently supports G1. The underlying representation is broader, but this does not remove the need for a certified human rig path.

## Runtime cost and platform

- Official test platform: Ubuntu 22.04, RTX 4090, driver 575, Python 3.11.
- PyTorch >=2.4, CMake >=3.15, and a C++17 compiler are required for bundled motion correction.
- TensorRT 10.13 is optional and its cached engines are version-specific.
- Text encoding uses gated `Meta-Llama-3-8B-Instruct` through LLM2Vec.
- README guidance reports approximately 14 GB VRAM for CUDA/bfloat16 text encoding. CPU text encoding is supported but slower; a persistent text-encoder service avoids repeated loads.
- Source parses on Windows, but the official runtime is not claimed as Windows-tested. The C++ extension, TensorRT build, and interactive stack remain integration risks on this host.

## Compatibility with Infinite Backlot

The new `MotionAuthoringRequest` already represents ARDY's useful controls: prompt sequences, current/continuation pose, root path and waypoints, heading, sparse joint/end-effector constraints, contacts, candidates, and seeds. The navigation and smart-interaction systems stay authoritative and backend-neutral.

Current blockers are skeleton and runtime cost:

1. no released SOMA checkpoint;
2. Core/G1-to-production-rig retargeting and rest-axis certification;
3. LLM2Vec memory competing with Bevy and Blender on a 16 GB GPU;
4. Windows and TensorRT build validation;
5. continuation-state serialization and deterministic replan testing.

## How ARDY complements Kimodo

Kimodo remains the offline hero-motion backend. ARDY is a future candidate for:

- continuous ambient motion;
- prompt changes while a shot is playing;
- reactive locomotion around temporal reservations;
- short replans after a portal or actor state changes;
- long unbroken takes that are expensive to author as one offline diffusion clip.

ARDY should consume the same validated path corridor and interaction definitions and return the same `MotionCandidate`/evaluation record. It must not become the navigation planner.

**Recommendation:** monitor the official SOMA checkpoint. When available, run a separate Windows feasibility milestone with a persistent CPU text encoder and GPU motion model, then compare continuation seams, root-corridor error, contacts, and VRAM against Kimodo. Do not replace Kimodo in the current production path.
