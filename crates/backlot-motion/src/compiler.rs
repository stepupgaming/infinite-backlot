use crate::library::ProcessedMotionClip;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MotionSource {
    Native,
    Procedural,
    ApprovedLibrary,
    EpisodeKimodo,
    GeneratedTransition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PoseSummary {
    pub root: [f32; 3],
    pub velocity: [f32; 3],
    pub joints: BTreeMap<String, [f32; 4]>,
    pub contacts: Vec<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimedInteractionEvent {
    pub normalized_time: f32,
    pub event: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MotionSegment {
    pub actor: String,
    pub semantic: String,
    pub source: MotionSource,
    pub clip: Option<PathBuf>,
    pub start: f32,
    pub duration: f32,
    pub start_pose: PoseSummary,
    pub end_pose: PoseSummary,
    pub interruptible: bool,
    pub interaction_events: Vec<TimedInteractionEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransitionDecision {
    DirectBlend {
        duration: f32,
    },
    ProceduralBridge {
        duration: f32,
    },
    GenerateKimodoBridge {
        duration: f32,
        prompt: String,
        start_pose: PoseSummary,
        end_pose: PoseSummary,
    },
    Incompatible {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct TransitionPolicy {
    pub direct_root_distance: f32,
    pub bridge_root_distance: f32,
    pub direct_pose_radians: f32,
    pub bridge_pose_radians: f32,
    pub direct_velocity_delta: f32,
}

impl Default for TransitionPolicy {
    fn default() -> Self {
        Self {
            direct_root_distance: 0.08,
            bridge_root_distance: 0.65,
            direct_pose_radians: 0.55,
            bridge_pose_radians: 1.35,
            direct_velocity_delta: 0.45,
        }
    }
}

pub fn summarize_clip(clip: &ProcessedMotionClip, at_end: bool) -> PoseSummary {
    let index = if at_end {
        clip.root_positions.len().saturating_sub(1)
    } else {
        0
    };
    let previous = index.saturating_sub(1);
    let root = clip.root_positions.get(index).copied().unwrap_or([0.0; 3]);
    let prev = clip.root_positions.get(previous).copied().unwrap_or(root);
    let dt = (index.saturating_sub(previous) as f32 / clip.sample_rate.max(0.001)).max(0.001);
    let velocity = [
        (root[0] - prev[0]) / dt,
        (root[1] - prev[1]) / dt,
        (root[2] - prev[2]) / dt,
    ];
    let joints = clip
        .tracks
        .iter()
        .filter_map(|track| {
            track
                .rotations
                .get(index)
                .copied()
                .map(|q| (track.joint.clone(), q))
        })
        .collect();
    PoseSummary {
        root,
        velocity,
        joints,
        contacts: clip.foot_contacts.get(index).cloned().unwrap_or_default(),
    }
}

pub fn classify_transition(
    from: &MotionSegment,
    to: &MotionSegment,
    policy: &TransitionPolicy,
) -> TransitionDecision {
    let root_distance = distance(from.end_pose.root, to.start_pose.root);
    let velocity_delta = distance(from.end_pose.velocity, to.start_pose.velocity);
    let pose_delta = shared_pose_delta(&from.end_pose.joints, &to.start_pose.joints);
    let contact_conflict = planted(&from.end_pose.contacts) && !planted(&to.start_pose.contacts);

    if root_distance <= policy.direct_root_distance
        && pose_delta <= policy.direct_pose_radians
        && velocity_delta <= policy.direct_velocity_delta
        && !contact_conflict
    {
        return TransitionDecision::DirectBlend { duration: 0.4 };
    }
    if root_distance <= policy.bridge_root_distance && pose_delta <= policy.bridge_pose_radians {
        return TransitionDecision::ProceduralBridge {
            duration: (0.45 + root_distance).clamp(0.5, 1.2),
        };
    }
    if from.interruptible {
        return TransitionDecision::GenerateKimodoBridge {
            duration: 1.2,
            prompt: format!(
                "Smoothly recover from {} and transition into {}, preserving planted feet before moving.",
                from.semantic, to.semantic
            ),
            start_pose: from.end_pose.clone(),
            end_pose: to.start_pose.clone(),
        };
    }
    TransitionDecision::Incompatible {
        reason: format!(
            "{} -> {} exceeds pose/root limits and the first segment is not interruptible",
            from.semantic, to.semantic
        ),
    }
}

fn planted(contacts: &[bool]) -> bool {
    contacts.iter().any(|value| *value)
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn shared_pose_delta(a: &BTreeMap<String, [f32; 4]>, b: &BTreeMap<String, [f32; 4]>) -> f32 {
    let mut total = 0.0;
    let mut count = 0usize;
    for (joint, qa) in a {
        let Some(qb) = b.get(joint) else { continue };
        let dot = (qa[0] * qb[0] + qa[1] * qb[1] + qa[2] * qb[2] + qa[3] * qb[3])
            .abs()
            .clamp(0.0, 1.0);
        total += 2.0 * dot.acos();
        count += 1;
    }
    if count == 0 {
        f32::INFINITY
    } else {
        total / count as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(root: [f32; 3], angle: f32, contacts: Vec<bool>) -> PoseSummary {
        let mut joints = BTreeMap::new();
        joints.insert(
            "hips".into(),
            [0.0, (angle * 0.5).sin(), 0.0, (angle * 0.5).cos()],
        );
        PoseSummary {
            root,
            velocity: [0.0; 3],
            joints,
            contacts,
        }
    }

    fn segment(name: &str, start: PoseSummary, end: PoseSummary) -> MotionSegment {
        MotionSegment {
            actor: "mara".into(),
            semantic: name.into(),
            source: MotionSource::ApprovedLibrary,
            clip: None,
            start: 0.0,
            duration: 1.0,
            start_pose: start,
            end_pose: end,
            interruptible: true,
            interaction_events: vec![],
        }
    }

    #[test]
    fn compatible_segments_crossfade() {
        let a = segment(
            "point",
            pose([0.0; 3], 0.0, vec![false]),
            pose([0.0; 3], 0.1, vec![false]),
        );
        let b = segment(
            "listen",
            pose([0.03, 0.0, 0.0], 0.15, vec![false]),
            pose([0.0; 3], 0.0, vec![false]),
        );
        assert!(matches!(
            classify_transition(&a, &b, &TransitionPolicy::default()),
            TransitionDecision::DirectBlend { .. }
        ));
    }

    #[test]
    fn incompatible_segments_request_generated_bridge() {
        let a = segment(
            "crouch",
            pose([0.0; 3], 0.0, vec![true]),
            pose([0.0; 3], 2.0, vec![true]),
        );
        let b = segment(
            "run",
            pose([2.0, 0.0, 0.0], 0.0, vec![false]),
            pose([3.0, 0.0, 0.0], 0.0, vec![false]),
        );
        assert!(matches!(
            classify_transition(&a, &b, &TransitionPolicy::default()),
            TransitionDecision::GenerateKimodoBridge { .. }
        ));
    }
}
