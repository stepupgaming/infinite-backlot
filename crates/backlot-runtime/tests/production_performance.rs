use backlot_runtime::production_performance::{
    apply_two_bone_contact_correction, evaluate_body_collisions, evaluate_motion_sanity,
    ContactCorrectionRequest, SomaFrame, SomaMotionTrack,
};
use serde_json::json;

fn track(frames: Vec<Vec<[f32; 3]>>) -> SomaMotionTrack {
    SomaMotionTrack {
        schema_version: 1,
        fps: 30,
        joint_names: vec![
            "Hips".into(),
            "Chest".into(),
            "Head".into(),
            "RightShoulder".into(),
            "RightArm".into(),
            "RightForeArm".into(),
            "RightHand".into(),
            "LeftLeg".into(),
            "LeftShin".into(),
            "LeftFoot".into(),
            "RightLeg".into(),
            "RightShin".into(),
            "RightFoot".into(),
        ],
        frames: frames
            .into_iter()
            .enumerate()
            .map(|(index, joints)| SomaFrame {
                time: index as f32 / 30.0,
                joints,
                root_heading: [0.0, 0.0, 1.0],
                foot_contacts: [true, true],
            })
            .collect(),
        source_segments: vec!["test".into()],
    }
}

fn standing_pose() -> Vec<[f32; 3]> {
    vec![
        [0.0, 0.95, 0.0],
        [0.0, 1.45, 0.0],
        [0.0, 1.72, 0.0],
        [0.18, 1.46, 0.0],
        [0.38, 1.40, 0.0],
        [0.62, 1.25, 0.0],
        [0.82, 1.12, 0.0],
        [-0.14, 0.90, 0.0],
        [-0.14, 0.48, 0.0],
        [-0.14, 0.04, 0.08],
        [0.14, 0.90, 0.0],
        [0.14, 0.48, 0.0],
        [0.14, 0.04, 0.08],
    ]
}

#[test]
fn two_bone_contact_correction_locks_the_hand_within_three_centimeters() {
    let mut motion = track(vec![standing_pose(); 30]);
    let result = apply_two_bone_contact_correction(
        &mut motion,
        &ContactCorrectionRequest {
            id: "door_handle".into(),
            shoulder_joint: "RightArm".into(),
            elbow_joint: "RightForeArm".into(),
            hand_joint: "RightHand".into(),
            target: [0.72, 1.18, 0.24],
            contact_start: 0.40,
            contact_end: 0.60,
            blend_seconds: 0.20,
            maximum_correction: 0.35,
        },
    )
    .unwrap();
    assert!(result.after_error_m <= 0.03, "{result:?}");
    assert!(result.maximum_applied_correction_m <= 0.35);
}

#[test]
fn body_collision_reports_the_colliding_limb_and_environment_object() {
    let motion = track(vec![standing_pose()]);
    let colliders = json!([
        {
            "id":"SHELF",
            "shape":"box",
            "center":[0.72,1.22,0.0],
            "half_extents":[0.20,0.30,0.20],
            "role":"static"
        }
    ]);
    let report = evaluate_body_collisions(&motion, colliders.as_array().unwrap(), &[]).unwrap();
    assert!(report.limb_collision_samples > 0);
    assert!(report
        .colliding_body_parts
        .iter()
        .any(|part| part.contains("RightForeArm") || part.contains("RightHand")));
    assert_eq!(report.colliding_environment_objects, vec!["SHELF"]);
    assert!(report.maximum_penetration_m > 0.0);
}

#[test]
fn motion_sanity_rejects_sustained_crouching_and_vertical_hops() {
    let mut frames = Vec::new();
    for index in 0..75 {
        let mut pose = standing_pose();
        for joint in &mut pose {
            joint[1] -= 0.38;
        }
        if index == 20 {
            for joint in &mut pose {
                joint[1] += 0.7;
            }
        }
        frames.push(pose);
    }
    let report = evaluate_motion_sanity(&track(frames));
    assert!(!report.valid);
    assert!(report.sustained_crouch_seconds > 0.8);
    assert!(report.maximum_vertical_root_jump_m > 0.5);
    assert!(report
        .rejection_reasons
        .contains(&"sustained_crouch".into()));
    assert!(report
        .rejection_reasons
        .contains(&"vertical_root_jump".into()));
}
