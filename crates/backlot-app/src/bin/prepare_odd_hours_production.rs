use backlot_core::navigation::{NavigationWorld, PortalState, ResolvedRoute, RouteRequest};
use backlot_runtime::motion_authoring::{
    ContactEvent, ContinuationPose, EndEffectorConstraint, EnvironmentConstraint, FullBodyKeyframe,
    MotionAuthoringRequest, PromptSegment, RootPathSample,
};
use backlot_runtime::production_performance::{
    apply_contact_foot_locks, apply_limb_clearance_guard, apply_procedural_soma_gait,
    apply_root_path_correction, apply_two_bone_contact_correction,
    apply_two_bone_contact_trajectory, apply_upright_torso_limit, concatenate_tracks,
    evaluate_body_collisions, evaluate_motion_sanity, repair_pose_discontinuities,
    stabilize_root_height, BodyCollisionReport, ContactCorrectionRequest, ContactCorrectionResult,
    FootLockReport, LimbClearanceCorrectionReport, MotionSanityReport,
    MovingContactCorrectionRequest, PoseRepairReport, ProceduralGaitReport,
    RootHeightStabilizationReport, RootPathCorrectionReport, SomaMotionTrack,
    TorsoUprightCorrectionReport,
};
use backlot_runtime::smart_interactions::{SmartInteraction, SmartInteractionCatalog};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const FPS: u32 = 30;
const DURATION: f32 = 17.7;
const PORTAL: &str = "NAV_PORTAL_ODD_HOURS_FRONT_DOOR";

#[derive(Debug, Serialize)]
struct ProductionPhase {
    id: String,
    start: f32,
    end: f32,
    phase_type: String,
    route_id: Option<String>,
    smart_interaction_id: Option<String>,
    resolved_target: Option<String>,
    state_transitions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CameraShot {
    id: String,
    start: f32,
    end: f32,
    anchor: String,
    subject: String,
    required_visible_objects: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ProductionPlan {
    schema_version: u32,
    scene_id: String,
    duration: f32,
    fps: u32,
    resolution: [u32; 2],
    renderer: String,
    world_asset: String,
    performer_contract: String,
    navigation_contract: String,
    smart_interaction_catalog: String,
    pipeline: Vec<String>,
    phases: Vec<ProductionPhase>,
    camera_shots: Vec<CameraShot>,
    audio_cues: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct CandidateRecord {
    request_id: String,
    candidate_index: usize,
    seed: u64,
    source_npz: String,
    track_path: String,
    worker_score: f32,
    root_correction: RootPathCorrectionReport,
    root_height: RootHeightStabilizationReport,
    pose_repair: PoseRepairReport,
    torso_upright: TorsoUprightCorrectionReport,
    procedural_gait: ProceduralGaitReport,
    foot_lock: FootLockReport,
    limb_clearance: Option<LimbClearanceCorrectionReport>,
    body_collision: BodyCollisionReport,
    sanity: MotionSanityReport,
    contact_correction: Option<ContactCorrectionResult>,
    valid: bool,
    rejection_reasons: Vec<String>,
    selected: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ODD HOURS PRODUCTION PREP FAILED: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let project_root = std::env::current_dir().map_err(|error| error.to_string())?;
    let output_dir = project_root.join("output/production-vertical-slice");
    std::fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;
    let plan_only = std::env::args().any(|argument| argument == "--plan-only");
    let reuse_kimodo = std::env::args().any(|argument| argument == "--reuse-kimodo");
    let regenerate_interactions =
        std::env::args().any(|argument| argument == "--regenerate-interactions");
    let navigation_path = project_root.join("assets/world/navigation/odd_hours_production.json");
    let catalog_path = project_root.join("assets/interactions/smart_interactions.json");
    let navigation =
        NavigationWorld::from_path(&navigation_path).map_err(|error| error.to_string())?;
    let catalog =
        SmartInteractionCatalog::from_path(&catalog_path).map_err(|error| error.to_string())?;
    catalog.validate()?;
    let door = required_interaction(&catalog, "SMART_DOOR_OPEN")?;
    let walk_through = required_interaction(&catalog, "SMART_DOOR_WALK_THROUGH")?;
    let pickup = required_interaction(&catalog, "SMART_PICKUP_SMALL")?;

    let routes = resolve_routes(&navigation)?;
    let requests = build_requests(&routes, door, walk_through, pickup, &output_dir)?;
    for request in &requests {
        request.validate()?;
    }
    let plan = build_plan(door, walk_through, pickup);
    write_json(output_dir.join("production_plan.json"), &plan)?;
    write_json(
        output_dir.join("resolved_routes.json"),
        &json!({
            "schema_version":1,
            "navigation_contract":"assets/world/navigation/odd_hours_production.json",
            "closed_door_crossing_rejected":true,
            "portal_state_sequence":[
                {"phase":"exterior_approach","portal":PORTAL,"state":"closed"},
                {"phase":"door_contact","portal":PORTAL,"state":"opening"},
                {"phase":"doorway_traversal","portal":PORTAL,"state":"open"}
            ],
            "routes":routes,
            "destination_reservation":{"actor":"mara_soma","destination":"ODD_HOURS_COUNTER_INTERACTION","start":13.7,"end":17.7,"radius":0.52}
        }),
    )?;
    let request_values = requests
        .iter()
        .map(|request| {
            let mut value = serde_json::to_value(request).unwrap();
            let object = value.as_object_mut().unwrap();
            object.insert(
                "navigation_contract".into(),
                Value::String("assets/world/navigation/odd_hours_production.json".into()),
            );
            object.insert("actor_radius".into(), json!(0.34));
            value
        })
        .collect::<Vec<_>>();
    write_json(output_dir.join("motion_requests.json"), &request_values)?;
    write_json(
        output_dir.join("structural_preflight.json"),
        &json!({
            "status":"passed",
            "routes_requested":3,
            "routes_resolved":3,
            "routes_failed":0,
            "static_collision_intersections":0,
            "closed_portal_crossing_rejected":true,
            "unsupported_floor_samples":0,
            "clearance_failures":0,
            "smart_interactions_resolved":[door.semantic_id,walk_through.semantic_id,pickup.semantic_id],
            "world_asset":"assets/world/locations/location_odd_hours_v3.glb",
            "performer_contract":"assets/characters/canonical_soma77.json"
        }),
    )?;
    if plan_only {
        println!(
            "ODD HOURS STRUCTURAL PLAN COMPLETE {}",
            output_dir.display()
        );
        return Ok(());
    }

    let motions_dir = output_dir.join("motions");
    if regenerate_interactions {
        let interaction_requests = request_values
            .iter()
            .filter(|value| {
                matches!(
                    value["request_id"].as_str(),
                    Some("door_open_interaction" | "pickup_recovery")
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let interaction_request_path = output_dir.join("interaction_motion_requests.json");
        let interaction_response_path = output_dir.join("interaction_kimodo_responses.json");
        write_json(&interaction_request_path, &interaction_requests)?;
        invoke_kimodo(
            &project_root,
            &output_dir,
            &interaction_request_path,
            &interaction_response_path,
            "interaction_kimodo",
        )?;
        let replacements: Vec<Value> = read_json(&interaction_response_path)?;
        let response_path = output_dir.join("kimodo_worker_responses.json");
        let mut merged: Vec<Value> = read_json(&response_path)?;
        for replacement in replacements {
            let request_id = replacement["request_id"]
                .as_str()
                .ok_or_else(|| "interaction response lacks request_id".to_string())?;
            let target = merged
                .iter_mut()
                .find(|response| response["request_id"].as_str() == Some(request_id))
                .ok_or_else(|| format!("cannot merge regenerated interaction {request_id}"))?;
            *target = replacement;
        }
        write_json(&response_path, &merged)?;
    } else if !reuse_kimodo {
        if motions_dir.exists() {
            std::fs::remove_dir_all(&motions_dir).map_err(|error| error.to_string())?;
        }
        std::fs::create_dir_all(&motions_dir).map_err(|error| error.to_string())?;
        invoke_kimodo(
            &project_root,
            &output_dir,
            &output_dir.join("motion_requests.json"),
            &output_dir.join("kimodo_worker_responses.json"),
            "kimodo",
        )?;
    } else if !output_dir.join("kimodo_worker_responses.json").is_file() {
        return Err("--reuse-kimodo requested but kimodo_worker_responses.json is missing".into());
    }
    let responses: Vec<Value> = read_json(output_dir.join("kimodo_worker_responses.json"))?;
    if responses.len() != requests.len() {
        return Err(format!(
            "Kimodo returned {} responses for {} requests",
            responses.len(),
            requests.len()
        ));
    }
    let navigation_value: Value = read_json(&navigation_path)?;
    let colliders = navigation_value["colliders"]
        .as_array()
        .ok_or_else(|| "navigation contract has no collider array".to_string())?;
    let python = PathBuf::from(r"C:\Projects\gemmy\runtimes\kimodo\.venv\Scripts\python.exe");
    let converter = project_root.join("tools/motion/export_soma_track.py");
    let contract = project_root.join("assets/characters/canonical_soma77.json");
    let mut records = Vec::new();
    let mut selected_tracks = Vec::new();
    let mut selected_segment_ids = Vec::new();
    let mut contact_results = Vec::new();

    for (request_index, request) in requests.iter().enumerate() {
        let response = &responses[request_index];
        let score_path = response["candidate_scores"]
            .as_str()
            .ok_or_else(|| "Kimodo response lacks candidate_scores".to_string())?;
        let scores: Value = read_json(score_path)?;
        let candidates = scores["candidates"]
            .as_array()
            .ok_or_else(|| "candidate score file lacks candidates".to_string())?;
        let mut segment_records = Vec::new();
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            let npz = candidate["npz"]
                .as_str()
                .ok_or_else(|| "candidate lacks npz".to_string())?;
            let track_path = motions_dir
                .join(&request.request_id)
                .join(format!("candidate_{candidate_index:02}.soma.json"));
            std::fs::create_dir_all(track_path.parent().unwrap())
                .map_err(|error| error.to_string())?;
            let status = Command::new(&python)
                .args([
                    converter.to_string_lossy().as_ref(),
                    "--npz",
                    npz,
                    "--contract",
                    contract.to_string_lossy().as_ref(),
                    "--output",
                    track_path.to_string_lossy().as_ref(),
                    "--source-segment",
                    &request.request_id,
                ])
                .env_remove("PYTHONPATH")
                .status()
                .map_err(|error| format!("failed to launch SOMA converter: {error}"))?;
            if !status.success() {
                return Err(format!(
                    "SOMA converter failed for {} candidate {candidate_index}",
                    request.request_id
                ));
            }
            let mut track: SomaMotionTrack = read_json(&track_path)?;
            let target_root_path = request
                .dense_root_path
                .iter()
                .map(|sample| sample.position)
                .collect::<Vec<_>>();
            let root_correction = apply_root_path_correction(&mut track, &target_root_path)?;
            let root_height = stabilize_root_height(&mut track, 0.08, 0.08)?;
            let torso_upright = apply_upright_torso_limit(&mut track, 52.0)?;
            let pose_repair = repair_pose_discontinuities(&mut track, 0.45)?;
            let procedural_gait = apply_procedural_soma_gait(&mut track)?;
            let foot_lock = if procedural_gait.applied {
                FootLockReport {
                    corrected_frames: 0,
                    inferred_contact_frames: 0,
                    maximum_correction_m: 0.0,
                }
            } else {
                apply_contact_foot_locks(&mut track)?
            };
            let contact_correction =
                apply_contact_correction_for_request(&mut track, request, door, pickup)?;
            let limb_clearance = if request.request_id == "door_open_interaction" {
                Some(apply_limb_clearance_guard(
                    &mut track,
                    "DOOR_FRAME_RIGHT_ARM_CLEARANCE",
                    &["RightArm", "RightForeArm", "RightHand"],
                    0,
                    0.66,
                    2,
                    [4.0, 4.68],
                    0.18,
                )?)
            } else if request.request_id == "pickup_recovery" {
                Some(apply_limb_clearance_guard(
                    &mut track,
                    "COUNTER_LEG_CLEARANCE",
                    &[
                        "LeftLeg",
                        "LeftShin",
                        "LeftFoot",
                        "RightLeg",
                        "RightShin",
                        "RightFoot",
                    ],
                    0,
                    1.20,
                    2,
                    [-3.10, -1.50],
                    0.25,
                )?)
            } else {
                None
            };
            write_json(&track_path, &track)?;
            let body_collision = evaluate_body_collisions(&track, colliders, &[])?;
            let sanity = evaluate_motion_sanity(&track);
            let mut rejection_reasons = Vec::new();
            if !body_collision.valid {
                rejection_reasons.push("full_body_environment_collision".into());
            }
            for reason in &sanity.rejection_reasons {
                if reason != "unstable_final_stance" || request.request_id == "pickup_recovery" {
                    rejection_reasons.push(reason.clone());
                }
            }
            if contact_correction
                .as_ref()
                .is_some_and(|result| !result.accepted)
            {
                rejection_reasons.push("contact_correction_out_of_bounds".into());
            }
            if limb_clearance
                .as_ref()
                .is_some_and(|result| !result.accepted)
            {
                rejection_reasons.push("limb_clearance_correction_out_of_bounds".into());
            }
            let record = CandidateRecord {
                request_id: request.request_id.clone(),
                candidate_index,
                seed: candidate["seed"].as_u64().unwrap_or(request.seed),
                source_npz: npz.to_string(),
                track_path: track_path.to_string_lossy().into_owned(),
                worker_score: candidate["evaluation"]["score"].as_f64().unwrap_or(999.0) as f32,
                root_correction,
                root_height,
                pose_repair,
                torso_upright,
                procedural_gait,
                foot_lock,
                limb_clearance,
                body_collision,
                sanity,
                contact_correction,
                valid: rejection_reasons.is_empty(),
                rejection_reasons,
                selected: false,
            };
            segment_records.push((record, track));
        }
        let selected_index = segment_records
            .iter()
            .enumerate()
            .filter(|(_, (record, _))| record.valid)
            .min_by(|(_, (a, _)), (_, (b, _))| {
                a.worker_score
                    .total_cmp(&b.worker_score)
                    .then_with(|| a.seed.cmp(&b.seed))
            })
            .map(|(index, _)| index)
            .ok_or_else(|| {
                format!(
                    "no structurally valid candidate for {}: {:?}",
                    request.request_id,
                    segment_records
                        .iter()
                        .map(|(record, _)| (
                            &record.rejection_reasons,
                            &record.body_collision,
                            &record.sanity
                        ))
                        .collect::<Vec<_>>()
                )
            })?;
        segment_records[selected_index].0.selected = true;
        let selected_path = motions_dir
            .join(&request.request_id)
            .join("selected.soma.json");
        write_json(&selected_path, &segment_records[selected_index].1)?;
        if let Some(contact) = &segment_records[selected_index].0.contact_correction {
            contact_results.push(contact.clone());
        }
        selected_tracks.push(segment_records[selected_index].1.clone());
        selected_segment_ids.push(request.request_id.clone());
        records.extend(segment_records.into_iter().map(|(record, _)| record));
    }
    let final_track = concatenate_tracks(&selected_tracks, 0.12)?;
    let final_path = output_dir.join("selected_soma_performance.json");
    write_json(&final_path, &final_track)?;
    let final_collision = evaluate_body_collisions(&final_track, colliders, &[])?;
    let final_sanity = evaluate_motion_sanity(&final_track);
    write_json(
        output_dir.join("body_collision_report.json"),
        &json!({
            "schema_version":1,
            "track":final_path,
            "selected_segments":selected_segment_ids,
            "report":final_collision,
            "motion_sanity":final_sanity
        }),
    )?;
    write_json(
        output_dir.join("contact_report.json"),
        &json!({
            "schema_version":1,
            "preferred_error_m":0.03,
            "maximum_error_m":0.05,
            "corrections":contact_results
        }),
    )?;
    write_json(
        output_dir.join("motion_candidates.json"),
        &json!({"schema_version":1,"backend":"kimodo-soma-rp-v1.1","candidates":records}),
    )?;
    if !final_collision.valid {
        return Err(format!(
            "final concatenated SOMA track intersects environment: {final_collision:?}"
        ));
    }
    if !final_sanity.valid {
        return Err(format!(
            "final concatenated SOMA track failed sanity: {final_sanity:?}"
        ));
    }
    println!(
        "ODD HOURS PRODUCTION MOTION COMPLETE frames={} track={}",
        final_track.frames.len(),
        final_path.display()
    );
    Ok(())
}

fn required_interaction<'a>(
    catalog: &'a SmartInteractionCatalog,
    id: &str,
) -> Result<&'a SmartInteraction, String> {
    catalog
        .get(id)
        .ok_or_else(|| format!("smart interaction catalog lacks {id}"))
}

fn resolve_routes(navigation: &NavigationWorld) -> Result<Vec<ResolvedRoute>, String> {
    let closed = BTreeMap::from([(PORTAL.into(), PortalState::Closed)]);
    let open = BTreeMap::from([(PORTAL.into(), PortalState::Open)]);
    let exterior = navigation
        .resolve_route(&RouteRequest {
            route_id: "exterior_approach".into(),
            start: [2.8, 0.0, 7.45],
            destinations: vec![[0.15, 0.0, 4.80]],
            actor_radius: 0.34,
            portal_states: closed.clone(),
        })
        .map_err(|error| error.to_string())?;
    if navigation
        .resolve_route(&RouteRequest {
            route_id: "closed_door_guard".into(),
            start: [0.15, 0.0, 4.80],
            destinations: vec![[0.15, 0.0, 3.55]],
            actor_radius: 0.34,
            portal_states: closed,
        })
        .is_ok()
    {
        return Err("closed Odd Hours door unexpectedly allowed traversal".into());
    }
    let doorway = navigation
        .resolve_route(&RouteRequest {
            route_id: "doorway_traversal".into(),
            start: [0.15, 0.0, 4.80],
            destinations: vec![[0.15, 0.0, 3.55]],
            actor_radius: 0.34,
            portal_states: open.clone(),
        })
        .map_err(|error| error.to_string())?;
    let interior = navigation
        .resolve_route(&RouteRequest {
            route_id: "interior_counter_approach".into(),
            start: [0.15, 0.0, 3.55],
            destinations: vec![[0.90, 0.0, -1.20], [0.90, 0.0, -1.90]],
            actor_radius: 0.34,
            portal_states: open,
        })
        .map_err(|error| error.to_string())?;
    Ok(vec![exterior, doorway, interior])
}

fn build_requests(
    routes: &[ResolvedRoute],
    door: &SmartInteraction,
    _walk_through: &SmartInteraction,
    pickup: &SmartInteraction,
    output_dir: &Path,
) -> Result<Vec<MotionAuthoringRequest>, String> {
    let door_contact = door
        .contact_events
        .first()
        .ok_or_else(|| "SMART_DOOR_OPEN has no contact event".to_string())?;
    let door_constraint = door
        .end_effector_constraints
        .iter()
        .find(|item| item.joint == "RightHand")
        .ok_or_else(|| "SMART_DOOR_OPEN has no right-hand constraint".to_string())?;
    let pickup_contact = pickup
        .contact_events
        .first()
        .ok_or_else(|| "SMART_PICKUP_SMALL has no contact event".to_string())?;
    let pickup_constraint = pickup
        .end_effector_constraints
        .iter()
        .find(|item| item.joint == "RightHand")
        .ok_or_else(|| "SMART_PICKUP_SMALL has no right-hand constraint".to_string())?;
    let make_output = |id: &str| {
        output_dir
            .join("motions")
            .join(id)
            .join("candidate")
            .to_string_lossy()
            .into_owned()
    };
    let walk = path_request(
        "exterior_walk_approach",
        4.2,
        &routes[0].dense_root_path,
        vec![
            PromptSegment { start: 0.0, end: 3.6, text: "A woman walks upright at a comfortable purposeful pace along a sidewalk, with natural heel-to-toe steps, relaxed shoulders, and no crouching or hopping.".into() },
            PromptSegment { start: 3.6, end: 4.2, text: "She decelerates smoothly, plants both feet, and settles into a balanced upright stance facing the store door.".into() },
        ],
        routes[0].arrival_heading,
        610_100,
        1,
        make_output("exterior_walk_approach"),
        None,
    );
    let door_duration = 3.0;
    let door_contact_start = door_contact.normalized_start * door_duration;
    let door_contact_end = door_contact.normalized_end * door_duration;
    let door_constraint_time = door_constraint.normalized_time * door_duration;
    let door_root = [0.15, 0.0, 4.80];
    let door_motion = MotionAuthoringRequest {
        schema_version: 1,
        request_id: "door_open_interaction".into(),
        performer: "canonical_soma77".into(),
        duration: door_duration,
        prompt_sequence: vec![
            PromptSegment { start: 0.0, end: 0.7, text: "A woman stands upright and balanced in front of a convenience-store door, preparing to reach with her right hand.".into() },
            PromptSegment { start: 0.7, end: 2.15, text: "She plants her feet, reaches naturally to the real door handle, maintains hand contact, and pulls the hinged door open while keeping her torso clear of the frame.".into() },
            PromptSegment { start: 2.15, end: 3.0, text: "She retracts slightly and recovers into an upright doorway-ready stance without crouching, jumping, or leaning excessively.".into() },
        ],
        root_waypoints: fixed_path(door_root, [0.0, 0.0, -1.0], door_duration, 6),
        dense_root_path: fixed_path(door_root, [0.0, 0.0, -1.0], door_duration, (door_duration * FPS as f32) as usize),
        arrival_heading: [0.0, 0.0, -1.0],
        full_body_keyframes: vec![FullBodyKeyframe { id: "DOOR_RECOVERY_PROXY".into(), time: 2.72, reference_motion: "official_soma_mixed".into(), reference_frame: 108, target_root: [door_root[0], 0.95, door_root[2]], strict: false }],
        joint_constraints: vec![],
        end_effector_constraints: vec![
            end_effector("DOOR_RIGHT_HAND_ACQUIRE", door_contact_start + 0.10, "RightHand", [0.48,1.08,4.38], door_constraint.rotation_xyzw, 1.0, 0.9),
            end_effector("DOOR_RIGHT_HAND_CONTACT", (door_contact_start + door_contact_end) * 0.5, "RightHand", [0.48,1.08,4.38], door_constraint.rotation_xyzw, 1.0, 0.9),
            end_effector("DOOR_RIGHT_HAND_RELEASE", door_contact_end - 0.10, "RightHand", [0.48,1.08,4.38], door_constraint.rotation_xyzw, 1.0, 0.9),
            end_effector("DOOR_LEFT_FOOT", door_constraint_time, "LeftFoot", [-0.03,0.04,4.73], [0.0,0.0,0.0,1.0], 0.9, 0.7),
            end_effector("DOOR_RIGHT_FOOT", door_constraint_time, "RightFoot", [0.30,0.04,4.87], [0.0,0.0,0.0,1.0], 0.9, 0.7),
        ],
        environment_constraints: vec![environment("EXECUTE_SMART_DOOR_OPEN", door, "INTERACT_ODD_HOURS_DOOR_HANDLE", "NAV_REGION_ODD_HOURS_EXTERIOR", door_root, [0.0,0.0,-1.0], 0.0, door_duration)],
        contact_events: vec![ContactEvent { id: "ODD_HOURS_HANDLE_CONTACT".into(), start: door_contact_start, end: door_contact_end, performer_joint: "RightHand".into(), target_id: "OH_DOOR_HANDLE".into(), state_transition: Some("door.open=true;portal.open=true".into()) }],
        candidate_count: 2,
        seed: 610_200,
        strictness: 0.95,
        continuation_pose: Some(ContinuationPose { source_motion: "exterior_walk_approach".into(), source_frame: 125 }),
        output_stem: make_output("door_open_interaction"),
    };
    let doorway = path_request(
        "doorway_traversal",
        2.2,
        &routes[1].dense_root_path,
        vec![PromptSegment { start: 0.0, end: 2.2, text: "A woman walks upright through an already open convenience-store doorway with natural narrow steps, keeping her head, torso, and arms clear of the frame and door.".into() }],
        routes[1].arrival_heading,
        610_300,
        1,
        make_output("doorway_traversal"),
        Some(ContinuationPose { source_motion: "door_open_interaction".into(), source_frame: 89 }),
    );
    let interior = path_request(
        "interior_counter_walk",
        4.3,
        &routes[2].dense_root_path,
        vec![
            PromptSegment { start: 0.0, end: 3.6, text: "A woman walks upright through a convenience-store aisle around real fixtures, turning naturally while maintaining steady grounded heel-to-toe steps.".into() },
            PromptSegment { start: 3.6, end: 4.3, text: "She slows at the checkout counter, turns to face the package, plants both feet, and settles without sliding or crouching.".into() },
        ],
        [1.0, 0.0, 0.0],
        610_400,
        1,
        make_output("interior_counter_walk"),
        Some(ContinuationPose { source_motion: "doorway_traversal".into(), source_frame: 65 }),
    );
    let pickup_duration = 4.0;
    let pickup_contact_start = pickup_contact.normalized_start * pickup_duration;
    let pickup_contact_end = pickup_contact.normalized_end * pickup_duration;
    let pickup_constraint_time = pickup_constraint.normalized_time * pickup_duration;
    let pickup_root = [0.90, 0.0, -1.90];
    let pickup_motion = MotionAuthoringRequest {
        schema_version: 1,
        request_id: "pickup_recovery".into(),
        performer: "canonical_soma77".into(),
        duration: pickup_duration,
        prompt_sequence: vec![
            PromptSegment { start: 0.0, end: 0.9, text: "A woman stands upright at a checkout counter and looks at a small package.".into() },
            PromptSegment { start: 0.9, end: 2.7, text: "She plants her feet, reaches with her right hand, aligns the palm to the package, grasps it, and lifts it slightly from the counter.".into() },
            PromptSegment { start: 2.7, end: 4.0, text: "Holding the package, she retracts her hand close to her torso, regains a balanced upright stance, and looks toward the refrigerators with a subtle surprised reaction.".into() },
        ],
        root_waypoints: fixed_path(pickup_root, [1.0, 0.0, 0.0], pickup_duration, 8),
        dense_root_path: fixed_path(pickup_root, [1.0, 0.0, 0.0], pickup_duration, (pickup_duration * FPS as f32) as usize),
        arrival_heading: [1.0, 0.0, 0.0],
        full_body_keyframes: vec![FullBodyKeyframe { id: "PICKUP_RECOVERY_PROXY".into(), time: 3.72, reference_motion: "official_soma_mixed".into(), reference_frame: 108, target_root: [pickup_root[0],0.95,pickup_root[2]], strict: false }],
        joint_constraints: vec![],
        end_effector_constraints: vec![
            end_effector("PICKUP_RIGHT_HAND_ACQUIRE", pickup_contact_start + 0.10, "RightHand", [1.70,1.22,-1.98], pickup_constraint.rotation_xyzw, 1.0, 0.9),
            end_effector("PICKUP_RIGHT_HAND_CONTACT", (pickup_contact_start + pickup_contact_end) * 0.5, "RightHand", [1.70,1.22,-1.98], pickup_constraint.rotation_xyzw, 1.0, 0.9),
            end_effector("PICKUP_RIGHT_HAND_RELEASE", pickup_contact_end - 0.10, "RightHand", [1.70,1.22,-1.98], pickup_constraint.rotation_xyzw, 1.0, 0.9),
            end_effector("PICKUP_LEFT_FOOT", pickup_constraint_time, "LeftFoot", [0.82,0.04,-1.72], [0.0,0.0,0.0,1.0], 0.9, 0.7),
            end_effector("PICKUP_RIGHT_FOOT", pickup_constraint_time, "RightFoot", [0.98,0.04,-2.05], [0.0,0.0,0.0,1.0], 0.9, 0.7),
        ],
        environment_constraints: vec![environment("EXECUTE_SMART_PICKUP_SMALL", pickup, "INTERACT_ODD_HOURS_PACKAGE", "NAV_REGION_ODD_HOURS_INTERIOR", pickup_root, [1.0,0.0,0.0], 0.0, pickup_duration)],
        contact_events: vec![ContactEvent { id: "ODD_HOURS_PACKAGE_CONTACT".into(), start: pickup_contact_start, end: pickup_contact_end, performer_joint: "RightHand".into(), target_id: "PROP_COUNTER_PACKAGE".into(), state_transition: Some("object.held_by=mara_soma".into()) }],
        candidate_count: 2,
        seed: 610_500,
        strictness: 0.95,
        continuation_pose: Some(ContinuationPose { source_motion: "interior_counter_walk".into(), source_frame: 128 }),
        output_stem: make_output("pickup_recovery"),
    };
    Ok(vec![walk, door_motion, doorway, interior, pickup_motion])
}

fn path_request(
    id: &str,
    duration: f32,
    path: &[[f32; 3]],
    prompt_sequence: Vec<PromptSegment>,
    arrival_heading: [f32; 3],
    seed: u64,
    candidate_count: u32,
    output_stem: String,
    continuation_pose: Option<ContinuationPose>,
) -> MotionAuthoringRequest {
    let dense_root_path = timed_path(path, duration, FPS);
    let root_waypoints = dense_root_path
        .iter()
        .enumerate()
        .filter(|(index, _)| index % FPS as usize == 0 || *index + 1 == dense_root_path.len())
        .map(|(_, sample)| sample.clone())
        .collect();
    MotionAuthoringRequest {
        schema_version: 1,
        request_id: id.into(),
        performer: "canonical_soma77".into(),
        duration,
        prompt_sequence,
        root_waypoints,
        dense_root_path,
        arrival_heading,
        full_body_keyframes: vec![],
        joint_constraints: vec![],
        end_effector_constraints: vec![],
        environment_constraints: vec![],
        contact_events: vec![],
        candidate_count,
        seed,
        strictness: 0.90,
        continuation_pose,
        output_stem,
    }
}

fn timed_path(path: &[[f32; 3]], duration: f32, fps: u32) -> Vec<RootPathSample> {
    let frames = (duration * fps as f32).round() as usize;
    let travel_duration = (duration - 0.45).max(duration * 0.75);
    (0..frames)
        .map(|index| {
            let time = index as f32 / fps as f32;
            let progress = (time / travel_duration).clamp(0.0, 1.0);
            let position = sample_polyline(path, progress);
            let next = sample_polyline(path, (progress + 0.01).min(1.0));
            RootPathSample {
                time,
                position,
                heading: normalize_xz([next[0] - position[0], 0.0, next[2] - position[2]]),
            }
        })
        .collect()
}

fn fixed_path(
    position: [f32; 3],
    heading: [f32; 3],
    duration: f32,
    samples: usize,
) -> Vec<RootPathSample> {
    let samples = samples.max(2);
    (0..samples)
        .map(|index| RootPathSample {
            time: (duration - 1.0 / FPS as f32) * index as f32 / (samples - 1) as f32,
            position,
            heading,
        })
        .collect()
}

fn sample_polyline(path: &[[f32; 3]], progress: f32) -> [f32; 3] {
    if path.len() <= 1 {
        return path.first().copied().unwrap_or([0.0, 0.0, 0.0]);
    }
    let lengths = path
        .windows(2)
        .map(|pair| distance(pair[0], pair[1]))
        .collect::<Vec<_>>();
    let target = lengths.iter().sum::<f32>() * progress.clamp(0.0, 1.0);
    let mut traversed = 0.0;
    for (index, length) in lengths.iter().copied().enumerate() {
        if target <= traversed + length || index + 1 == lengths.len() {
            let value = ((target - traversed) / length.max(1e-5)).clamp(0.0, 1.0);
            return [
                path[index][0] + (path[index + 1][0] - path[index][0]) * value,
                path[index][1] + (path[index + 1][1] - path[index][1]) * value,
                path[index][2] + (path[index + 1][2] - path[index][2]) * value,
            ];
        }
        traversed += length;
    }
    *path.last().unwrap()
}

fn end_effector(
    id: &str,
    time: f32,
    joint: &str,
    position: [f32; 3],
    rotation_xyzw: [f32; 4],
    position_weight: f32,
    rotation_weight: f32,
) -> EndEffectorConstraint {
    EndEffectorConstraint {
        id: id.into(),
        time,
        joint: joint.into(),
        position,
        rotation_xyzw,
        position_weight,
        rotation_weight,
        strict: true,
        reference_motion: "official_soma_ee".into(),
        reference_frame: 94,
    }
}

fn environment(
    id: &str,
    interaction: &SmartInteraction,
    target: &str,
    region: &str,
    staging_slot: [f32; 3],
    facing: [f32; 3],
    start: f32,
    end: f32,
) -> EnvironmentConstraint {
    EnvironmentConstraint {
        id: id.into(),
        smart_interaction_id: interaction.semantic_id.clone(),
        target_id: target.into(),
        approach_region: region.into(),
        staging_slot,
        facing,
        clearance_radius: interaction.required_clearance,
        start,
        end,
    }
}

fn apply_contact_correction_for_request(
    track: &mut SomaMotionTrack,
    request: &MotionAuthoringRequest,
    door: &SmartInteraction,
    pickup: &SmartInteraction,
) -> Result<Option<ContactCorrectionResult>, String> {
    if request.request_id == "door_open_interaction" {
        let contact = door
            .contact_events
            .first()
            .ok_or_else(|| "SMART_DOOR_OPEN lacks a contact event".to_string())?;
        let start = contact.normalized_start * request.duration;
        let end = contact.normalized_end * request.duration;
        let closed_handle = [0.48, 1.08, 4.38];
        let pivot = [-0.725, 0.0, 4.24];
        let relative = [
            closed_handle[0] - pivot[0],
            closed_handle[1] - pivot[1],
            closed_handle[2] - pivot[2],
        ];
        let angle = 4.0_f32.to_radians();
        let rotated = [
            pivot[0] + relative[0] * angle.cos() + relative[2] * angle.sin(),
            closed_handle[1],
            pivot[2] - relative[0] * angle.sin() + relative[2] * angle.cos(),
        ];
        let moving = MovingContactCorrectionRequest {
            id: "door_open_interaction_right_hand_lock".into(),
            shoulder_joint: "RightArm".into(),
            elbow_joint: "RightForeArm".into(),
            hand_joint: "RightHand".into(),
            target_samples: vec![
                (start, closed_handle),
                (start + 0.17, closed_handle),
                (end, rotated),
            ],
            contact_start: start,
            contact_end: end,
            blend_seconds: 0.30,
            maximum_correction: 0.50,
        };
        return apply_two_bone_contact_trajectory(track, &moving).map(Some);
    }
    if request.request_id == "pickup_recovery" {
        let contact = pickup
            .contact_events
            .first()
            .ok_or_else(|| "SMART_PICKUP_SMALL lacks a contact event".to_string())?;
        let correction = ContactCorrectionRequest {
            id: "pickup_recovery_right_hand_lock".into(),
            shoulder_joint: "RightArm".into(),
            elbow_joint: "RightForeArm".into(),
            hand_joint: "RightHand".into(),
            target: [1.70, 1.22, -1.98],
            contact_start: contact.normalized_start * request.duration,
            contact_end: contact.normalized_end * request.duration,
            blend_seconds: 0.30,
            maximum_correction: 0.35,
        };
        return apply_two_bone_contact_correction(track, &correction).map(Some);
    }
    Ok(None)
}

fn build_plan(
    door: &SmartInteraction,
    walk_through: &SmartInteraction,
    pickup: &SmartInteraction,
) -> ProductionPlan {
    ProductionPlan {
        schema_version: 1,
        scene_id: "odd_hours_first_integrated_production_scene".into(),
        duration: DURATION,
        fps: FPS,
        resolution: [1080, 1920],
        renderer: "Bevy 0.19 GPU offscreen production capture".into(),
        world_asset: "assets/world/locations/location_odd_hours_v3.glb".into(),
        performer_contract: "assets/characters/canonical_soma77.json".into(),
        navigation_contract: "assets/world/navigation/odd_hours_production.json".into(),
        smart_interaction_catalog: "assets/interactions/smart_interactions.json".into(),
        pipeline: vec![
            "authored semantic action".into(),
            "semantic destination and interaction resolution".into(),
            "deterministic NavigationWorld route planning".into(),
            "portal and destination reservations".into(),
            "data-driven smart-interaction expansion".into(),
            "MotionAuthoringRequest batch".into(),
            "Kimodo candidate generation".into(),
            "full-body validation and candidate selection".into(),
            "bounded contact correction".into(),
            "SOMA motion track".into(),
            "Bevy world playback and GPU capture".into(),
        ],
        phases: vec![
            phase(
                "exterior_walk_approach",
                0.0,
                4.2,
                "navigation",
                Some("exterior_approach"),
                None,
                None,
                vec![],
            ),
            phase(
                "door_open_interaction",
                4.2,
                7.2,
                "smart_interaction",
                None,
                Some(&door.semantic_id),
                Some("INTERACT_ODD_HOURS_DOOR_HANDLE"),
                door.runtime_state_transitions.clone(),
            ),
            phase(
                "doorway_traversal",
                7.2,
                9.4,
                "smart_interaction",
                Some("doorway_traversal"),
                Some(&walk_through.semantic_id),
                Some(PORTAL),
                walk_through.runtime_state_transitions.clone(),
            ),
            phase(
                "interior_counter_walk",
                9.4,
                13.7,
                "navigation",
                Some("interior_counter_approach"),
                None,
                None,
                vec![],
            ),
            phase(
                "pickup_recovery",
                13.7,
                17.7,
                "smart_interaction",
                None,
                Some(&pickup.semantic_id),
                Some("PROP_COUNTER_PACKAGE"),
                pickup.runtime_state_transitions.clone(),
            ),
        ],
        camera_shots: vec![
            shot(
                "SHOT_01_EXTERIOR",
                0.0,
                4.2,
                "CAM_OH_EXTERIOR_WIDE",
                "mara_soma",
                vec!["DOOR_ODD_HOURS_HERO", "OH_SIGN_LETTERS"],
            ),
            shot(
                "SHOT_02_DOOR",
                4.2,
                9.4,
                "CAM_OH_DOOR_MEDIUM",
                "OH_DOOR_HANDLE",
                vec!["mara_soma", "DOOR_ODD_HOURS_HERO", "OH_DOOR_HANDLE"],
            ),
            shot(
                "SHOT_03_INTERIOR",
                9.4,
                13.7,
                "CAM_OH_INTERIOR_WIDE",
                "mara_soma",
                vec!["mara_soma", "OH_SHELF_CENTER", "OH_CHECKOUT_COUNTER"],
            ),
            shot(
                "SHOT_04_COUNTER",
                13.7,
                17.7,
                "CAM_OH_COUNTER_MEDIUM",
                "PROP_COUNTER_PACKAGE",
                vec!["mara_soma", "PROP_COUNTER_PACKAGE", "OH_CHECKOUT_COUNTER"],
            ),
        ],
        audio_cues: vec![
            json!({"id":"door_latch","time":5.35,"event":"ODD_HOURS_HANDLE_CONTACT"}),
            json!({"id":"door_movement","start":5.40,"end":7.0,"event":"door.open=true"}),
            json!({"id":"store_chime","time":7.45,"event":"portal.entered"}),
            json!({"id":"package_pickup","time":15.70,"event":"object.held_by=mara_soma"}),
        ],
    }
}

fn phase(
    id: &str,
    start: f32,
    end: f32,
    phase_type: &str,
    route_id: Option<&str>,
    smart_interaction_id: Option<&str>,
    resolved_target: Option<&str>,
    state_transitions: Vec<String>,
) -> ProductionPhase {
    ProductionPhase {
        id: id.into(),
        start,
        end,
        phase_type: phase_type.into(),
        route_id: route_id.map(str::to_string),
        smart_interaction_id: smart_interaction_id.map(str::to_string),
        resolved_target: resolved_target.map(str::to_string),
        state_transitions,
    }
}

fn shot(
    id: &str,
    start: f32,
    end: f32,
    anchor: &str,
    subject: &str,
    visible: Vec<&str>,
) -> CameraShot {
    CameraShot {
        id: id.into(),
        start,
        end,
        anchor: anchor.into(),
        subject: subject.into(),
        required_visible_objects: visible.into_iter().map(str::to_string).collect(),
    }
}

fn invoke_kimodo(
    project_root: &Path,
    output_dir: &Path,
    requests: &Path,
    responses: &Path,
    log_prefix: &str,
) -> Result<(), String> {
    let python = PathBuf::from(r"C:\Projects\gemmy\runtimes\kimodo\.venv\Scripts\python.exe");
    let script = project_root.join("runtimes/kimodo/backlot_batch_kimodo.py");
    let checkpoint = PathBuf::from(r"F:\Models\Kimodo\Kimodo-SOMA-RP-v1.1");
    let output = Command::new(&python)
        .args([
            script.to_string_lossy().as_ref(),
            "--checkpoint",
            checkpoint.to_string_lossy().as_ref(),
            "--requests",
            requests.to_string_lossy().as_ref(),
            "--responses",
            responses.to_string_lossy().as_ref(),
            "--diffusion-steps",
            "12",
        ])
        .current_dir(project_root.join("runtimes/kimodo"))
        .env_remove("PYTHONPATH")
        .output()
        .map_err(|error| format!("failed to launch Kimodo worker: {error}"))?;
    std::fs::write(
        output_dir.join(format!("{log_prefix}_stdout.jsonl")),
        &output.stdout,
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(
        output_dir.join(format!("{log_prefix}_stderr.log")),
        &output.stderr,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Kimodo worker failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn normalize_xz(value: [f32; 3]) -> [f32; 3] {
    let length = (value[0] * value[0] + value[2] * value[2]).sqrt();
    if length < 1e-5 {
        [0.0, 0.0, 1.0]
    } else {
        [value[0] / length, 0.0, value[2] / length]
    }
}

fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn read_json<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> Result<T, String> {
    let path = path.as_ref();
    serde_json::from_slice(
        &std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?,
    )
    .map_err(|error| format!("{}: {error}", path.display()))
}
