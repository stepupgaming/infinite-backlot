# Canonical SOMA77 Performer Contract

Infinite Backlot's canonical motion performer uses the native 77-joint SOMA hierarchy produced by Kimodo. The production vertical slice deliberately does **not** retarget this motion to KayKit.

## Runtime asset

- Contract: `assets/characters/canonical_soma77.json`
- Runtime motion track: `output/production-vertical-slice/selected_performance.soma.json`
- Bevy playback: `crates/backlot-app/src/bin/odd_hours_bevy_render.rs`
- Source motion: selected native Kimodo `posed_joints` arrays from deterministic NPZ candidates

The performer is a reusable stylized procedural body assembled from beveled capsules and spheres in Bevy. Its body segments are bound directly to the named SOMA joint positions each frame. This preserves native Kimodo skeleton motion while avoiding a retargeting layer in the proof.

## Skeleton and rest calibration

`canonical_soma77.json` records all 77 joint names and parent indices in Kimodo order. The display rest calibration is the first frame of the pinned native SOMA/Kimodo reference track named in `rest_pose_source`. It is calibration data, not a claim that every future production character must share its mesh proportions.

Major runtime joints include:

- Root: `Hips`
- Spine: `Spine`, `Spine1`, `Spine2`, `Chest`, `Neck`, `Neck1`, `Head`
- Arms: `LeftShoulder`, `LeftArm`, `LeftForeArm`, `LeftHand` and right-side equivalents
- Legs: `LeftUpLeg`, `LeftLeg`, `LeftFoot`, `LeftToeBase` and right-side equivalents
- Finger chains remain present in the 77-joint contract even though the current procedural display body does not draw articulated finger geometry.

## Coordinate system and scale

- World: right-handed Bevy coordinates, **Y up**, metres
- Export conversion from Blender: `[x, z, -y]`
- Kimodo root paths and native posed joints are authored directly in the same Bevy world coordinates
- `Hips` is the root motion joint
- Nominal performer height is approximately 1.7 m; no hidden runtime scale or root retargeting is applied

## Motion loading

1. The production compiler resolves semantic navigation and smart-interaction constraints.
2. Kimodo writes native candidate NPZ files with `posed_joints`, `root_positions`, rotations, and foot contacts.
3. `tools/motion/export_soma_track.py` converts each candidate into a typed `SomaMotionTrack` JSON without changing the skeleton order.
4. Full-body collision, locomotion sanity, foot locking, contact correction, and deterministic selection run against these native tracks.
5. Selected corrected segment tracks are continuity-blended into `selected_performance.soma.json`.
6. Bevy evaluates one native pose per rendered frame and moves the procedural body segments between their named joints.

## Materials

The reusable proof performer uses a deliberately readable show palette:

- Berry/magenta jacket torso and upper arms
- Warm skin head and hands
- Deep navy trousers
- Teal shoes
- Dark hair/head cap

These are runtime `StandardMaterial` instances and are independent of the motion contract.

## Future production-character conformance

A future mesh character can conform without changing motion authoring by:

1. Supplying the exact 77 SOMA joint names and parent hierarchy, or a formally documented deterministic mapping.
2. Matching Y-up metres and the `Hips` root convention.
3. Providing an explicit rest-pose calibration against `canonical_soma77.json`.
4. Keeping body proportions within the validated collision-volume envelope, or regenerating that character's capsule radii.
5. Preserving foot, hand, and head joint semantics used by contact correction and collision validation.

Finger articulation, facial controls, skin deformation, and production clothing are intentionally future presentation layers. They are not prerequisites for native SOMA playback or environment contact.
