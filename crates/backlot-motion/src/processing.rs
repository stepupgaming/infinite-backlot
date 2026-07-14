use crate::library::ProcessedMotionClip;
use crate::retarget::warp_root_to_path;

#[derive(Debug, Clone)]
pub struct MotionProcessingConfig {
    pub target_sample_rate: f32,
    pub floor_height: f32,
    pub root_path: Vec<[f32; 3]>,
    pub max_contact_drift: f32,
}

impl Default for MotionProcessingConfig {
    fn default() -> Self {
        Self {
            target_sample_rate: 30.0,
            floor_height: 0.0,
            root_path: vec![],
            max_contact_drift: 0.02,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MotionValidation {
    pub valid: bool,
    pub frame_count: usize,
    pub contact_drift: f32,
    pub errors: Vec<String>,
}

pub fn process_clip(
    clip: &mut ProcessedMotionClip,
    config: &MotionProcessingConfig,
) -> MotionValidation {
    let mut errors = Vec::new();
    if clip.sample_rate <= 0.0 || !clip.sample_rate.is_finite() {
        errors.push("invalid sample rate".into());
    }
    if clip.root_positions.is_empty() {
        errors.push("clip has no root positions".into());
    }
    if !config.root_path.is_empty() {
        warp_root_to_path(&mut clip.root_positions, &config.root_path);
    }
    let floor_offset = clip
        .root_positions
        .iter()
        .map(|p| p[1])
        .fold(f32::INFINITY, f32::min);
    if floor_offset.is_finite() {
        for position in &mut clip.root_positions {
            position[1] += config.floor_height - floor_offset;
        }
    }
    let frame_count = clip.root_positions.len();
    if !clip.foot_contacts.is_empty() && clip.foot_contacts.len() != frame_count {
        errors.push("foot contact count does not match root frames".into());
    }
    if !clip.foot_positions.is_empty() && clip.foot_positions.len() != frame_count {
        errors.push("foot position count does not match root frames".into());
    }
    let contact_drift = if errors.is_empty() {
        lock_contacted_feet(clip)
    } else {
        f32::INFINITY
    };
    if contact_drift > config.max_contact_drift {
        errors.push(format!(
            "foot contact drift {:.4} m exceeds {:.4} m",
            contact_drift, config.max_contact_drift
        ));
    }
    clip.duration = frame_count.saturating_sub(1) as f32 / clip.sample_rate.max(0.001);
    MotionValidation {
        valid: errors.is_empty(),
        frame_count,
        contact_drift,
        errors,
    }
}

/// Correct root translation so each continuous planted-contact interval keeps
/// its SOMA contact joint at a stable world-space anchor. Multiple simultaneous
/// contacts contribute an averaged correction, avoiding a hard snap to either
/// foot while still keeping measured drift bounded.
fn lock_contacted_feet(clip: &mut ProcessedMotionClip) -> f32 {
    if clip.foot_contacts.is_empty() || clip.foot_positions.is_empty() {
        return 0.0;
    }
    let channels = clip
        .foot_contacts
        .iter()
        .map(Vec::len)
        .min()
        .unwrap_or(0)
        .min(clip.foot_positions.iter().map(Vec::len).min().unwrap_or(0));
    let mut anchors: Vec<Option<[f32; 3]>> = vec![None; channels];
    let mut corrected_positions = clip.foot_positions.clone();
    let mut lock_offsets = clip
        .foot_positions
        .iter()
        .map(|feet| vec![[0.0; 3]; feet.len()])
        .collect::<Vec<_>>();
    let mut worst = 0.0f32;
    for frame in 0..clip.root_positions.len() {
        let mut correction = [0.0f32; 3];
        let mut count = 0.0f32;
        for channel in 0..channels {
            if clip.foot_contacts[frame][channel] {
                let foot = corrected_positions[frame][channel];
                let anchor = *anchors[channel].get_or_insert(foot);
                correction[0] += anchor[0] - foot[0];
                correction[1] += anchor[1] - foot[1];
                correction[2] += anchor[2] - foot[2];
                count += 1.0;
            } else {
                anchors[channel] = None;
            }
        }
        if count > 0.0 {
            correction = [
                correction[0] / count,
                correction[1] / count,
                correction[2] / count,
            ];
            clip.root_positions[frame][0] += correction[0];
            clip.root_positions[frame][2] += correction[2];
            for foot in &mut corrected_positions[frame] {
                foot[0] += correction[0];
                foot[2] += correction[2];
            }
            for channel in 0..channels {
                if let Some(anchor) = anchors[channel] {
                    let foot = corrected_positions[frame][channel];
                    let offset = [
                        anchor[0] - foot[0],
                        anchor[1] - foot[1],
                        anchor[2] - foot[2],
                    ];
                    lock_offsets[frame][channel] = offset;
                    let corrected = [
                        foot[0] + offset[0],
                        foot[1] + offset[1],
                        foot[2] + offset[2],
                    ];
                    let drift = ((anchor[0] - corrected[0]).powi(2)
                        + (anchor[2] - corrected[2]).powi(2))
                    .sqrt();
                    worst = worst.max(drift);
                }
            }
        }
    }
    clip.foot_positions = corrected_positions;
    clip.foot_lock_offsets = lock_offsets;
    worst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_path_reaches_authored_destination() {
        let mut clip = ProcessedMotionClip {
            schema_version: 1,
            semantic: "walk".into(),
            sample_rate: 30.0,
            duration: 0.0,
            tracks: vec![],
            root_positions: vec![[0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
            foot_contacts: vec![],
            foot_positions: vec![],
            foot_lock_offsets: vec![],
            contact_channels: vec![],
            looping: false,
        };
        let config = MotionProcessingConfig {
            root_path: vec![[2.0, 0.0, 3.0], [4.0, 0.0, 6.0]],
            ..Default::default()
        };
        let result = process_clip(&mut clip, &config);
        assert!(result.valid);
        assert_eq!(clip.root_positions[0][0], 2.0);
        assert_eq!(clip.root_positions[1][2], 6.0);
        assert_eq!(clip.root_positions[0][1], 0.0);
    }
}
