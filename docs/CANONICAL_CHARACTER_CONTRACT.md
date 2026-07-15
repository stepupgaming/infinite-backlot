# Canonical Character Contract

## Scope

This contract is the import boundary for future Infinite Backlot characters. It does **not** claim that current KayKit performers or the motion-lab SOMA body are production-ready hero characters. The motion lab deliberately uses Kimodo's native SOMA77 skin to judge source motion before retargeting.

## Required coordinate contract

- Units: meters.
- Runtime up axis: `+Y`.
- Runtime forward axis: `-Z` for authored GLB assets.
- Root transform: identity scale, finite translation/rotation.
- Character origin: projected center of the feet in neutral standing.
- Nominal adult height: 1.55–2.05 m unless the character manifest explicitly declares a stylized scale.
- The exported GLB must preserve the declared coordinate conversion and must not depend on an undocumented import rotation.

## Skeleton and rest pose

A character manifest must declare:

- Stable skeleton ID and version.
- Complete bone-name list and parent hierarchy.
- Rest-pose local translation and rotation for every joint.
- Forward/up axes for each major joint family when they differ from the skeleton default.
- Scale and body-height measurement.
- Whether the neutral is T-pose, A-pose, or another reviewed pose.

Minimum production joints:

```text
root / hips
spine chain / chest
neck / head
left and right clavicle / upper arm / forearm / hand
left and right upper leg / lower leg / foot / toe
```

Optional finger and facial joints must have stable names and may not reorder required body joints between versions.

## Skinning

- One armature owns the deforming body meshes.
- Every deforming vertex has finite normalized weights.
- No required body region may be rigidly weighted to an unrelated ancestor.
- Inverse bind matrices must match the exported rest pose.
- Materials use stable semantic slots rather than Blender-generated numeric names.
- The asset must render without external absolute texture paths.

## Contact contract

The character declares exact bones or semantic points for:

- Left/right heel.
- Left/right toe or ball.
- Left/right palm.
- Left/right primary hand-contact point.
- Head and gaze origin.
- Pelvis/root trajectory origin.

Motion clips may provide per-frame foot-contact booleans and contact positions. Retargeting must preserve these channels even when the target has fewer visual foot bones.

## Optional facial contract

A face-capable character may expose reviewed blendshapes for blink, jaw open, smile/frown, and visemes. Names and ranges are versioned in the character manifest. Missing facial capability must be explicit; it may not silently map to unrelated shapes.

## Validation gates

A character is structurally loadable only when:

1. The GLB parses and all referenced buffers/images exist.
2. The skeleton hierarchy is acyclic and required semantic bones resolve exactly once.
3. Rest transforms, inverse binds, scale, and bounds are finite.
4. Contact bones and root are present.
5. A neutral-pose preview contains no inverted feet, collapsed limbs, or unbound vertices.
6. A walk, 90-degree turn, panel reach, and full-body reaction can be previewed without NaNs or catastrophic penetration.

Visual approval remains separate from structural loading.

## Current adapters

### SOMA77 motion lab

`assets/motion-lab/canonical_soma_performer.json` points to Kimodo's bundled SOMA77 skeleton, standard T-pose, and skin. Motion is displayed directly on that native body with no KayKit retargeting.

### KayKit production cast

The current episode cast uses the measured SOMA-to-KayKit profile and project-owned per-joint corrections. It remains a compatibility adapter, not the canonical source skeleton. Path-warped locomotion and limited contact correction are documented limitations.

## Versioning

Character manifests and retarget profiles are immutable by version. Bone renames, hierarchy changes, rest-pose changes, or material-slot changes require a new version and an explicit migration. Existing frozen episodes retain their referenced versions.
