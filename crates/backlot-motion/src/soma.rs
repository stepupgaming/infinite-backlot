use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SomaJoint {
    pub name: &'static str,
    pub parent: Option<&'static str>,
}

/// NVIDIA Kimodo SOMA v1.1 `somaskel77` hierarchy in source order.
pub const SOMA77: &[SomaJoint] = &[
    SomaJoint {
        name: "Hips",
        parent: None,
    },
    SomaJoint {
        name: "Spine1",
        parent: Some("Hips"),
    },
    SomaJoint {
        name: "Spine2",
        parent: Some("Spine1"),
    },
    SomaJoint {
        name: "Chest",
        parent: Some("Spine2"),
    },
    SomaJoint {
        name: "Neck1",
        parent: Some("Chest"),
    },
    SomaJoint {
        name: "Neck2",
        parent: Some("Neck1"),
    },
    SomaJoint {
        name: "Head",
        parent: Some("Neck2"),
    },
    SomaJoint {
        name: "HeadEnd",
        parent: Some("Head"),
    },
    SomaJoint {
        name: "Jaw",
        parent: Some("Head"),
    },
    SomaJoint {
        name: "LeftEye",
        parent: Some("Head"),
    },
    SomaJoint {
        name: "RightEye",
        parent: Some("Head"),
    },
    SomaJoint {
        name: "LeftShoulder",
        parent: Some("Chest"),
    },
    SomaJoint {
        name: "LeftArm",
        parent: Some("LeftShoulder"),
    },
    SomaJoint {
        name: "LeftForeArm",
        parent: Some("LeftArm"),
    },
    SomaJoint {
        name: "LeftHand",
        parent: Some("LeftForeArm"),
    },
    SomaJoint {
        name: "LeftHandThumb1",
        parent: Some("LeftHand"),
    },
    SomaJoint {
        name: "LeftHandThumb2",
        parent: Some("LeftHandThumb1"),
    },
    SomaJoint {
        name: "LeftHandThumb3",
        parent: Some("LeftHandThumb2"),
    },
    SomaJoint {
        name: "LeftHandThumbEnd",
        parent: Some("LeftHandThumb3"),
    },
    SomaJoint {
        name: "LeftHandIndex1",
        parent: Some("LeftHand"),
    },
    SomaJoint {
        name: "LeftHandIndex2",
        parent: Some("LeftHandIndex1"),
    },
    SomaJoint {
        name: "LeftHandIndex3",
        parent: Some("LeftHandIndex2"),
    },
    SomaJoint {
        name: "LeftHandIndex4",
        parent: Some("LeftHandIndex3"),
    },
    SomaJoint {
        name: "LeftHandIndexEnd",
        parent: Some("LeftHandIndex4"),
    },
    SomaJoint {
        name: "LeftHandMiddle1",
        parent: Some("LeftHand"),
    },
    SomaJoint {
        name: "LeftHandMiddle2",
        parent: Some("LeftHandMiddle1"),
    },
    SomaJoint {
        name: "LeftHandMiddle3",
        parent: Some("LeftHandMiddle2"),
    },
    SomaJoint {
        name: "LeftHandMiddle4",
        parent: Some("LeftHandMiddle3"),
    },
    SomaJoint {
        name: "LeftHandMiddleEnd",
        parent: Some("LeftHandMiddle4"),
    },
    SomaJoint {
        name: "LeftHandRing1",
        parent: Some("LeftHand"),
    },
    SomaJoint {
        name: "LeftHandRing2",
        parent: Some("LeftHandRing1"),
    },
    SomaJoint {
        name: "LeftHandRing3",
        parent: Some("LeftHandRing2"),
    },
    SomaJoint {
        name: "LeftHandRing4",
        parent: Some("LeftHandRing3"),
    },
    SomaJoint {
        name: "LeftHandRingEnd",
        parent: Some("LeftHandRing4"),
    },
    SomaJoint {
        name: "LeftHandPinky1",
        parent: Some("LeftHand"),
    },
    SomaJoint {
        name: "LeftHandPinky2",
        parent: Some("LeftHandPinky1"),
    },
    SomaJoint {
        name: "LeftHandPinky3",
        parent: Some("LeftHandPinky2"),
    },
    SomaJoint {
        name: "LeftHandPinky4",
        parent: Some("LeftHandPinky3"),
    },
    SomaJoint {
        name: "LeftHandPinkyEnd",
        parent: Some("LeftHandPinky4"),
    },
    SomaJoint {
        name: "RightShoulder",
        parent: Some("Chest"),
    },
    SomaJoint {
        name: "RightArm",
        parent: Some("RightShoulder"),
    },
    SomaJoint {
        name: "RightForeArm",
        parent: Some("RightArm"),
    },
    SomaJoint {
        name: "RightHand",
        parent: Some("RightForeArm"),
    },
    SomaJoint {
        name: "RightHandThumb1",
        parent: Some("RightHand"),
    },
    SomaJoint {
        name: "RightHandThumb2",
        parent: Some("RightHandThumb1"),
    },
    SomaJoint {
        name: "RightHandThumb3",
        parent: Some("RightHandThumb2"),
    },
    SomaJoint {
        name: "RightHandThumbEnd",
        parent: Some("RightHandThumb3"),
    },
    SomaJoint {
        name: "RightHandIndex1",
        parent: Some("RightHand"),
    },
    SomaJoint {
        name: "RightHandIndex2",
        parent: Some("RightHandIndex1"),
    },
    SomaJoint {
        name: "RightHandIndex3",
        parent: Some("RightHandIndex2"),
    },
    SomaJoint {
        name: "RightHandIndex4",
        parent: Some("RightHandIndex3"),
    },
    SomaJoint {
        name: "RightHandIndexEnd",
        parent: Some("RightHandIndex4"),
    },
    SomaJoint {
        name: "RightHandMiddle1",
        parent: Some("RightHand"),
    },
    SomaJoint {
        name: "RightHandMiddle2",
        parent: Some("RightHandMiddle1"),
    },
    SomaJoint {
        name: "RightHandMiddle3",
        parent: Some("RightHandMiddle2"),
    },
    SomaJoint {
        name: "RightHandMiddle4",
        parent: Some("RightHandMiddle3"),
    },
    SomaJoint {
        name: "RightHandMiddleEnd",
        parent: Some("RightHandMiddle4"),
    },
    SomaJoint {
        name: "RightHandRing1",
        parent: Some("RightHand"),
    },
    SomaJoint {
        name: "RightHandRing2",
        parent: Some("RightHandRing1"),
    },
    SomaJoint {
        name: "RightHandRing3",
        parent: Some("RightHandRing2"),
    },
    SomaJoint {
        name: "RightHandRing4",
        parent: Some("RightHandRing3"),
    },
    SomaJoint {
        name: "RightHandRingEnd",
        parent: Some("RightHandRing4"),
    },
    SomaJoint {
        name: "RightHandPinky1",
        parent: Some("RightHand"),
    },
    SomaJoint {
        name: "RightHandPinky2",
        parent: Some("RightHandPinky1"),
    },
    SomaJoint {
        name: "RightHandPinky3",
        parent: Some("RightHandPinky2"),
    },
    SomaJoint {
        name: "RightHandPinky4",
        parent: Some("RightHandPinky3"),
    },
    SomaJoint {
        name: "RightHandPinkyEnd",
        parent: Some("RightHandPinky4"),
    },
    SomaJoint {
        name: "LeftLeg",
        parent: Some("Hips"),
    },
    SomaJoint {
        name: "LeftShin",
        parent: Some("LeftLeg"),
    },
    SomaJoint {
        name: "LeftFoot",
        parent: Some("LeftShin"),
    },
    SomaJoint {
        name: "LeftToeBase",
        parent: Some("LeftFoot"),
    },
    SomaJoint {
        name: "LeftToeEnd",
        parent: Some("LeftToeBase"),
    },
    SomaJoint {
        name: "RightLeg",
        parent: Some("Hips"),
    },
    SomaJoint {
        name: "RightShin",
        parent: Some("RightLeg"),
    },
    SomaJoint {
        name: "RightFoot",
        parent: Some("RightShin"),
    },
    SomaJoint {
        name: "RightToeBase",
        parent: Some("RightFoot"),
    },
    SomaJoint {
        name: "RightToeEnd",
        parent: Some("RightToeBase"),
    },
];

pub fn semantic_alias(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "root" | "pelvis" | "hips" => Some("Hips"),
        "spine" | "spine1" => Some("Spine1"),
        "spine2" => Some("Spine2"),
        "chest" | "upperchest" => Some("Chest"),
        "neck" | "neck1" => Some("Neck1"),
        "neck2" => Some("Neck2"),
        "head" => Some("Head"),
        "jaw" => Some("Jaw"),
        "lefteye" => Some("LeftEye"),
        "righteye" => Some("RightEye"),
        "leftupperarm" | "leftarm" => Some("LeftArm"),
        "leftforearm" => Some("LeftForeArm"),
        "lefthand" => Some("LeftHand"),
        "rightupperarm" | "rightarm" => Some("RightArm"),
        "rightforearm" => Some("RightForeArm"),
        "righthand" => Some("RightHand"),
        "leftthigh" | "leftleg" => Some("LeftLeg"),
        "leftshin" => Some("LeftShin"),
        "leftfoot" => Some("LeftFoot"),
        "rightthigh" | "rightleg" => Some("RightLeg"),
        "rightshin" => Some("RightShin"),
        "rightfoot" => Some("RightFoot"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soma77_is_complete_and_parent_ordered() {
        assert_eq!(SOMA77.len(), 77);
        for (index, joint) in SOMA77.iter().enumerate() {
            if let Some(parent) = joint.parent {
                let parent_index = SOMA77
                    .iter()
                    .position(|candidate| candidate.name == parent)
                    .unwrap();
                assert!(
                    parent_index < index,
                    "{} parent must precede it",
                    joint.name
                );
            }
        }
    }
}
