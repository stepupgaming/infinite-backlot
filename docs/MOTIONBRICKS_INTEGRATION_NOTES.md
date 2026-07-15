# MotionBricks Integration Notes

## Evidence inspected

- NVIDIA project page: `https://nvlabs.github.io/motionbricks/`.
- Paper: arXiv `2604.24833`, *MotionBricks: Scalable Real-Time Motions with Modular Latent Generative Model and Smart Primitives* (2026).
- The NVIDIA project page currently does **not** expose an official NVIDIA GitHub repository. The only public runtime found is the third-party mirror `Aero-Ex/Nvidia_MotionBricks`, inspected at commit `3d41c4a4b606fbeba2e8107b3c914690d5c64835`.
- Public mirror checkpoint reference: `Aero-Ex/NV_MotionBricks` on Hugging Face, NVIDIA Open Model license according to the mirror.
- A local source-only feasibility check (`python -m compileall`) passed on Windows. No checkpoint was downloaded and no inference claim is made.

## Current public runtime

The mirror contains a modular VQ-VAE, separate root and pose models, a real-time in-betweening controller, MuJoCo integration, training scripts, and a Unitree G1 asset. The demo synthesizes short in-between windows at runtime and steers them with target movement direction, velocity, root position, and heading. Sparse start/end or intermediate keyframes condition root and pose components.

The published smart-locomotion examples mix styles such as idle, walk, jog, run, crouch, injured, zombie, strafing, and crawling. Smart-object examples include pickup, falling, jumping a bench, sitting, and interactive proxy-keyframe authoring.

## Skeleton and retargeting reality

The public mirror ships only a Unitree G1 MuJoCo skeleton and G1 checkpoints. It does not ship SOMA checkpoints, a human production rig, or a Backlot retarget profile. Infinite Backlot would need:

1. a source-to-SOMA or source-to-production-rig joint map;
2. rest-pose and axis certification;
3. scale/root-height normalization;
4. foot, hand, pelvis, and contact-semantic mapping;
5. per-interaction proxy-keyframe conversion;
6. runtime root and heading coordinate conversion;
7. regression clips for hand/foot contact and foot skating.

## Windows limitations

- Core Python source parses on Windows, and the mirror mentions a Windows `keyboard` fallback.
- The supported interactive path is Linux/X11 + MuJoCo; key grabs and focus handling are explicitly problematic elsewhere.
- Checkpoint packaging is separate, not pinned by an official NVIDIA release repository.
- A production Windows adapter should run inference headlessly and avoid inheriting the MuJoCo/X11 demo loop.

## What Infinite Backlot adopts now

The data-driven catalog in `assets/interactions/smart_interactions.json` adopts the durable architectural idea rather than a runtime dependency:

- approach and exit slots;
- root alignment and facing;
- proxy full-body keyframes;
- end-effector contacts;
- state transitions;
- clearance and camera-safe zones;
- backend capability tags.

The navigation planner also owns target velocity, route curvature, and heading samples in a backend-neutral form. These map naturally to MotionBricks root/heading controls later.

## What should wait

Do not replace Kimodo now. Wait for either an official NVIDIA runtime/checkpoint release or a deliberate decision to support the third-party mirror, plus a certified human/SOMA retarget path. MotionBricks is most attractive for real-time locomotion, short reactive in-betweening, step-aside actions, doorway clearance, and smart-object transitions—not for replacing current offline constrained hero authoring.

## Proposed adapter

`MotionBackend::author(request)` would:

1. convert the current pose and a short future `dense_root_path` window into MotionBricks context and root keyframes;
2. map velocity/heading/style from prompt segments;
3. map a smart interaction's proxy keyframes to root/pose component masks;
4. autoregress short windows with deterministic continuation state;
5. retarget G1/model joints to the production skeleton;
6. return `MotionCandidate` with root, joints, contacts, and provenance;
7. reuse Backlot candidate scoring and final corridor/contact validation.

**Recommendation:** preserve the adapter seam and smart-primitive data model now; defer model integration until official packaging and a human retarget profile exist.
