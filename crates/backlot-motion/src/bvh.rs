use crate::library::{JointTrack, ProcessedMotionClip};
use crate::retarget::RetargetMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BvhError {
    #[error("BVH is missing MOTION")]
    MissingMotion,
    #[error("invalid BVH frame metadata")]
    InvalidFrameMetadata,
    #[error("invalid BVH numeric value '{0}'")]
    InvalidNumber(String),
    #[error("frame {frame} has {actual} channels; expected {expected}")]
    ChannelCount {
        frame: usize,
        actual: usize,
        expected: usize,
    },
    #[error("BVH has no frames")]
    NoFrames,
    #[error("motion sidecar frame count does not match BVH")]
    SidecarFrameCount,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BvhJoint {
    pub name: String,
    pub parent: Option<usize>,
    pub offset: [f32; 3],
    pub channels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BvhMotion {
    pub joints: Vec<BvhJoint>,
    pub frame_time: f32,
    pub frames: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MotionSidecar {
    pub schema_version: u32,
    pub sample_rate: f32,
    pub root_positions: Vec<[f32; 3]>,
    #[serde(default)]
    pub foot_contacts: Vec<Vec<bool>>,
    #[serde(default)]
    pub foot_positions: Vec<Vec<[f32; 3]>>,
    #[serde(default)]
    pub contact_channels: Vec<String>,
}

/// Convert a Kimodo SOMA BVH plus its NPZ-derived sidecar into the compact,
/// seek-safe runtime representation. Joint tracks are renamed for the target
/// cast but retain source local rotations; Bevy applies them as deltas from
/// frame zero over the cast's native relaxed pose.
pub fn to_processed_clip(
    motion: &BvhMotion,
    sidecar: &MotionSidecar,
    semantic: &str,
    retarget: &RetargetMap,
    looping: bool,
) -> Result<ProcessedMotionClip, BvhError> {
    if motion.frames.is_empty() {
        return Err(BvhError::NoFrames);
    }
    if sidecar.root_positions.len() != motion.frames.len()
        || (!sidecar.foot_contacts.is_empty() && sidecar.foot_contacts.len() != motion.frames.len())
    {
        return Err(BvhError::SidecarFrameCount);
    }

    let mut channel_offsets = Vec::with_capacity(motion.joints.len());
    let mut offset = 0usize;
    for joint in &motion.joints {
        channel_offsets.push(offset);
        offset += joint.channels.len();
    }
    let mut tracks = Vec::new();
    for (joint_index, joint) in motion.joints.iter().enumerate() {
        let Some(mapping) = retarget.target_for(&joint.name) else {
            continue;
        };
        let start = channel_offsets[joint_index];
        let mut rotations = Vec::with_capacity(motion.frames.len());
        let mut translations = Vec::new();
        for frame in &motion.frames {
            let mut q = [0.0, 0.0, 0.0, 1.0];
            let mut translation = [0.0; 3];
            let mut has_translation = false;
            for (channel_index, channel) in joint.channels.iter().enumerate() {
                let value = frame[start + channel_index];
                match channel.as_str() {
                    "Xrotation" => q = quat_mul(q, axis_angle([1.0, 0.0, 0.0], value)),
                    "Yrotation" => q = quat_mul(q, axis_angle([0.0, 1.0, 0.0], value)),
                    "Zrotation" => q = quat_mul(q, axis_angle([0.0, 0.0, 1.0], value)),
                    "Xposition" => {
                        translation[0] = value * 0.01;
                        has_translation = true;
                    }
                    "Yposition" => {
                        translation[1] = value * 0.01;
                        has_translation = true;
                    }
                    "Zposition" => {
                        translation[2] = value * 0.01;
                        has_translation = true;
                    }
                    _ => {}
                }
            }
            rotations.push(quat_normalize(quat_mul(mapping.rest_correction, q)));
            if has_translation {
                translations.push(translation);
            }
        }
        tracks.push(JointTrack {
            joint: mapping.target.clone(),
            rotations,
            translations,
        });
    }

    let sample_rate = if sidecar.sample_rate.is_finite() && sidecar.sample_rate > 0.0 {
        sidecar.sample_rate
    } else {
        1.0 / motion.frame_time.max(0.000_001)
    };
    Ok(ProcessedMotionClip {
        schema_version: 2,
        semantic: semantic.to_string(),
        sample_rate,
        duration: motion.frames.len().saturating_sub(1) as f32 / sample_rate,
        tracks,
        root_positions: sidecar.root_positions.clone(),
        foot_contacts: sidecar.foot_contacts.clone(),
        foot_positions: sidecar.foot_positions.clone(),
        foot_lock_offsets: vec![],
        contact_channels: sidecar.contact_channels.clone(),
        looping,
    })
}

fn axis_angle(axis: [f32; 3], degrees: f32) -> [f32; 4] {
    let half = degrees.to_radians() * 0.5;
    let s = half.sin();
    [axis[0] * s, axis[1] * s, axis[2] * s, half.cos()]
}

fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let length = (q.iter().map(|value| value * value).sum::<f32>())
        .sqrt()
        .max(0.000_001);
    [q[0] / length, q[1] / length, q[2] / length, q[3] / length]
}

pub fn parse_bvh(input: &str) -> Result<BvhMotion, BvhError> {
    let (hierarchy, motion) = input.split_once("MOTION").ok_or(BvhError::MissingMotion)?;
    let mut joints = Vec::<BvhJoint>::new();
    let mut stack = Vec::<usize>::new();
    let mut pending_joint = None;
    for raw in hierarchy.lines() {
        let line = raw.trim();
        if let Some(name) = line
            .strip_prefix("ROOT ")
            .or_else(|| line.strip_prefix("JOINT "))
        {
            let parent = stack.last().copied();
            joints.push(BvhJoint {
                name: name.trim().into(),
                parent,
                offset: [0.0; 3],
                channels: vec![],
            });
            pending_joint = Some(joints.len() - 1);
        } else if line == "{" {
            if let Some(index) = pending_joint.take() {
                stack.push(index);
            }
        } else if line == "}" {
            stack.pop();
        } else if let Some(values) = line.strip_prefix("OFFSET ") {
            if let Some(index) = stack.last().copied() {
                let nums = numbers(values)?;
                if nums.len() >= 3 {
                    joints[index].offset = [nums[0], nums[1], nums[2]];
                }
            }
        } else if let Some(values) = line.strip_prefix("CHANNELS ") {
            if let Some(index) = stack.last().copied() {
                let mut parts = values.split_whitespace();
                let count = parts
                    .next()
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                joints[index].channels = parts.take(count).map(str::to_string).collect();
            }
        }
    }
    let mut lines = motion
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let frame_count = lines
        .next()
        .and_then(|line| line.strip_prefix("Frames:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .ok_or(BvhError::InvalidFrameMetadata)?;
    let frame_time = lines
        .next()
        .and_then(|line| line.strip_prefix("Frame Time:"))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .ok_or(BvhError::InvalidFrameMetadata)?;
    let expected: usize = joints.iter().map(|joint| joint.channels.len()).sum();
    let mut frames = Vec::with_capacity(frame_count);
    for (frame, line) in lines.take(frame_count).enumerate() {
        let values = numbers(line)?;
        if values.len() != expected {
            return Err(BvhError::ChannelCount {
                frame,
                actual: values.len(),
                expected,
            });
        }
        frames.push(values);
    }
    if frames.len() != frame_count {
        return Err(BvhError::InvalidFrameMetadata);
    }
    Ok(BvhMotion {
        joints,
        frame_time,
        frames,
    })
}

fn numbers(input: &str) -> Result<Vec<f32>, BvhError> {
    input
        .split_whitespace()
        .map(|value| {
            value
                .parse::<f32>()
                .map_err(|_| BvhError::InvalidNumber(value.into()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_motion() {
        let bvh = "HIERARCHY\nROOT Hips\n{\nOFFSET 0 0 0\nCHANNELS 6 Xposition Yposition Zposition Zrotation Xrotation Yrotation\nJOINT Chest\n{\nOFFSET 0 1 0\nCHANNELS 3 Zrotation Xrotation Yrotation\n}\n}\nMOTION\nFrames: 1\nFrame Time: 0.033333\n0 0 0 0 0 0 0 0 0";
        let parsed = parse_bvh(bvh).unwrap();
        assert_eq!(parsed.joints.len(), 2);
        assert_eq!(parsed.joints[1].parent, Some(0));
        assert_eq!(parsed.frames[0].len(), 9);
    }

    #[test]
    fn converts_rotation_channels_and_preserves_contacts() {
        let bvh = "HIERARCHY\nROOT Root\n{\nOFFSET 0 0 0\nCHANNELS 6 Xposition Yposition Zposition Zrotation Yrotation Xrotation\nJOINT Hips\n{\nOFFSET 0 100 0\nCHANNELS 3 Zrotation Yrotation Xrotation\n}\n}\nMOTION\nFrames: 2\nFrame Time: 0.033333\n0 0 0 0 0 0 0 0 0\n0 0 0 0 0 0 30 0 0";
        let parsed = parse_bvh(bvh).unwrap();
        let sidecar = MotionSidecar {
            schema_version: 1,
            sample_rate: 30.0,
            root_positions: vec![[0.0; 3], [0.0; 3]],
            foot_contacts: vec![vec![true; 6], vec![false; 6]],
            foot_positions: vec![vec![[0.0; 3]; 6], vec![[0.0; 3]; 6]],
            contact_channels: (0..6).map(|i| format!("contact_{i}")).collect(),
        };
        let map = RetargetMap {
            source_skeleton: "soma77".into(),
            target_skeleton: "cast".into(),
            scale: 1.0,
            joints: vec![crate::retarget::RetargetJoint {
                source: "Hips".into(),
                target: "hips".into(),
                rest_correction: [0.0, 0.0, 0.0, 1.0],
            }],
        };
        let clip = to_processed_clip(&parsed, &sidecar, "turn", &map, false).unwrap();
        assert_eq!(clip.tracks.len(), 1);
        assert_eq!(clip.foot_contacts[0].len(), 6);
        assert_ne!(clip.tracks[0].rotations[0], clip.tracks[0].rotations[1]);
    }
}
