use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn output() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("output/production-vertical-slice")
}

fn json(name: &str) -> Value {
    let path = output().join(name);
    serde_json::from_slice(&fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "required production artifact {} is unreadable: {error}",
            path.display()
        )
    }))
    .unwrap_or_else(|error| {
        panic!(
            "required production artifact {} is invalid: {error}",
            path.display()
        )
    })
}

#[test]
fn integrated_odd_hours_plan_executes_navigation_and_smart_interactions() {
    let plan = json("production_plan.json");
    let duration = plan["duration"].as_f64().unwrap();
    assert!((15.0..=25.0).contains(&duration));
    assert_eq!(
        plan["renderer"],
        "Bevy 0.19 GPU offscreen production capture"
    );
    let interactions = plan["phases"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|phase| phase["smart_interaction_id"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        interactions,
        [
            "SMART_DOOR_OPEN",
            "SMART_DOOR_WALK_THROUGH",
            "SMART_PICKUP_SMALL"
        ]
    );

    let routes = json("resolved_routes.json");
    assert_eq!(routes["closed_door_crossing_rejected"], true);
    assert_eq!(routes["routes"].as_array().unwrap().len(), 3);
    assert_eq!(routes["portal_state_sequence"][2]["state"], "open");
    assert_eq!(
        routes["destination_reservation"]["destination"],
        "ODD_HOURS_COUNTER_INTERACTION"
    );
}

#[test]
fn selected_native_soma_motion_is_contact_corrected_and_full_body_safe() {
    let track = json("selected_soma_performance.json");
    assert_eq!(track["joint_names"].as_array().unwrap().len(), 77);
    assert_eq!(track["fps"], 30);
    let frames = track["frames"].as_array().unwrap().len();
    assert!((450..=750).contains(&frames));

    let candidates = json("motion_candidates.json");
    let selected = candidates["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|candidate| candidate["selected"].as_bool() == Some(true))
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 5);
    assert!(selected
        .iter()
        .all(|candidate| candidate["valid"].as_bool() == Some(true)));

    let collision = json("body_collision_report.json");
    let report = &collision["report"];
    assert_eq!(report["valid"], true);
    assert_eq!(report["root_collision_samples"], 0);
    assert_eq!(report["body_collision_samples"], 0);
    assert_eq!(report["limb_collision_samples"], 0);

    let contacts = json("contact_report.json");
    let corrections = contacts["corrections"].as_array().unwrap();
    assert_eq!(corrections.len(), 2);
    assert!(corrections.iter().all(|correction| {
        correction["accepted"].as_bool() == Some(true)
            && correction["after_error_m"].as_f64().unwrap() <= 0.05
    }));
}
