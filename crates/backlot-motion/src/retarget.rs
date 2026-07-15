use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetargetJoint {
    pub source: String,
    pub target: String,
    #[serde(default = "identity_quat")]
    pub rest_correction: [f32; 4],
}

fn identity_quat() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

/// Runtime delta-basis correction (xyzw). Both loaders normalize the reviewed
/// clips and KayKit rig to the same Bevy-local axes; raw Blender Z-up rest-space
/// corrections are diagnostic only and must not be baked into clip deltas.
fn kaykit_basis(target: &str) -> [f32; 4] {
    match target {
        "hips" | "spine" | "chest" | "neck" | "head" | "upperarm.l" | "lowerarm.l" | "hand.l"
        | "upperarm.r" | "lowerarm.r" | "hand.r" | "upperleg.l" | "lowerleg.l" | "foot.l"
        | "toes.l" | "upperleg.r" | "lowerleg.r" | "foot.r" | "toes.r" => identity_quat(),
        _ => identity_quat(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetargetMap {
    pub source_skeleton: String,
    pub target_skeleton: String,
    pub scale: f32,
    pub joints: Vec<RetargetJoint>,
}

impl RetargetMap {
    pub fn target_for(&self, source: &str) -> Option<&RetargetJoint> {
        self.joints.iter().find(|joint| joint.source == source)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.scale.is_finite() || self.scale <= 0.0 {
            return Err("retarget scale must be positive".into());
        }
        let mut targets = BTreeMap::new();
        for joint in &self.joints {
            if targets.insert(&joint.target, &joint.source).is_some() {
                return Err(format!("duplicate target joint {}", joint.target));
            }
        }
        Ok(())
    }

    /// Fixed one-time SOMA77 -> KayKit cast mapping. Infinite Backlot has two
    /// approved cast rigs, so a bounded cast-specific map is more reliable than
    /// a general-purpose humanoid importer.
    pub fn soma77_to_kaykit() -> Self {
        let pairs = [
            ("Hips", "hips"),
            ("Spine1", "spine"),
            ("Chest", "chest"),
            ("Neck2", "neck"),
            ("Head", "head"),
            ("LeftArm", "upperarm.l"),
            ("LeftForeArm", "lowerarm.l"),
            ("LeftHand", "hand.l"),
            ("RightArm", "upperarm.r"),
            ("RightForeArm", "lowerarm.r"),
            ("RightHand", "hand.r"),
            ("LeftLeg", "upperleg.l"),
            ("LeftShin", "lowerleg.l"),
            ("LeftFoot", "foot.l"),
            ("LeftToeBase", "toes.l"),
            ("RightLeg", "upperleg.r"),
            ("RightShin", "lowerleg.r"),
            ("RightFoot", "foot.r"),
            ("RightToeBase", "toes.r"),
        ];
        Self {
            source_skeleton: "somaskel77".into(),
            target_skeleton: "kaykit_backlot_cast".into(),
            scale: 1.0,
            joints: pairs
                .into_iter()
                .map(|(source, target)| RetargetJoint {
                    source: source.into(),
                    target: target.into(),
                    rest_correction: kaykit_basis(target),
                })
                .collect(),
        }
    }
}

pub fn warp_root_to_path(root: &mut [[f32; 3]], path: &[[f32; 3]]) {
    if root.is_empty() || path.len() < 2 {
        return;
    }
    let start = root[0];
    let end = *root.last().unwrap();
    let generated_distance = ((end[0] - start[0]).powi(2) + (end[2] - start[2]).powi(2))
        .sqrt()
        .max(0.001);
    let mut lengths = Vec::with_capacity(path.len());
    lengths.push(0.0);
    for i in 1..path.len() {
        let dx = path[i][0] - path[i - 1][0];
        let dz = path[i][2] - path[i - 1][2];
        lengths.push(lengths[i - 1] + (dx * dx + dz * dz).sqrt());
    }
    let total = *lengths.last().unwrap();
    for position in root.iter_mut() {
        let progress = (((position[0] - start[0]).powi(2) + (position[2] - start[2]).powi(2))
            .sqrt()
            / generated_distance)
            .clamp(0.0, 1.0);
        let distance = progress * total;
        let segment = lengths
            .iter()
            .position(|value| *value >= distance)
            .unwrap_or(path.len() - 1)
            .max(1);
        let span = (lengths[segment] - lengths[segment - 1]).max(0.001);
        let t = (distance - lengths[segment - 1]) / span;
        position[0] = path[segment - 1][0] + (path[segment][0] - path[segment - 1][0]) * t;
        position[2] = path[segment - 1][2] + (path[segment][2] - path[segment - 1][2]) * t;
    }
}
