use backlot_runtime::motion_authoring::{
    select_best_candidate, CandidateMetrics, ContactEvent, EndEffectorConstraint,
    KimodoMotionBackend, MotionAuthoringRequest, MotionCandidate, PromptSegment, RootPathSample,
};

fn request() -> MotionAuthoringRequest {
    MotionAuthoringRequest {
        schema_version: 1,
        request_id: "MARA_NAV_PANEL_001".into(),
        performer: "mara_soma".into(),
        duration: 5.8,
        prompt_sequence: vec![
            PromptSegment {
                start: 0.0,
                end: 2.8,
                text: "walks briskly while annoyed".into(),
            },
            PromptSegment {
                start: 2.8,
                end: 4.5,
                text: "slows and turns toward the panel".into(),
            },
            PromptSegment {
                start: 4.5,
                end: 5.8,
                text: "presses the panel with the right hand".into(),
            },
        ],
        root_waypoints: vec![],
        dense_root_path: vec![
            RootPathSample {
                time: 0.0,
                position: [0.0, 0.0, 0.0],
                heading: [0.0, 0.0, 1.0],
            },
            RootPathSample {
                time: 5.8,
                position: [2.0, 0.0, 3.0],
                heading: [1.0, 0.0, 0.0],
            },
        ],
        arrival_heading: [1.0, 0.0, 0.0],
        full_body_keyframes: vec![],
        joint_constraints: vec![],
        end_effector_constraints: vec![EndEffectorConstraint {
            id: "RIGHT_HAND_PANEL_CONTACT".into(),
            time: 5.0,
            joint: "RightHand".into(),
            position: [2.35, 1.15, 3.0],
            rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            position_weight: 1.0,
            rotation_weight: 1.0,
            strict: true,
            reference_motion: "official_soma_ee".into(),
            reference_frame: 94,
        }],
        environment_constraints: vec![],
        contact_events: vec![ContactEvent {
            id: "PANEL_PRESS_CONTACT".into(),
            start: 4.9,
            end: 5.15,
            performer_joint: "RightHand".into(),
            target_id: "INTERACT_ELEVATOR_PANEL".into(),
            state_transition: Some("panel.pressed=true".into()),
        }],
        candidate_count: 3,
        seed: 424242,
        strictness: 0.85,
        continuation_pose: None,
        output_stem: "output/test".into(),
    }
}

#[test]
fn rich_request_round_trips_and_validates() {
    let request = request();
    request.validate().expect("valid rich request");
    let encoded = serde_json::to_string_pretty(&request).unwrap();
    let decoded: MotionAuthoringRequest = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.prompt_sequence.len(), 3);
    assert_eq!(decoded.end_effector_constraints[0].joint, "RightHand");
    assert_eq!(decoded.contact_events.len(), 1);
    assert_eq!(decoded.candidate_seeds(), vec![424242, 424243, 424244]);
}

#[test]
fn rejects_prompt_gap_and_unbounded_candidate_count() {
    let mut request = request();
    request.prompt_sequence[1].start = 3.2;
    assert!(request
        .validate()
        .unwrap_err()
        .contains("prompt sequence gap"));
    request.prompt_sequence[1].start = 2.8;
    request.candidate_count = 9;
    assert!(request.validate().unwrap_err().contains("candidate_count"));
}

#[test]
fn selects_lowest_structural_error_not_first_candidate() {
    let candidates = vec![
        MotionCandidate::scored(
            424242,
            CandidateMetrics {
                root_path_deviation: 0.4,
                hand_target_error: 0.2,
                hand_orientation_error_deg: 10.0,
                foot_slide: 0.08,
                floor_penetration: 0.0,
                body_obstacle_intersections: 0,
                duration_error: 0.0,
                arrival_heading_error_deg: 5.0,
                contact_timing_error: 0.1,
                joint_limit_violations: 0,
            },
        ),
        MotionCandidate::scored(
            424243,
            CandidateMetrics {
                root_path_deviation: 0.05,
                hand_target_error: 0.03,
                hand_orientation_error_deg: 2.0,
                foot_slide: 0.02,
                floor_penetration: 0.0,
                body_obstacle_intersections: 0,
                duration_error: 0.0,
                arrival_heading_error_deg: 1.0,
                contact_timing_error: 0.02,
                joint_limit_violations: 0,
            },
        ),
    ];
    let selected = select_best_candidate(&candidates).expect("valid candidate selected");
    assert_eq!(selected.seed, 424243);
    assert!(selected.evaluation.valid);
}

#[test]
fn generated_navigation_request_uses_the_complete_runtime_contract() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("output/navigation-kimodo-proof/kimodo_request.json");
    let text = std::fs::read_to_string(path).expect("generated navigation request exists");
    let request: MotionAuthoringRequest =
        serde_json::from_str(&text).expect("request matches Rust contract");
    request
        .validate()
        .expect("generated request passes runtime validation");
    assert_eq!(request.prompt_sequence.len(), 5);
    assert!(request.dense_root_path.len() >= 500);
    assert!(request.root_waypoints.len() >= 10);
    assert_eq!(request.full_body_keyframes.len(), 1);
    assert_eq!(request.end_effector_constraints.len(), 3);
    assert_eq!(request.environment_constraints.len(), 3);
    assert_eq!(request.contact_events.len(), 2);
    assert_eq!(request.candidate_count, 2);
}

#[test]
fn rich_kimodo_backend_uses_the_one_load_batch_worker() {
    let backend = KimodoMotionBackend {
        python: "C:/runtime/python.exe".into(),
        runtime_directory: "C:/runtime/kimodo".into(),
        script: "C:/runtime/kimodo/backlot_batch_kimodo.py".into(),
        checkpoint: "F:/Models/Kimodo/Kimodo-SOMA-RP-v1.1".into(),
        diffusion_steps: 12,
    };
    let command = backend.command_preview(
        std::path::Path::new("request.json"),
        std::path::Path::new("response.json"),
    );
    assert!(command
        .iter()
        .any(|part| part.ends_with("backlot_batch_kimodo.py")));
    assert_eq!(command.last().map(String::as_str), Some("12"));
    assert!(command.contains(&"--checkpoint".to_string()));
}
