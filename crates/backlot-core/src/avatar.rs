//! Humanoid avatar abstraction — a SMPL-X-compatible *canonical contract*.
//!
//! The runtime performers (Bevy scene meshes, the offline software renderer,
//! attachment logic, and the camera director) are all expressed in terms of
//! this stable humanoid standard rather than raw capsule geometry. The exact
//! mesh may later be a SOMA/SMPL-X-derived skinned character; this module is
//! the shared mapping layer so swapping the visual asset does not require
//! touching camera, interaction, or attachment code.
//!
//! This is engine-agnostic: no Bevy types, only `f32` math.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Semantic joints every humanoid performer exposes. Camera framing, gaze,
/// and prop attachment all target these names — never a capsule height.
///
/// A few of these double as internal limb anchors in the reduced rig (e.g.
/// `LeftEye`/`RightEye` anchor the thighs and `LeftWrist`/`RightWrist` anchor
/// the upper arms / lower legs). The semantic *camera/attachment* targets
/// (`Head`, `UpperChest`, `PropGrip`, `Gaze`, `LeftHand`, `RightHand`) are the
/// stable contract; the internal anchors are an implementation detail of the
/// blocky stand-in and are remapped when a real skinned asset is supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticJoint {
    Root,
    Pelvis,
    Chest,
    UpperChest,
    Neck,
    Head,
    LeftHand,
    RightHand,
    LeftWrist,
    RightWrist,
    Jaw,
    LeftEye,
    RightEye,
    Gaze,
    PropGrip,
    // Unique rig-part joints so transforms never collide by joint key.
    LeftUpperArm,
    RightUpperArm,
    LeftThigh,
    RightThigh,
    LeftShin,
    RightShin,
}

impl SemanticJoint {
    pub fn all() -> &'static [SemanticJoint] {
        use SemanticJoint::*;
        &[
            Root,
            Pelvis,
            Chest,
            UpperChest,
            Neck,
            Head,
            LeftHand,
            RightHand,
            LeftWrist,
            RightWrist,
            Jaw,
            LeftEye,
            RightEye,
            Gaze,
            PropGrip,
            LeftUpperArm,
            RightUpperArm,
            LeftThigh,
            RightThigh,
            LeftShin,
            RightShin,
        ]
    }
}

/// A single rigid body part of the humanoid, expressed relative to its parent
/// joint in the canonical rest pose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigPart {
    pub joint: SemanticJoint,
    pub parent: SemanticJoint,
    /// Local offset of this part's center from the parent joint (meters).
    pub offset: [f32; 3],
    /// Half-extents of the box approximating this part (meters).
    pub half: [f32; 3],
    /// Approximate flat-shaded base color (linear RGB 0..1).
    pub color: [f32; 3],
    /// Whether this part is the canonical "grip" used for held props.
    pub is_grip: bool,
}

/// The canonical humanoid rig: a reduced, remappable skeleton with semantic
/// joints, grip points, and camera-interest targets. A future SOMA/SMPL-X
/// character is mapped onto these same semantic joints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanoidRig {
    /// Stable character id this rig instance is bound to.
    pub character_id: String,
    /// Persistent voice id (echoed for scheduling).
    pub voice_id: String,
    /// Body-shape profile (canonical, SMPL-X-compatible placeholders).
    pub body: BodyShapeProfile,
    /// Parts in parent-before-child order.
    pub parts: Vec<RigPart>,
}

/// Canonical body-shape parameters. Real SOMA output would populate these;
/// here they are safe placeholders that still drive proportions + camera.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyShapeProfile {
    pub height_m: f32,
    pub shoulder_width_m: f32,
    pub hip_width_m: f32,
    pub head_size_m: f32,
    pub build: String,
}

impl Default for BodyShapeProfile {
    fn default() -> Self {
        Self {
            height_m: 1.75,
            shoulder_width_m: 0.42,
            hip_width_m: 0.32,
            head_size_m: 0.24,
            build: "average".into(),
        }
    }
}

impl HumanoidRig {
    /// Build a commercially-safe blocky humanoid stand-in bound to `character_id`.
    /// The proportions and semantic joints are identical to what a SOMA/SMPL-X
    /// character would expose, so the content engine is unchanged when a real
    /// skinned asset is dropped in later.
    pub fn default_humanoid(character_id: &str, voice_id: &str, body_color: [f32; 3]) -> Self {
        let sw = 0.21;
        let hw = 0.16;
        let ua = 0.28;
        let la = 0.26;
        let thigh = 0.42;
        let shin = 0.42;
        let skin = body_color;
        let cloth = [
            body_color[0] * 0.6,
            body_color[1] * 0.6,
            body_color[2] * 0.65 + 0.1,
        ];
        let dark = [0.12, 0.12, 0.14];
        use SemanticJoint::*;
        let parts = vec![
            RigPart {
                joint: Pelvis,
                parent: Root,
                offset: [0.0, 0.92, 0.0],
                half: [hw, 0.16, 0.13],
                color: cloth,
                is_grip: false,
            },
            RigPart {
                joint: Chest,
                parent: Pelvis,
                offset: [0.0, 0.30, 0.0],
                half: [sw, 0.20, 0.16],
                color: cloth,
                is_grip: false,
            },
            RigPart {
                joint: UpperChest,
                parent: Chest,
                offset: [0.0, 0.18, 0.0],
                half: [sw, 0.10, 0.16],
                color: cloth,
                is_grip: false,
            },
            RigPart {
                joint: Neck,
                parent: UpperChest,
                offset: [0.0, 0.12, 0.0],
                half: [0.06, 0.07, 0.06],
                color: skin,
                is_grip: false,
            },
            RigPart {
                joint: Head,
                parent: Neck,
                offset: [0.0, 0.16, 0.0],
                half: [0.12, 0.13, 0.12],
                color: skin,
                is_grip: false,
            },
            RigPart {
                joint: Jaw,
                parent: Head,
                offset: [0.0, -0.07, 0.06],
                half: [0.06, 0.04, 0.05],
                color: skin,
                is_grip: false,
            },
            RigPart {
                joint: LeftEye,
                parent: Head,
                offset: [-0.05, 0.02, 0.10],
                half: [0.02, 0.02, 0.01],
                color: [0.9, 0.9, 0.95],
                is_grip: false,
            },
            RigPart {
                joint: RightEye,
                parent: Head,
                offset: [0.05, 0.02, 0.10],
                half: [0.02, 0.02, 0.01],
                color: [0.9, 0.9, 0.95],
                is_grip: false,
            },
            RigPart {
                joint: Gaze,
                parent: Head,
                offset: [0.0, 0.0, 0.5],
                half: [0.01, 0.01, 0.01],
                color: [1.0, 1.0, 1.0],
                is_grip: false,
            },
            // Arms: upper-arm box hangs from upper chest; hand extends it.
            RigPart {
                joint: LeftUpperArm,
                parent: UpperChest,
                offset: [-sw - 0.02, 0.10, 0.0],
                half: [0.05, ua, 0.05],
                color: skin,
                is_grip: false,
            },
            RigPart {
                joint: RightUpperArm,
                parent: UpperChest,
                offset: [sw + 0.02, 0.10, 0.0],
                half: [0.05, ua, 0.05],
                color: skin,
                is_grip: false,
            },
            RigPart {
                joint: LeftHand,
                parent: LeftUpperArm,
                offset: [0.0, -ua - la * 0.5, 0.0],
                half: [0.05, la * 0.5, 0.05],
                color: skin,
                is_grip: true,
            },
            RigPart {
                joint: RightHand,
                parent: RightUpperArm,
                offset: [0.0, -ua - la * 0.5, 0.0],
                half: [0.05, la * 0.5, 0.05],
                color: skin,
                is_grip: true,
            },
            // Legs: thigh box hangs from pelvis; shin + foot extend.
            RigPart {
                joint: LeftThigh,
                parent: Pelvis,
                offset: [-hw, -0.02, 0.0],
                half: [0.08, thigh, 0.09],
                color: dark,
                is_grip: false,
            },
            RigPart {
                joint: RightThigh,
                parent: Pelvis,
                offset: [hw, -0.02, 0.0],
                half: [0.08, thigh, 0.09],
                color: dark,
                is_grip: false,
            },
            RigPart {
                joint: LeftShin,
                parent: LeftThigh,
                offset: [0.0, -thigh - shin * 0.5 + 0.02, 0.04],
                half: [0.07, shin * 0.5, 0.08],
                color: dark,
                is_grip: false,
            },
            RigPart {
                joint: RightShin,
                parent: RightThigh,
                offset: [0.0, -thigh - shin * 0.5 + 0.02, 0.04],
                half: [0.07, shin * 0.5, 0.08],
                color: dark,
                is_grip: false,
            },
            RigPart {
                joint: PropGrip,
                parent: LeftHand,
                offset: [0.0, -0.06, 0.06],
                half: [0.02, 0.02, 0.02],
                color: [1.0, 0.8, 0.3],
                is_grip: true,
            },
        ];
        Self {
            character_id: character_id.into(),
            voice_id: voice_id.into(),
            body: BodyShapeProfile::default(),
            parts,
        }
    }

    /// Compute world transforms (rotation matrix + translation) for every joint
    /// by walking the parent chain. `root` places the character in the world.
    pub fn world_matrices(&self, root: &Xform, pose: &Pose) -> HashMap<SemanticJoint, RigWorld> {
        let mut out: HashMap<SemanticJoint, RigWorld> = HashMap::new();
        out.insert(
            SemanticJoint::Root,
            RigWorld {
                rot: rot3(root.rot),
                pos: root.pos,
            },
        );
        for p in &self.parts {
            let parent = out.get(&p.parent).cloned().unwrap_or_else(|| RigWorld {
                rot: rot3(root.rot),
                pos: root.pos,
            });
            let local = pose
                .0
                .get(&p.joint)
                .cloned()
                .unwrap_or_else(Xform::identity);
            let local_rot = rot3(local.rot);
            let rot = mul3(parent.rot, local_rot);
            let translation = add3(parent.pos, mul3v(parent.rot, add3(p.offset, local.pos)));
            out.insert(
                p.joint,
                RigWorld {
                    rot,
                    pos: translation,
                },
            );
        }
        out
    }

    /// World-space position of a semantic joint.
    pub fn joint_world(&self, joint: SemanticJoint, root: &Xform, pose: &Pose) -> [f32; 3] {
        self.world_matrices(root, pose)
            .get(&joint)
            .map(|w| w.pos)
            .unwrap_or(root.pos)
    }

    /// Camera-interest target for a semantic role.
    pub fn camera_target(&self, role: CameraTargetRole, root: &Xform, pose: &Pose) -> [f32; 3] {
        let w = self.world_matrices(root, pose);
        match role {
            CameraTargetRole::Head => w
                .get(&SemanticJoint::Head)
                .map(|x| [x.pos[0], x.pos[1] + 0.10, x.pos[2]])
                .unwrap_or(root.pos),
            CameraTargetRole::Chest => w
                .get(&SemanticJoint::UpperChest)
                .map(|x| x.pos)
                .or_else(|| w.get(&SemanticJoint::Chest).map(|x| x.pos))
                .unwrap_or(root.pos),
            CameraTargetRole::Gaze => w
                .get(&SemanticJoint::Gaze)
                .map(|x| x.pos)
                .unwrap_or(root.pos),
            CameraTargetRole::PropGrip => w
                .get(&SemanticJoint::PropGrip)
                .or_else(|| w.get(&SemanticJoint::LeftHand))
                .map(|x| x.pos)
                .unwrap_or(root.pos),
        }
    }

    /// World transform of a semantic joint (for attaching props via grip).
    pub fn joint_world_xform(&self, joint: SemanticJoint, root: &Xform, pose: &Pose) -> RigWorld {
        self.world_matrices(root, pose)
            .get(&joint)
            .cloned()
            .unwrap_or_else(|| RigWorld {
                rot: rot3(root.rot),
                pos: root.pos,
            })
    }
}

/// Which semantic point a camera shot should frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraTargetRole {
    Head,
    Chest,
    Gaze,
    PropGrip,
}

/// World transform of a joint: rotation matrix + translation.
#[derive(Debug, Clone)]
pub struct RigWorld {
    pub rot: [[f32; 3]; 3],
    pub pos: [f32; 3],
}

/// A local transform: translation + XYZ euler rotation (radians).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Xform {
    pub pos: [f32; 3],
    pub rot: [f32; 3],
}

impl Xform {
    pub fn identity() -> Self {
        Self {
            pos: [0.0; 3],
            rot: [0.0; 3],
        }
    }
    pub fn from_pos(pos: [f32; 3]) -> Self {
        Self { pos, rot: [0.0; 3] }
    }
    pub fn from_rot(rot: [f32; 3]) -> Self {
        Self { pos: [0.0; 3], rot }
    }
}

/// Per-joint pose: local transform overrides from the rest pose.
#[derive(Debug, Clone, Default)]
pub struct Pose(pub HashMap<SemanticJoint, Xform>);

impl Pose {
    pub fn set(&mut self, j: SemanticJoint, x: Xform) {
        self.0.insert(j, x);
    }
    pub fn get(&self, j: SemanticJoint) -> Option<&Xform> {
        self.0.get(&j)
    }

    /// Blend two local poses. Missing joints are treated as identity transforms,
    /// which makes this suitable for temporary upper-body overlays.
    pub fn blend(a: &Pose, b: &Pose, weight: f32) -> Pose {
        let w = weight.clamp(0.0, 1.0);
        let mut out = Pose::default();
        for joint in SemanticJoint::all() {
            let av = a.get(*joint).cloned().unwrap_or_else(Xform::identity);
            let bv = b.get(*joint).cloned().unwrap_or_else(Xform::identity);
            let lerp3 = |x: [f32; 3], y: [f32; 3]| {
                [
                    x[0] + (y[0] - x[0]) * w,
                    x[1] + (y[1] - x[1]) * w,
                    x[2] + (y[2] - x[2]) * w,
                ]
            };
            let blended = Xform {
                pos: lerp3(av.pos, bv.pos),
                rot: lerp3(av.rot, bv.rot),
            };
            if blended.pos != [0.0; 3] || blended.rot != [0.0; 3] {
                out.set(*joint, blended);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Animation: produce a Pose for a performance state at time `t` (seconds).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceState {
    Idle,
    Walk,
    Talk,
    Listen,
    React,
    Gesture,
    Point,
    Look,
}

fn rot3(e: [f32; 3]) -> [[f32; 3]; 3] {
    let (sx, cx) = e[0].sin_cos();
    let (sy, cy) = e[1].sin_cos();
    let (sz, cz) = e[2].sin_cos();
    [
        [cy * cz, -cy * sz, sy],
        [cx * sz + sx * sy * cz, cx * cz - sx * sy * sz, -sx * cy],
        [sx * sz - cx * sy * cz, sx * cz + cx * sy * sz, cx * cy],
    ]
}

fn mul3(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut r = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    r
}

fn mul3v(a: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        a[0][0] * v[0] + a[0][1] * v[1] + a[0][2] * v[2],
        a[1][0] * v[0] + a[1][1] * v[1] + a[1][2] * v[2],
        a[2][0] * v[0] + a[2][1] * v[1] + a[2][2] * v[2],
    ]
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Build the rest pose plus state-specific modulation for a character.
pub fn character_pose(state: PerformanceState, t: f32, walk_phase: f32) -> Pose {
    let mut pose = Pose::default();
    use SemanticJoint::*;
    let lsh = LeftUpperArm;
    let rsh = RightUpperArm;
    match state {
        PerformanceState::Idle => {
            let bob = (t * 1.6).sin() * 0.02;
            pose.set(
                Chest,
                Xform {
                    pos: [0.0, bob, 0.0],
                    rot: [0.0, 0.0, 0.0],
                },
            );
            pose.set(
                LeftUpperArm,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.05, 0.0, 0.12],
                },
            );
            pose.set(
                RightUpperArm,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.05, 0.0, -0.12],
                },
            );
        }
        PerformanceState::Walk => {
            let s = (walk_phase * 6.0).sin();
            let c = (walk_phase * 6.0).cos();
            pose.set(
                lsh,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.4 + s * 0.5, 0.0, 0.12],
                },
            );
            pose.set(
                rsh,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.4 - s * 0.5, 0.0, -0.12],
                },
            );
            pose.set(
                LeftThigh,
                Xform {
                    pos: [0.0; 3],
                    rot: [c * 0.5, 0.0, 0.0],
                },
            );
            pose.set(
                RightThigh,
                Xform {
                    pos: [0.0; 3],
                    rot: [-c * 0.5, 0.0, 0.0],
                },
            );
            let bob = (walk_phase * 6.0).abs().sin() * 0.04;
            pose.set(
                Chest,
                Xform {
                    pos: [0.0, bob, 0.0],
                    rot: [0.05, 0.0, 0.0],
                },
            );
        }
        PerformanceState::Talk => {
            let nod = (t * 4.0).sin() * 0.14;
            pose.set(
                Head,
                Xform {
                    pos: [0.0; 3],
                    rot: [nod, 0.0, 0.0],
                },
            );
            pose.set(
                rsh,
                Xform {
                    pos: [0.0; 3],
                    rot: [-0.16 + (t * 3.0).sin() * 0.34, 0.0, -0.28],
                },
            );
            pose.set(
                lsh,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.1, 0.0, 0.15],
                },
            );
        }
        PerformanceState::Listen => {
            let shift = (t * 1.2).sin() * 0.025;
            pose.set(
                Head,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.03, 0.08, 0.0],
                },
            );
            pose.set(
                Chest,
                Xform {
                    pos: [shift, 0.0, 0.0],
                    rot: [0.0, 0.04, -shift],
                },
            );
            pose.set(
                lsh,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.05, 0.0, 0.13],
                },
            );
            pose.set(
                rsh,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.05, 0.0, -0.13],
                },
            );
        }
        PerformanceState::React => {
            let k = ((t * 9.0).sin() * (1.0 - (t * 2.0).min(1.0))).clamp(-1.0, 1.0);
            pose.set(
                Head,
                Xform {
                    pos: [0.0; 3],
                    rot: [-0.25 * k, 0.1 * k, 0.0],
                },
            );
            pose.set(
                Chest,
                Xform {
                    pos: [0.0, 0.03 * k, 0.0],
                    rot: [-0.15 * k, 0.0, 0.0],
                },
            );
            pose.set(
                lsh,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.6 * k, 0.0, 0.2],
                },
            );
            pose.set(
                rsh,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.6 * k, 0.0, -0.2],
                },
            );
        }
        PerformanceState::Gesture => {
            let w = (t * 3.0).sin() * 0.18;
            pose.set(
                rsh,
                Xform {
                    pos: [0.0; 3],
                    rot: [-0.62, 0.0, -0.22 + w],
                },
            );
            pose.set(
                lsh,
                Xform {
                    pos: [0.0; 3],
                    rot: [-0.18, 0.0, 0.18],
                },
            );
            pose.set(
                Head,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.05, -0.1, 0.0],
                },
            );
        }
        PerformanceState::Point => {
            pose.set(
                rsh,
                Xform {
                    pos: [0.0; 3],
                    rot: [-1.3, 0.0, -0.1],
                },
            );
            pose.set(
                Head,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.0, 0.2, 0.0],
                },
            );
        }
        PerformanceState::Look => {
            pose.set(
                Head,
                Xform {
                    pos: [0.0; 3],
                    rot: [0.0, 0.3, 0.0],
                },
            );
        }
    }
    pose
}

/// Transform a point by an `Xform`'s rotation.
pub fn xform_point(x: &Xform, p: [f32; 3]) -> [f32; 3] {
    add3(x.pos, mul3v(rot3(x.rot), p))
}

/// World-space 8-corner box for a rig part given the part's world transform.
pub fn part_corners(part: &RigPart, world: &RigWorld) -> [[f32; 3]; 8] {
    let h = part.half;
    let corners = [
        [-h[0], -h[1], -h[2]],
        [h[0], -h[1], -h[2]],
        [h[0], h[1], -h[2]],
        [-h[0], h[1], -h[2]],
        [-h[0], -h[1], h[2]],
        [h[0], -h[1], h[2]],
        [h[0], h[1], h[2]],
        [-h[0], h[1], h[2]],
    ];
    let mut out = [[0.0f32; 3]; 8];
    for (i, c) in corners.iter().enumerate() {
        out[i] = add3(world.pos, mul3v(world.rot, *c));
    }
    out
}
