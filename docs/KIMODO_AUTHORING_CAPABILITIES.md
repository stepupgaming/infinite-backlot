# Kimodo Authoring Capabilities

## Audited installation

- Local source: `runtimes/kimodo`, mirrored from `https://github.com/nv-tlabs/kimodo`.
- Production checkpoint: `F:/Models/Kimodo/Kimodo-SOMA-RP-v1.1` (weights remain outside Git).
- Production Python: `C:/Projects/gemmy/runtimes/kimodo/.venv/Scripts/python.exe`.
- Observed runtime: Python 3.11, PyTorch 2.7.1+cu128, CUDA available on the NVIDIA GeForce RTX 5060 Ti.
- Canonical performer skeleton: SOMA. The model conditions on SOMA30 and exports SOMA77-compatible motion for skinning/BVH.
- Model rate: 30 Hz.
- Audited sources: `kimodo/constraints.py`, `kimodo/demo/generation.py`, `kimodo/scripts/generate.py`, model registry, motion I/O, post-processing, all bundled SOMA/G1 demo metadata and constraints, and Backlot's batch/runtime adapters.

## Actual control surface

| Control | Runtime support | Backlot status after this pass |
|---|---|---|
| Single text prompt | Native | Supported |
| Timed prompt sequence | Native multi-prompt generation with per-prompt frame counts and transition frames | Rich authoring schema and batch adapter |
| Candidate count | Native `num_samples`; Backlot executes explicit per-candidate seeds in one model-loaded worker | Bounded 1–4 with persisted seeds and metrics |
| Deterministic seed | `seed_everything` | Persisted per request/candidate |
| Dense root path | `Root2DConstraintSet` on every frame | Generated from validated navigation corridor |
| Sparse root waypoints | Same set on sparse frame indices | Supported |
| Initial/final root transform | Root constraints on first/final frames | Supported through generated root constraints |
| Heading constraints | `global_root_heading` on root frames | Supported by adapter |
| Full-body keyframes | `FullBodyConstraintSet` fixes all global joint positions plus root/heading | Supported from SOMA reference poses |
| Hand/foot position | Left/right hand/foot constraint classes | Supported |
| Hand/foot rotation | End-effector sets feed global joint rotation constraints | Supported |
| Mixed constraints | Multiple constraint sets in one call | Supported; official mixed example was inspected |
| Motion in-betweening | Diffusion between sparse constrained frames | Supported |
| Curved paths | Dense or sparse X/Z root trajectories | Supported; navigation generates curves |
| Contact metadata | Output predicts `foot_contacts`; contacts are not a first-class text event input | Output persisted; authored events remain Backlot metadata |
| Foot-contact prediction | Native output channel | Persisted and scored |
| Motion postprocess | Native SOMA post-processing reduces foot skating | Enabled for SOMA |
| Root correction | Separate bundled MotionCorrection C++/Python trajectory corrector | Available as bounded post-generation correction, not used to excuse unsafe routes |
| Export | Kimodo NPZ; SOMA BVH; SMPL-X AMASS NPZ; G1 MuJoCo CSV | NPZ/BVH plus Backlot motion sidecars |
| Environment/object constraints | No first-class mesh/object collision solver in Kimodo | Backlot resolves object-relative intent into root/end-effector constraints before inference |
| Continuation from current pose | Can be represented by an initial full-body keyframe; no audited stateful streaming API | Request schema supports a source pose; offline generation remains clip-based |
| Batch/memory behavior | `num_samples` batches candidates; one model load can process independent requests | One-load batch worker retained |

## Important details and limits

- `FullBodyConstraintSet` conditions global joint **positions**, root Y, root X/Z, and heading. Its loaded global rotations are used for reconstructing the pose, but the full-body conditioning path does not separately append global joint rotations.
- `EndEffectorConstraintSet` conditions both selected global joint positions and rotations. This is the correct mechanism for oriented hand/foot contact.
- Root constraints are planar X/Z plus optional heading. Floor height comes from full-body/end-effector root Y or from Backlot's floor support contract.
- Kimodo's stored heading pair is `[cos(theta), sin(theta)]`, while its world forward vector is `[sin(theta), 0, cos(theta)]`; Backlot therefore converts a world `[x, y, z]` heading to encoded `[z, x]`. Treating the pair as `[x, z]` produces a visibly wrong arrival orientation.
- Kimodo does not plan around walls. Navigation must produce and validate a safe corridor first.
- Kimodo has no audited native concept of a door, panel, bench, counter, or camera corridor. Smart interactions resolve those objects into backend-neutral approach/root/contact constraints.
- Contact-event timestamps are production metadata aligned with constrained frames. Only foot contact is predicted directly by the model.
- Multiple candidates are useful for hero interactions, but are structurally scored before any video review.
- `crates/backlot-runtime/src/motion_authoring.rs` defines the backend-neutral request/candidate/evaluation/backend contract and an executable `KimodoMotionBackend`.
- The one-load worker rotates proxy poses to the requested route heading and translates them to the validated root before applying exact world-space joint targets. Without that transform, a reference pose's original facing can override arrival heading.
- The audited CUDA path exposed an upstream mixed-constraint device bug: multi-prompt crops reconstructed end-effector joint indices on CPU while frame indices remained on CUDA. The local runtime now normalizes the paired index tensor device, with a CUDA regression test.

## Bundled SOMA evidence inspected

- `01_single_text_prompt`: one text prompt.
- `02_multi_text_prompt`: two prompts with independent durations.
- `03_full_body_keyframes`: two full-body keyframes.
- `04_ee_constraint`: simultaneous left/right hand and foot position/rotation constraints.
- `05_root_path`: 300-frame dense root path.
- `06_root_waypoints`: three sparse root waypoints.
- `07_mixed_constraints`: one full-body keyframe plus a 152-frame dense root path.
- `08_stylized_text`: text-only style control.

## Concrete Infinite Backlot uses

### Collision-safe locomotion

1. Resolve lobby → entrance → transit wait → Odd Hours counter through `connected_navigation.json`.
2. Smooth and sample the validated route.
3. Convert samples to root X/Z constraints and turn headings.
4. Generate a prompt sequence such as brisk walk → impatient wait → careful doorway traversal → counter approach.
5. Reject candidates whose generated root leaves the safe corridor.

### Panel press

- Reserve the panel staging slot.
- Constrain arrival root and heading.
- Use a right-hand end-effector position and orientation at the contact frame.
- Keep one supporting foot constrained through contact.
- Persist the authored contact event and runtime panel state transition.

### Door interaction

- Require an open portal or schedule the door-opening state transition.
- Constrain approach/follow-through root path.
- Constrain handle-side hand pose and contact window.
- Validate the capsule and generated root through the doorway.

### Sit/pickup/handoff

- Use full-body proxy keyframes for pelvis/torso posture.
- Use hand and foot end-effectors for precise contact.
- Score candidates by target error, foot skate, floor penetration, duration, and corridor compliance.

## Ownership boundary

```text
Navigation: valid geometry, portals, clearance, floor support, reservations
Scene blocking: destination, timing, actor relationships, interaction choice
Kimodo: body performance around validated root/contact constraints
Runtime validation: generated root corridor, contact, floor, and obstacle checks
```

An LLM may request semantic movement, but it never draws the collision-free geometric path.
