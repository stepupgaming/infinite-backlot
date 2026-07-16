use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub type Point3 = [f32; 3];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SomaFrame {
    pub time: f32,
    pub joints: Vec<Point3>,
    pub root_heading: Point3,
    #[serde(default)]
    pub foot_contacts: [bool; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SomaMotionTrack {
    pub schema_version: u32,
    pub fps: u32,
    pub joint_names: Vec<String>,
    pub frames: Vec<SomaFrame>,
    #[serde(default)]
    pub source_segments: Vec<String>,
}

impl SomaMotionTrack {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.fps == 0 || self.frames.is_empty() {
            return Err("invalid SOMA motion track header".into());
        }
        let joints = self.joint_names.len();
        if joints == 0 {
            return Err("SOMA motion track has no joints".into());
        }
        for frame in &self.frames {
            if frame.joints.len() != joints {
                return Err("SOMA frame joint count does not match joint_names".into());
            }
            if frame
                .joints
                .iter()
                .flatten()
                .chain(frame.root_heading.iter())
                .any(|value| !value.is_finite())
            {
                return Err("SOMA track contains non-finite values".into());
            }
        }
        Ok(())
    }

    pub fn joint_index(&self, name: &str) -> Result<usize, String> {
        self.joint_names
            .iter()
            .position(|candidate| candidate == name)
            .ok_or_else(|| format!("SOMA track has no joint {name}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContactCorrectionRequest {
    pub id: String,
    pub shoulder_joint: String,
    pub elbow_joint: String,
    pub hand_joint: String,
    pub target: Point3,
    pub contact_start: f32,
    pub contact_end: f32,
    pub blend_seconds: f32,
    pub maximum_correction: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContactCorrectionResult {
    pub id: String,
    pub before_error_m: f32,
    pub after_error_m: f32,
    pub maximum_applied_correction_m: f32,
    pub corrected_frames: u32,
    pub accepted: bool,
}

pub fn apply_two_bone_contact_correction(
    track: &mut SomaMotionTrack,
    request: &ContactCorrectionRequest,
) -> Result<ContactCorrectionResult, String> {
    track.validate()?;
    if request.contact_end <= request.contact_start || request.maximum_correction <= 0.0 {
        return Err(format!("invalid contact correction {}", request.id));
    }
    let shoulder = track.joint_index(&request.shoulder_joint)?;
    let elbow = track.joint_index(&request.elbow_joint)?;
    let hand = track.joint_index(&request.hand_joint)?;
    let center_time = (request.contact_start + request.contact_end) * 0.5;
    let center_frame = ((center_time * track.fps as f32).round() as usize)
        .min(track.frames.len().saturating_sub(1));
    let before_error_m = distance(track.frames[center_frame].joints[hand], request.target);
    let window_start = (request.contact_start - request.blend_seconds).max(0.0);
    let window_end = request.contact_end + request.blend_seconds;
    let mut maximum_applied_correction_m = 0.0_f32;
    let mut corrected_frames = 0_u32;

    for frame in &mut track.frames {
        if frame.time < window_start || frame.time > window_end {
            continue;
        }
        let weight = if frame.time < request.contact_start {
            smoothstep(inverse_lerp(
                window_start,
                request.contact_start,
                frame.time,
            ))
        } else if frame.time <= request.contact_end {
            1.0
        } else {
            smoothstep(1.0 - inverse_lerp(request.contact_end, window_end, frame.time))
        };
        if weight <= 0.0 {
            continue;
        }
        let original_shoulder = frame.joints[shoulder];
        let original_elbow = frame.joints[elbow];
        let original_hand = frame.joints[hand];
        let upper_length = distance(original_shoulder, original_elbow).max(0.05);
        let lower_length = distance(original_elbow, original_hand).max(0.05);
        let maximum_reach = upper_length + lower_length - 0.005;
        let raw_target_distance = distance(request.target, original_shoulder);
        let shoulder_compensation = (raw_target_distance - maximum_reach).clamp(0.0, 0.08);
        let compensated_shoulder = add(
            original_shoulder,
            scale(
                normalize(sub(request.target, original_shoulder)),
                shoulder_compensation,
            ),
        );
        let working_shoulder = lerp(original_shoulder, compensated_shoulder, weight);
        frame.joints[shoulder] = working_shoulder;
        let to_target = sub(request.target, working_shoulder);
        let target_distance = length(to_target).max(1e-5);
        let direction = scale(to_target, 1.0 / target_distance);
        let reach =
            target_distance.clamp((upper_length - lower_length).abs() + 0.005, maximum_reach);
        let hand_target = if target_distance <= maximum_reach {
            request.target
        } else {
            add(working_shoulder, scale(direction, reach))
        };
        let bend = sub(original_elbow, working_shoulder);
        let projected = scale(direction, dot(bend, direction));
        let mut perpendicular = sub(bend, projected);
        if length(perpendicular) < 1e-5 {
            perpendicular = cross(direction, [0.0, 1.0, 0.0]);
            if length(perpendicular) < 1e-5 {
                perpendicular = cross(direction, [1.0, 0.0, 0.0]);
            }
        }
        perpendicular = normalize(perpendicular);
        let x = ((upper_length * upper_length - lower_length * lower_length + reach * reach)
            / (2.0 * reach))
            .clamp(0.0, upper_length);
        let y = (upper_length * upper_length - x * x).max(0.0).sqrt();
        let solved_elbow = add(
            add(working_shoulder, scale(direction, x)),
            scale(perpendicular, y),
        );
        let solved_hand = hand_target;
        frame.joints[elbow] = lerp(original_elbow, solved_elbow, weight);
        frame.joints[hand] = lerp(original_hand, solved_hand, weight);
        maximum_applied_correction_m = maximum_applied_correction_m
            .max(distance(original_shoulder, frame.joints[shoulder]))
            .max(distance(original_elbow, frame.joints[elbow]))
            .max(distance(original_hand, frame.joints[hand]));
        corrected_frames += 1;
    }
    let after_error_m = distance(track.frames[center_frame].joints[hand], request.target);
    Ok(ContactCorrectionResult {
        id: request.id.clone(),
        before_error_m,
        after_error_m,
        maximum_applied_correction_m,
        corrected_frames,
        accepted: after_error_m <= 0.05
            && maximum_applied_correction_m <= request.maximum_correction + 1e-4,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MovingContactCorrectionRequest {
    pub id: String,
    pub shoulder_joint: String,
    pub elbow_joint: String,
    pub hand_joint: String,
    pub target_samples: Vec<(f32, Point3)>,
    pub contact_start: f32,
    pub contact_end: f32,
    pub blend_seconds: f32,
    pub maximum_correction: f32,
}

pub fn apply_two_bone_contact_trajectory(
    track: &mut SomaMotionTrack,
    request: &MovingContactCorrectionRequest,
) -> Result<ContactCorrectionResult, String> {
    track.validate()?;
    if request.target_samples.is_empty() {
        return Err("moving contact correction needs target samples".into());
    }
    let shoulder_index = track.joint_index(&request.shoulder_joint)?;
    let elbow_index = track.joint_index(&request.elbow_joint)?;
    let hand_index = track.joint_index(&request.hand_joint)?;
    let center_time = (request.contact_start + request.contact_end) * 0.5;
    let center_frame = track
        .frames
        .iter()
        .min_by(|left, right| {
            (left.time - center_time)
                .abs()
                .total_cmp(&(right.time - center_time).abs())
        })
        .ok_or_else(|| "SOMA track has no frames".to_string())?;
    let center_target = sampled_target(&request.target_samples, center_time);
    let before_error = distance(center_frame.joints[hand_index], center_target);
    let mut maximum_applied_correction: f32 = 0.0;
    let mut after_error: f32 = 0.0;
    let mut corrected_frames = 0_u32;
    for frame in &mut track.frames {
        let target = sampled_target(&request.target_samples, frame.time);
        let weight = contact_weight(
            frame.time,
            request.contact_start,
            request.contact_end,
            request.blend_seconds,
        );
        if weight <= 0.0 {
            continue;
        }
        let original_shoulder = frame.joints[shoulder_index];
        let elbow = frame.joints[elbow_index];
        let hand = frame.joints[hand_index];
        let upper = distance(original_shoulder, elbow).max(0.05);
        let lower = distance(elbow, hand).max(0.05);
        let maximum_reach = upper + lower - 1e-4;
        let raw_distance = distance(target, original_shoulder);
        let shoulder_compensation = (raw_distance - maximum_reach).clamp(0.0, 0.08);
        let compensated_shoulder = add(
            original_shoulder,
            scale(
                normalize(sub(target, original_shoulder)),
                shoulder_compensation,
            ),
        );
        let shoulder = lerp(original_shoulder, compensated_shoulder, weight);
        frame.joints[shoulder_index] = shoulder;
        let delta = sub(target, shoulder);
        let requested_distance = length(delta).max(1e-5);
        let solved_distance = requested_distance.clamp((upper - lower).abs() + 1e-4, maximum_reach);
        let direction = scale(delta, 1.0 / requested_distance);
        let bend = sub(elbow, shoulder);
        let projected = scale(direction, dot(bend, direction));
        let mut perpendicular = sub(bend, projected);
        if length(perpendicular) < 1e-5 {
            perpendicular = cross(direction, [0.0, 1.0, 0.0]);
            if length(perpendicular) < 1e-5 {
                perpendicular = cross(direction, [1.0, 0.0, 0.0]);
            }
        }
        perpendicular = normalize(perpendicular);
        let along = (upper * upper - lower * lower + solved_distance * solved_distance)
            / (2.0 * solved_distance);
        let out = (upper * upper - along * along).max(0.0).sqrt();
        let solved_elbow = add(
            add(shoulder, scale(direction, along)),
            scale(perpendicular, out),
        );
        let solved_hand = add(shoulder, scale(direction, solved_distance));
        frame.joints[elbow_index] = lerp(elbow, solved_elbow, weight);
        frame.joints[hand_index] = lerp(hand, solved_hand, weight);
        maximum_applied_correction = maximum_applied_correction
            .max(distance(original_shoulder, frame.joints[shoulder_index]))
            .max(distance(hand, frame.joints[hand_index]));
        corrected_frames += 1;
        if (request.contact_start..=request.contact_end).contains(&frame.time) {
            after_error = after_error.max(distance(frame.joints[hand_index], target));
        }
    }
    Ok(ContactCorrectionResult {
        id: request.id.clone(),
        before_error_m: before_error,
        after_error_m: after_error,
        maximum_applied_correction_m: maximum_applied_correction,
        corrected_frames,
        accepted: after_error <= 0.05 && maximum_applied_correction <= request.maximum_correction,
    })
}

fn sampled_target(samples: &[(f32, Point3)], time: f32) -> Point3 {
    if time <= samples[0].0 {
        return samples[0].1;
    }
    for pair in samples.windows(2) {
        if time <= pair[1].0 {
            let span = (pair[1].0 - pair[0].0).max(1e-5);
            return lerp(
                pair[0].1,
                pair[1].1,
                ((time - pair[0].0) / span).clamp(0.0, 1.0),
            );
        }
    }
    samples.last().unwrap().1
}

fn contact_weight(time: f32, start: f32, end: f32, blend: f32) -> f32 {
    if (start..=end).contains(&time) {
        1.0
    } else if time < start && time >= start - blend {
        smoothstep((time - (start - blend)) / blend.max(1e-5))
    } else if time > end && time <= end + blend {
        smoothstep(1.0 - (time - end) / blend.max(1e-5))
    } else {
        0.0
    }
}

pub fn concatenate_tracks(
    tracks: &[SomaMotionTrack],
    transition_seconds: f32,
) -> Result<SomaMotionTrack, String> {
    let first = tracks
        .first()
        .ok_or_else(|| "no SOMA tracks to concatenate".to_string())?;
    first.validate()?;
    let mut output = first.clone();
    let transition_frames = (transition_seconds.max(0.0) * first.fps as f32).round() as usize;
    for track in &tracks[1..] {
        track.validate()?;
        if track.fps != output.fps || track.joint_names != output.joint_names {
            return Err("SOMA segment contracts do not match".into());
        }
        let prior = output.frames.last().unwrap().joints.clone();
        let time_offset = output.frames.last().unwrap().time + 1.0 / output.fps as f32;
        for (index, source) in track.frames.iter().enumerate() {
            let mut frame = source.clone();
            if index < transition_frames {
                let weight = smoothstep((index + 1) as f32 / (transition_frames + 1) as f32);
                for (joint, previous) in frame.joints.iter_mut().zip(&prior) {
                    *joint = lerp(*previous, *joint, weight);
                }
            }
            frame.time = time_offset + index as f32 / output.fps as f32;
            output.frames.push(frame);
        }
        output.source_segments.extend(track.source_segments.clone());
    }
    Ok(output)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BodyCollisionReport {
    pub root_collision_samples: u32,
    pub body_collision_samples: u32,
    pub limb_collision_samples: u32,
    pub maximum_penetration_m: f32,
    pub colliding_body_parts: Vec<String>,
    pub colliding_environment_objects: Vec<String>,
    pub valid: bool,
}

#[derive(Debug, Clone)]
struct EnvironmentCollider {
    id: String,
    center: Point3,
    half_extents: Point3,
}

pub fn evaluate_body_collisions(
    track: &SomaMotionTrack,
    collider_values: &[Value],
    ignored_objects: &[String],
) -> Result<BodyCollisionReport, String> {
    track.validate()?;
    let ignored = ignored_objects.iter().cloned().collect::<BTreeSet<_>>();
    let colliders = collider_values
        .iter()
        .filter_map(|value| {
            let id = value.get("id")?.as_str()?.to_string();
            if ignored.contains(&id) {
                return None;
            }
            Some(EnvironmentCollider {
                id,
                center: value_to_point(value.get("center")?)?,
                half_extents: value_to_point(value.get("half_extents")?)?,
            })
        })
        .collect::<Vec<_>>();
    let index = |name: &str| {
        track
            .joint_names
            .iter()
            .position(|candidate| candidate == name)
    };
    let mut report = BodyCollisionReport {
        root_collision_samples: 0,
        body_collision_samples: 0,
        limb_collision_samples: 0,
        maximum_penetration_m: 0.0,
        colliding_body_parts: Vec::new(),
        colliding_environment_objects: Vec::new(),
        valid: true,
    };
    let mut parts = BTreeSet::new();
    let mut objects = BTreeSet::new();

    for frame in &track.frames {
        let mut volumes: Vec<(&str, CollisionClass, Point3, Point3, f32)> = Vec::new();
        if let (Some(hips), Some(chest)) = (index("Hips"), index("Chest")) {
            volumes.push((
                "RootPelvisCapsule",
                CollisionClass::Root,
                frame.joints[hips],
                frame.joints[chest],
                0.20,
            ));
            volumes.push((
                "TorsoCapsule",
                CollisionClass::Body,
                frame.joints[hips],
                frame.joints[chest],
                0.22,
            ));
        }
        if let Some(head) = index("Head") {
            volumes.push((
                "HeadSphere",
                CollisionClass::Body,
                frame.joints[head],
                frame.joints[head],
                0.14,
            ));
        }
        for (label, a, b, radius) in [
            ("LeftUpperArm", "LeftArm", "LeftForeArm", 0.075),
            ("LeftForeArm", "LeftForeArm", "LeftHand", 0.065),
            ("RightUpperArm", "RightArm", "RightForeArm", 0.075),
            ("RightForeArm", "RightForeArm", "RightHand", 0.065),
            ("LeftUpperLeg", "LeftLeg", "LeftShin", 0.105),
            ("LeftLowerLeg", "LeftShin", "LeftFoot", 0.09),
            ("RightUpperLeg", "RightLeg", "RightShin", 0.105),
            ("RightLowerLeg", "RightShin", "RightFoot", 0.09),
        ] {
            if let (Some(a), Some(b)) = (index(a), index(b)) {
                volumes.push((
                    label,
                    CollisionClass::Limb,
                    frame.joints[a],
                    frame.joints[b],
                    radius,
                ));
            }
        }
        for (label, joint, radius) in [
            ("LeftHand", "LeftHand", 0.085),
            ("RightHand", "RightHand", 0.085),
            ("LeftFoot", "LeftFoot", 0.11),
            ("RightFoot", "RightFoot", 0.11),
        ] {
            if let Some(joint) = index(joint) {
                volumes.push((
                    label,
                    CollisionClass::Limb,
                    frame.joints[joint],
                    frame.joints[joint],
                    radius,
                ));
            }
        }
        for (label, class, start, end, radius) in volumes {
            for collider in &colliders {
                let penetration = segment_aabb_penetration(start, end, radius, collider);
                if penetration <= 0.0 {
                    continue;
                }
                match class {
                    CollisionClass::Root => report.root_collision_samples += 1,
                    CollisionClass::Body => report.body_collision_samples += 1,
                    CollisionClass::Limb => report.limb_collision_samples += 1,
                }
                report.maximum_penetration_m = report.maximum_penetration_m.max(penetration);
                parts.insert(label.to_string());
                objects.insert(collider.id.clone());
            }
        }
    }
    report.colliding_body_parts = parts.into_iter().collect();
    report.colliding_environment_objects = objects.into_iter().collect();
    report.valid = report.root_collision_samples == 0
        && report.body_collision_samples == 0
        && report.limb_collision_samples == 0;
    Ok(report)
}

#[derive(Debug, Clone, Copy)]
enum CollisionClass {
    Root,
    Body,
    Limb,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProceduralGaitReport {
    pub applied: bool,
    pub traveled_distance_m: f32,
    pub step_count: u32,
    pub maximum_joint_correction_m: f32,
}

pub fn apply_procedural_soma_gait(
    track: &mut SomaMotionTrack,
) -> Result<ProceduralGaitReport, String> {
    track.validate()?;
    let hips_i = track.joint_index("Hips")?;
    let left_leg = track.joint_index("LeftLeg")?;
    let left_shin = track.joint_index("LeftShin")?;
    let left_foot = track.joint_index("LeftFoot")?;
    let left_toe = track.joint_indices_optional("LeftToeBase");
    let right_leg = track.joint_index("RightLeg")?;
    let right_shin = track.joint_index("RightShin")?;
    let right_foot = track.joint_index("RightFoot")?;
    let right_toe = track.joint_indices_optional("RightToeBase");
    let mut cumulative = vec![0.0_f32; track.frames.len()];
    for index in 1..track.frames.len() {
        cumulative[index] = cumulative[index - 1]
            + length([
                track.frames[index].joints[hips_i][0] - track.frames[index - 1].joints[hips_i][0],
                0.0,
                track.frames[index].joints[hips_i][2] - track.frames[index - 1].joints[hips_i][2],
            ]);
    }
    let traveled = *cumulative.last().unwrap_or(&0.0);
    if traveled < 0.25 {
        return Ok(ProceduralGaitReport {
            applied: false,
            traveled_distance_m: traveled,
            step_count: 0,
            maximum_joint_correction_m: 0.0,
        });
    }
    let first = &track.frames[0];
    let upper = ((distance(first.joints[left_leg], first.joints[left_shin])
        + distance(first.joints[right_leg], first.joints[right_shin]))
        * 0.5)
        .max(0.25);
    let lower = ((distance(first.joints[left_shin], first.joints[left_foot])
        + distance(first.joints[right_shin], first.joints[right_foot]))
        * 0.5)
        .max(0.25);
    let floor = track
        .frames
        .iter()
        .flat_map(|frame| [frame.joints[left_foot][1], frame.joints[right_foot][1]])
        .fold(f32::INFINITY, f32::min)
        + 0.04;
    let mut forward = normalize(track.frames[0].root_heading);
    if length(forward) < 0.5 {
        forward = [0.0, 0.0, -1.0];
    }
    let side = [forward[2], 0.0, -forward[0]];
    let initial_root = track.frames[0].joints[hips_i];
    let mut anchors = [
        add(
            add([initial_root[0], floor, initial_root[2]], scale(side, 0.11)),
            scale(forward, -0.10),
        ),
        add(
            add(
                [initial_root[0], floor, initial_root[2]],
                scale(side, -0.11),
            ),
            scale(forward, 0.10),
        ),
    ];
    let mut support = 0_usize;
    let mut swing = 1_usize;
    let mut swing_start = anchors[swing];
    let mut last_feet = anchors;
    let step_length = 0.55_f32;
    let mut step_start = 0.0_f32;
    let mut next_step = step_length;
    let mut step_count = 0_u32;
    let mut maximum_joint_correction_m = 0.0_f32;
    for index in 0..track.frames.len() {
        let root = track.frames[index].joints[hips_i];
        let authored_heading = normalize(track.frames[index].root_heading);
        if length(authored_heading) > 0.5 {
            forward = authored_heading;
        }
        let side_axis = [forward[2], 0.0, -forward[0]];
        while cumulative[index] >= next_step {
            anchors[swing] = last_feet[swing];
            support = swing;
            swing = 1 - support;
            swing_start = last_feet[swing];
            step_start = next_step;
            next_step += step_length;
            step_count += 1;
        }
        let phase = ((cumulative[index] - step_start) / step_length).clamp(0.0, 1.0);
        let smooth = phase * phase * (3.0 - 2.0 * phase);
        let swing_end = add(
            add(
                [root[0], floor, root[2]],
                scale(side_axis, if swing == 0 { 0.11 } else { -0.11 }),
            ),
            scale(forward, 0.32),
        );
        let mut feet = anchors;
        feet[swing] = lerp(swing_start, swing_end, smooth);
        feet[swing][1] = floor + (std::f32::consts::PI * phase).sin().max(0.0) * 0.09;
        feet[support] = anchors[support];
        last_feet = feet;
        let speed = if index == 0 {
            0.0
        } else {
            (cumulative[index] - cumulative[index - 1]) * track.fps as f32
        };
        track.frames[index].foot_contacts = if speed < 0.08 {
            [true, true]
        } else if support == 0 {
            [true, false]
        } else {
            [false, true]
        };
        for (side_index, (leg_i, shin_i, foot_i, toe_i)) in [
            (left_leg, left_shin, left_foot, left_toe),
            (right_leg, right_shin, right_foot, right_toe),
        ]
        .into_iter()
        .enumerate()
        {
            let original = [
                track.frames[index].joints[leg_i],
                track.frames[index].joints[shin_i],
                track.frames[index].joints[foot_i],
            ];
            let hip = add(
                [root[0], root[1] - 0.05, root[2]],
                scale(side_axis, if side_index == 0 { 0.10 } else { -0.10 }),
            );
            let target = feet[side_index];
            let delta = sub(target, hip);
            let requested = length(delta).max(1e-4);
            let direction = scale(delta, 1.0 / requested);
            let reach = requested.clamp((upper - lower).abs() + 0.01, upper + lower - 0.01);
            let solved_foot = add(hip, scale(direction, reach));
            let x =
                ((upper * upper - lower * lower + reach * reach) / (2.0 * reach)).clamp(0.0, upper);
            let y = (upper * upper - x * x).max(0.0).sqrt();
            let pole = normalize(cross(cross(direction, forward), direction));
            let knee = add(add(hip, scale(direction, x)), scale(pole, y));
            track.frames[index].joints[leg_i] = hip;
            track.frames[index].joints[shin_i] = knee;
            track.frames[index].joints[foot_i] = solved_foot;
            if let Some(toe_i) = toe_i {
                track.frames[index].joints[toe_i] = add(solved_foot, scale(forward, 0.16));
            }
            maximum_joint_correction_m = maximum_joint_correction_m
                .max(distance(original[0], hip))
                .max(distance(original[1], knee))
                .max(distance(original[2], solved_foot));
        }
    }
    Ok(ProceduralGaitReport {
        applied: true,
        traveled_distance_m: traveled,
        step_count,
        maximum_joint_correction_m,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RootHeightStabilizationReport {
    pub corrected_frames: u32,
    pub maximum_correction_m: f32,
}

pub fn stabilize_root_height(
    track: &mut SomaMotionTrack,
    allowed_deviation_m: f32,
    maximum_step_m: f32,
) -> Result<RootHeightStabilizationReport, String> {
    track.validate()?;
    let hips = track.joint_index("Hips")?;
    let mut heights = track
        .frames
        .iter()
        .map(|frame| frame.joints[hips][1])
        .collect::<Vec<_>>();
    heights.sort_by(f32::total_cmp);
    let median = heights[heights.len() / 2];
    let mut corrected_frames = 0;
    let mut maximum_correction_m = 0.0_f32;
    let mut previous = median;
    for frame in &mut track.frames {
        let original = frame.joints[hips][1];
        let target = original
            .clamp(median - allowed_deviation_m, median + allowed_deviation_m)
            .clamp(previous - maximum_step_m, previous + maximum_step_m);
        let correction = target - original;
        if correction.abs() > 1e-5 {
            for joint in &mut frame.joints {
                joint[1] += correction;
            }
            corrected_frames += 1;
            maximum_correction_m = maximum_correction_m.max(correction.abs());
        }
        previous = target;
    }
    Ok(RootHeightStabilizationReport {
        corrected_frames,
        maximum_correction_m,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TorsoUprightCorrectionReport {
    pub corrected_frames: u32,
    pub maximum_correction_m: f32,
}

pub fn apply_upright_torso_limit(
    track: &mut SomaMotionTrack,
    maximum_lean_degrees: f32,
) -> Result<TorsoUprightCorrectionReport, String> {
    track.validate()?;
    let hips = track.joint_index("Hips")?;
    let chest = track.joint_index("Chest")?;
    let upper_names = [
        "Spine2",
        "Chest",
        "Neck",
        "Neck1",
        "Head",
        "LeftShoulder",
        "LeftArm",
        "LeftForeArm",
        "LeftHand",
        "RightShoulder",
        "RightArm",
        "RightForeArm",
        "RightHand",
    ];
    let upper = upper_names
        .iter()
        .filter_map(|name| {
            track
                .joint_names
                .iter()
                .position(|candidate| candidate == name)
        })
        .collect::<Vec<_>>();
    let tangent = maximum_lean_degrees.to_radians().tan();
    let mut corrected_frames = 0;
    let mut maximum_correction_m = 0.0_f32;
    for frame in &mut track.frames {
        let root = frame.joints[hips];
        let torso = frame.joints[chest];
        let signed_vertical = torso[1] - root[1];
        let vertical = signed_vertical.max(0.28);
        let horizontal = [torso[0] - root[0], 0.0, torso[2] - root[2]];
        let horizontal_length = length(horizontal);
        let maximum_horizontal = vertical * tangent;
        if horizontal_length > maximum_horizontal || signed_vertical < 0.28 {
            let desired = if horizontal_length > maximum_horizontal {
                scale(horizontal, maximum_horizontal / horizontal_length)
            } else {
                horizontal
            };
            let mut correction = sub(desired, horizontal);
            correction[1] = vertical - signed_vertical;
            for joint in &upper {
                frame.joints[*joint] = add(frame.joints[*joint], correction);
            }
            corrected_frames += 1;
            maximum_correction_m = maximum_correction_m.max(length(correction));
        }
    }
    Ok(TorsoUprightCorrectionReport {
        corrected_frames,
        maximum_correction_m,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LimbClearanceCorrectionReport {
    pub id: String,
    pub corrected_joint_samples: u32,
    pub maximum_correction_m: f32,
    pub accepted: bool,
}

pub fn apply_limb_clearance_guard(
    track: &mut SomaMotionTrack,
    id: &str,
    joint_names: &[&str],
    primary_axis: usize,
    maximum_value: f32,
    secondary_axis: usize,
    secondary_range: [f32; 2],
    maximum_correction: f32,
) -> Result<LimbClearanceCorrectionReport, String> {
    track.validate()?;
    if primary_axis > 2 || secondary_axis > 2 || primary_axis == secondary_axis {
        return Err("limb clearance axes must be distinct XYZ indices".into());
    }
    let joints = joint_names
        .iter()
        .map(|name| track.joint_index(name))
        .collect::<Result<Vec<_>, _>>()?;
    let mut corrected_joint_samples = 0;
    let mut maximum_correction_m = 0.0_f32;
    for frame in &mut track.frames {
        for joint in &joints {
            let point = &mut frame.joints[*joint];
            if (secondary_range[0]..=secondary_range[1]).contains(&point[secondary_axis])
                && point[primary_axis] > maximum_value
            {
                let correction = point[primary_axis] - maximum_value;
                point[primary_axis] = maximum_value;
                maximum_correction_m = maximum_correction_m.max(correction);
                corrected_joint_samples += 1;
            }
        }
    }
    Ok(LimbClearanceCorrectionReport {
        id: id.into(),
        corrected_joint_samples,
        maximum_correction_m,
        accepted: maximum_correction_m <= maximum_correction,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RootPathCorrectionReport {
    pub corrected_frames: u32,
    pub maximum_correction_m: f32,
    pub mean_correction_m: f32,
}

pub fn apply_root_path_correction(
    track: &mut SomaMotionTrack,
    target_path: &[Point3],
) -> Result<RootPathCorrectionReport, String> {
    track.validate()?;
    if target_path.is_empty() {
        return Err("root correction requires a non-empty target path".into());
    }
    let hips = track.joint_index("Hips")?;
    let mut corrected_frames = 0;
    let mut maximum_correction_m = 0.0_f32;
    let mut correction_sum = 0.0_f32;
    let frame_count = track.frames.len().max(1);
    for (frame_index, frame) in track.frames.iter_mut().enumerate() {
        let target_index = if frame_count <= 1 {
            0
        } else {
            ((frame_index as f32 / (frame_count - 1) as f32) * (target_path.len() - 1) as f32)
                .round() as usize
        };
        let target = target_path[target_index.min(target_path.len() - 1)];
        let root = frame.joints[hips];
        let correction = [target[0] - root[0], 0.0, target[2] - root[2]];
        let magnitude = length(correction);
        if magnitude > 1e-5 {
            for joint in &mut frame.joints {
                *joint = add(*joint, correction);
            }
            corrected_frames += 1;
        }
        maximum_correction_m = maximum_correction_m.max(magnitude);
        correction_sum += magnitude;
    }
    let mut authored_heading = normalize(track.frames[0].root_heading);
    if length(authored_heading) < 0.5 {
        authored_heading = [0.0, 0.0, -1.0];
    }
    for frame_index in 0..track.frames.len() {
        let before = frame_index.saturating_sub(1);
        let after = (frame_index + 1).min(track.frames.len() - 1);
        let delta = sub(
            track.frames[after].joints[hips],
            track.frames[before].joints[hips],
        );
        let horizontal = [delta[0], 0.0, delta[2]];
        if length(horizontal) > 1e-4 {
            authored_heading = normalize(horizontal);
        }
        track.frames[frame_index].root_heading = authored_heading;
    }
    Ok(RootPathCorrectionReport {
        corrected_frames,
        maximum_correction_m,
        mean_correction_m: correction_sum / frame_count as f32,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PoseRepairReport {
    pub corrected_joint_samples: u32,
    pub maximum_before_m: f32,
    pub maximum_after_m: f32,
}

pub fn repair_pose_discontinuities(
    track: &mut SomaMotionTrack,
    maximum_step_m: f32,
) -> Result<PoseRepairReport, String> {
    track.validate()?;
    if maximum_step_m <= 0.0 {
        return Err("pose discontinuity limit must be positive".into());
    }
    let mut corrected_joint_samples = 0;
    let mut maximum_before_m = 0.0_f32;
    let mut maximum_after_m = 0.0_f32;
    for frame_index in 1..track.frames.len() {
        let (past, present) = track.frames.split_at_mut(frame_index);
        let previous = &past[frame_index - 1].joints;
        for (current, prior) in present[0].joints.iter_mut().zip(previous) {
            let step = distance(*current, *prior);
            maximum_before_m = maximum_before_m.max(step);
            if step > maximum_step_m {
                *current = add(*prior, scale(sub(*current, *prior), maximum_step_m / step));
                corrected_joint_samples += 1;
            }
            maximum_after_m = maximum_after_m.max(distance(*current, *prior));
        }
    }
    Ok(PoseRepairReport {
        corrected_joint_samples,
        maximum_before_m,
        maximum_after_m,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FootLockReport {
    pub corrected_frames: u32,
    pub inferred_contact_frames: u32,
    pub maximum_correction_m: f32,
}

pub fn apply_contact_foot_locks(track: &mut SomaMotionTrack) -> Result<FootLockReport, String> {
    track.validate()?;
    let left_foot = track.joint_index("LeftFoot")?;
    let right_foot = track.joint_index("RightFoot")?;
    let left_leg = track.joint_index("LeftLeg")?;
    let left_shin = track.joint_index("LeftShin")?;
    let right_leg = track.joint_index("RightLeg")?;
    let right_shin = track.joint_index("RightShin")?;
    let left_toe = track.joint_indices_optional("LeftToeBase");
    let right_toe = track.joint_indices_optional("RightToeBase");
    let floor = track
        .frames
        .iter()
        .flat_map(|frame| [frame.joints[left_foot][1], frame.joints[right_foot][1]])
        .fold(f32::INFINITY, f32::min);
    let fps = track.fps.max(1) as f32;
    let nominal_lengths = [
        (
            distance(
                track.frames[0].joints[left_leg],
                track.frames[0].joints[left_shin],
            ),
            distance(
                track.frames[0].joints[left_shin],
                track.frames[0].joints[left_foot],
            ),
        ),
        (
            distance(
                track.frames[0].joints[right_leg],
                track.frames[0].joints[right_shin],
            ),
            distance(
                track.frames[0].joints[right_shin],
                track.frames[0].joints[right_foot],
            ),
        ),
    ];
    let hips = track.joint_index("Hips")?;
    let mut anchors: [Option<Point3>; 2] = [None, None];
    let mut corrected_frames = 0;
    let mut inferred_contact_frames = 0;
    let mut maximum_correction_m = 0.0_f32;
    let mut stationary_support_side: Option<usize> = None;
    let mut moving_support_side: Option<usize> = None;
    for frame_index in 0..track.frames.len() {
        let root_speed = if frame_index == 0 {
            0.0
        } else {
            let current = track.frames[frame_index].joints[hips];
            let previous = track.frames[frame_index - 1].joints[hips];
            length([current[0] - previous[0], 0.0, current[2] - previous[2]]) * fps
        };
        if root_speed < 0.15 {
            moving_support_side = None;
            let side = *stationary_support_side.get_or_insert_with(|| {
                if track.frames[frame_index].joints[left_foot][1]
                    <= track.frames[frame_index].joints[right_foot][1]
                {
                    0
                } else {
                    1
                }
            });
            let (foot, toe) = if side == 0 {
                (left_foot, left_toe)
            } else {
                (right_foot, right_toe)
            };
            let correction = floor + 0.04 - track.frames[frame_index].joints[foot][1];
            track.frames[frame_index].joints[foot][1] += correction;
            if let Some(toe) = toe {
                track.frames[frame_index].joints[toe][1] += correction;
            }
            track.frames[frame_index].foot_contacts[side] = true;
            track.frames[frame_index].foot_contacts[1 - side] = false;
            inferred_contact_frames += 1;
            if correction.abs() > 1e-5 {
                corrected_frames += 1;
                maximum_correction_m = maximum_correction_m.max(correction.abs());
            }
        } else {
            stationary_support_side = None;
            let side = ((track.frames[frame_index].time / 0.55).floor() as usize) % 2;
            if moving_support_side != Some(side) {
                anchors = [None, None];
                moving_support_side = Some(side);
            }
            let (foot, toe) = if side == 0 {
                (left_foot, left_toe)
            } else {
                (right_foot, right_toe)
            };
            let correction = floor + 0.04 - track.frames[frame_index].joints[foot][1];
            track.frames[frame_index].joints[foot][1] += correction;
            if let Some(toe) = toe {
                track.frames[frame_index].joints[toe][1] += correction;
            }
            track.frames[frame_index].foot_contacts[side] = true;
            track.frames[frame_index].foot_contacts[1 - side] = false;
            inferred_contact_frames += 1;
            if correction.abs() > 1e-5 {
                corrected_frames += 1;
                maximum_correction_m = maximum_correction_m.max(correction.abs());
            }
        }
        for (side, (foot, toe)) in [(left_foot, left_toe), (right_foot, right_toe)]
            .into_iter()
            .enumerate()
        {
            let current = track.frames[frame_index].joints[foot];
            let speed = if frame_index == 0 {
                0.0
            } else {
                let previous = track.frames[frame_index - 1].joints[foot];
                length([current[0] - previous[0], 0.0, current[2] - previous[2]]) * fps
            };
            let inferred = current[1] <= floor + 0.11 && speed < 0.9;
            if inferred && !track.frames[frame_index].foot_contacts[side] {
                track.frames[frame_index].foot_contacts[side] = true;
                inferred_contact_frames += 1;
            }
            if track.frames[frame_index].foot_contacts[side] {
                let anchor = anchors[side].get_or_insert(current);
                let correction = [anchor[0] - current[0], 0.0, anchor[2] - current[2]];
                let magnitude = length(correction);
                if magnitude <= 0.14 || root_speed < 0.15 || moving_support_side == Some(side) {
                    track.frames[frame_index].joints[foot] = add(current, correction);
                    if let Some(toe) = toe {
                        track.frames[frame_index].joints[toe] =
                            add(track.frames[frame_index].joints[toe], correction);
                    }
                    maximum_correction_m = maximum_correction_m.max(magnitude);
                    if magnitude > 1e-5 {
                        corrected_frames += 1;
                    }
                } else {
                    anchors[side] = Some(current);
                }
            } else {
                anchors[side] = None;
            }
        }
        for (side, (leg, shin, foot, toe)) in [
            (left_leg, left_shin, left_foot, left_toe),
            (right_leg, right_shin, right_foot, right_toe),
        ]
        .into_iter()
        .enumerate()
        {
            if !track.frames[frame_index].foot_contacts[side] {
                continue;
            }
            let hip = track.frames[frame_index].joints[leg];
            let original_knee = track.frames[frame_index].joints[shin];
            let requested_foot = track.frames[frame_index].joints[foot];
            let (upper, lower) = nominal_lengths[side];
            let delta = sub(requested_foot, hip);
            let requested_distance = length(delta).max(1e-5);
            let direction = scale(delta, 1.0 / requested_distance);
            let reach =
                requested_distance.clamp((upper - lower).abs() + 0.005, upper + lower - 0.005);
            let solved_foot = add(hip, scale(direction, reach));
            let bend = sub(original_knee, hip);
            let projected = scale(direction, dot(bend, direction));
            let mut perpendicular = sub(bend, projected);
            if length(perpendicular) < 1e-5 {
                perpendicular = cross(direction, [0.0, 0.0, 1.0]);
                if length(perpendicular) < 1e-5 {
                    perpendicular = cross(direction, [1.0, 0.0, 0.0]);
                }
            }
            perpendicular = normalize(perpendicular);
            let along = (upper * upper - lower * lower + reach * reach) / (2.0 * reach);
            let outward = (upper * upper - along * along).max(0.0).sqrt();
            let solved_knee = add(
                add(hip, scale(direction, along)),
                scale(perpendicular, outward),
            );
            let foot_delta = sub(solved_foot, requested_foot);
            track.frames[frame_index].joints[shin] = solved_knee;
            track.frames[frame_index].joints[foot] = solved_foot;
            if let Some(toe) = toe {
                track.frames[frame_index].joints[toe] =
                    add(track.frames[frame_index].joints[toe], foot_delta);
            }
            maximum_correction_m = maximum_correction_m
                .max(distance(original_knee, solved_knee))
                .max(length(foot_delta));
        }
    }
    Ok(FootLockReport {
        corrected_frames,
        inferred_contact_frames,
        maximum_correction_m,
    })
}

impl SomaMotionTrack {
    fn joint_indices_optional(&self, name: &str) -> Option<usize> {
        self.joint_names
            .iter()
            .position(|candidate| candidate == name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MotionSanityReport {
    pub valid: bool,
    pub sustained_crouch_seconds: f32,
    pub maximum_torso_lean_deg: f32,
    pub maximum_vertical_root_jump_m: f32,
    pub maximum_airborne_seconds: f32,
    pub maximum_speed_mps: f32,
    pub mean_contact_foot_slide_mps: f32,
    pub maximum_pose_discontinuity_m: f32,
    pub stable_final_stance: bool,
    pub rejection_reasons: Vec<String>,
}

pub fn evaluate_motion_sanity(track: &SomaMotionTrack) -> MotionSanityReport {
    let fps = track.fps.max(1) as f32;
    let idx = |name: &str| {
        track
            .joint_names
            .iter()
            .position(|candidate| candidate == name)
    };
    let hips = idx("Hips");
    let chest = idx("Chest");
    let left_foot = idx("LeftFoot");
    let right_foot = idx("RightFoot");
    let floor = track
        .frames
        .iter()
        .flat_map(|frame| {
            [
                left_foot.map(|joint| frame.joints[joint][1]),
                right_foot.map(|joint| frame.joints[joint][1]),
            ]
        })
        .flatten()
        .fold(f32::INFINITY, f32::min);
    let mut crouch_run = 0_u32;
    let mut max_crouch_run = 0_u32;
    let mut airborne_run = 0_u32;
    let mut max_airborne_run = 0_u32;
    let mut maximum_torso_lean_deg = 0.0_f32;
    let mut maximum_vertical_root_jump_m = 0.0_f32;
    let mut maximum_speed_mps = 0.0_f32;
    let mut maximum_pose_discontinuity_m = 0.0_f32;
    let mut foot_slide_sum = 0.0_f32;
    let mut foot_slide_count = 0_u32;

    for (frame_index, frame) in track.frames.iter().enumerate() {
        if hips.is_some_and(|joint| frame.joints[joint][1] < 0.72) {
            crouch_run += 1;
            max_crouch_run = max_crouch_run.max(crouch_run);
        } else {
            crouch_run = 0;
        }
        let feet_above_floor = left_foot.zip(right_foot).is_some_and(|(left, right)| {
            frame.joints[left][1] > floor + 0.12 && frame.joints[right][1] > floor + 0.12
        });
        if !frame.foot_contacts[0] && !frame.foot_contacts[1] && feet_above_floor {
            airborne_run += 1;
            max_airborne_run = max_airborne_run.max(airborne_run);
        } else {
            airborne_run = 0;
        }
        if let (Some(hips), Some(chest)) = (hips, chest) {
            let axis = normalize(sub(frame.joints[chest], frame.joints[hips]));
            let lean = dot(axis, [0.0, 1.0, 0.0])
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees();
            maximum_torso_lean_deg = maximum_torso_lean_deg.max(lean);
        }
        if frame_index == 0 {
            continue;
        }
        let previous = &track.frames[frame_index - 1];
        if let Some(hips) = hips {
            maximum_vertical_root_jump_m = maximum_vertical_root_jump_m
                .max((frame.joints[hips][1] - previous.joints[hips][1]).abs());
            let xz = [
                frame.joints[hips][0] - previous.joints[hips][0],
                0.0,
                frame.joints[hips][2] - previous.joints[hips][2],
            ];
            maximum_speed_mps = maximum_speed_mps.max(length(xz) * fps);
        }
        for (current, prior) in frame.joints.iter().zip(&previous.joints) {
            maximum_pose_discontinuity_m =
                maximum_pose_discontinuity_m.max(distance(*current, *prior));
        }
        for (contact, foot) in [frame.foot_contacts[0], frame.foot_contacts[1]]
            .into_iter()
            .zip([left_foot, right_foot])
        {
            if contact {
                if let Some(foot) = foot {
                    foot_slide_sum += distance(frame.joints[foot], previous.joints[foot]) * fps;
                    foot_slide_count += 1;
                }
            }
        }
    }
    let final_window = (track.fps as usize / 2).max(2).min(track.frames.len());
    let stable_final_stance = if let Some(hips) = hips {
        let start = track.frames.len() - final_window;
        let displacement = distance(
            track.frames[start].joints[hips],
            track.frames.last().unwrap().joints[hips],
        );
        displacement < 0.16
            && track
                .frames
                .last()
                .is_some_and(|frame| frame.foot_contacts.iter().any(|value| *value))
    } else {
        false
    };
    let sustained_crouch_seconds = max_crouch_run as f32 / fps;
    let maximum_airborne_seconds = max_airborne_run as f32 / fps;
    let mean_contact_foot_slide_mps = if foot_slide_count == 0 {
        0.0
    } else {
        foot_slide_sum / foot_slide_count as f32
    };
    let mut rejection_reasons = Vec::new();
    if sustained_crouch_seconds > 0.8 {
        rejection_reasons.push("sustained_crouch".into());
    }
    if maximum_torso_lean_deg > 55.0 {
        rejection_reasons.push("excessive_torso_lean".into());
    }
    if maximum_vertical_root_jump_m > 0.28 {
        rejection_reasons.push("vertical_root_jump".into());
    }
    if maximum_airborne_seconds > 0.45 {
        rejection_reasons.push("extended_airborne_period".into());
    }
    if maximum_speed_mps > 3.8 {
        rejection_reasons.push("implausible_root_speed".into());
    }
    if mean_contact_foot_slide_mps > 0.75 {
        rejection_reasons.push("contact_foot_slide".into());
    }
    if maximum_pose_discontinuity_m > 0.55 {
        rejection_reasons.push("pose_discontinuity".into());
    }
    if !stable_final_stance {
        rejection_reasons.push("unstable_final_stance".into());
    }
    MotionSanityReport {
        valid: rejection_reasons.is_empty(),
        sustained_crouch_seconds,
        maximum_torso_lean_deg,
        maximum_vertical_root_jump_m,
        maximum_airborne_seconds,
        maximum_speed_mps,
        mean_contact_foot_slide_mps,
        maximum_pose_discontinuity_m,
        stable_final_stance,
        rejection_reasons,
    }
}

fn value_to_point(value: &Value) -> Option<Point3> {
    let values = value.as_array()?;
    if values.len() != 3 {
        return None;
    }
    Some([
        values[0].as_f64()? as f32,
        values[1].as_f64()? as f32,
        values[2].as_f64()? as f32,
    ])
}

fn segment_aabb_penetration(
    start: Point3,
    end: Point3,
    radius: f32,
    collider: &EnvironmentCollider,
) -> f32 {
    let mut maximum = 0.0_f32;
    for sample in 0..=8 {
        let point = lerp(start, end, sample as f32 / 8.0);
        let min = sub(collider.center, collider.half_extents);
        let max = add(collider.center, collider.half_extents);
        let closest = [
            point[0].clamp(min[0], max[0]),
            point[1].clamp(min[1], max[1]),
            point[2].clamp(min[2], max[2]),
        ];
        let outside_distance = distance(point, closest);
        let penetration = if outside_distance > 1e-6 {
            (radius - outside_distance).max(0.0)
        } else {
            let face_distance = [
                point[0] - min[0],
                max[0] - point[0],
                point[1] - min[1],
                max[1] - point[1],
                point[2] - min[2],
                max[2] - point[2],
            ]
            .into_iter()
            .fold(f32::INFINITY, f32::min);
            radius + face_distance.max(0.0)
        };
        maximum = maximum.max(penetration);
    }
    maximum
}

fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if (b - a).abs() < 1e-6 {
        1.0
    } else {
        ((value - a) / (b - a)).clamp(0.0, 1.0)
    }
}
fn smoothstep(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}
fn add(a: Point3, b: Point3) -> Point3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: Point3, b: Point3) -> Point3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: Point3, scalar: f32) -> Point3 {
    [a[0] * scalar, a[1] * scalar, a[2] * scalar]
}
fn dot(a: Point3, b: Point3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Point3, b: Point3) -> Point3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn length(a: Point3) -> f32 {
    dot(a, a).sqrt()
}
fn normalize(a: Point3) -> Point3 {
    let length = length(a);
    if length < 1e-6 {
        [0.0, 0.0, 0.0]
    } else {
        scale(a, 1.0 / length)
    }
}
fn distance(a: Point3, b: Point3) -> f32 {
    length(sub(a, b))
}
fn lerp(a: Point3, b: Point3, t: f32) -> Point3 {
    add(a, scale(sub(b, a), t.clamp(0.0, 1.0)))
}
