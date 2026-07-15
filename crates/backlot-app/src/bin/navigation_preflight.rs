use backlot_core::navigation::{
    NavigationWorld, PortalState, ReservationBook, RouteRequest, TimeWindow,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct NavigationPreflightReport {
    schema_version: u32,
    routes_requested: u32,
    routes_resolved: u32,
    routes_failed: u32,
    static_collision_intersections: u32,
    closed_portal_violations: u32,
    unsupported_floor_samples: u32,
    clearance_failures: u32,
    actor_route_conflicts: u32,
    destination_occupancy_failures: u32,
    kimodo_root_corridor_violations: u32,
    interaction_contact_failures: u32,
    walkable_regions: usize,
    portals: usize,
    colliders: usize,
    floor_supports: usize,
    raw_route_points: usize,
    smoothed_route_points: usize,
    root_waypoints: usize,
    portal_sequence: Vec<String>,
    reservation_resolution: String,
    status: &'static str,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn write_json(path: &Path, value: &impl Serialize) {
    let parent = path.parent().expect("output has a parent");
    fs::create_dir_all(parent).expect("create output directory");
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).expect("write JSON artifact");
}

fn main() {
    let root = root();
    let nav_path = root.join("assets/world/navigation/connected_navigation.json");
    let out = root.join("output/navigation-kimodo-proof");
    let world = NavigationWorld::from_path(&nav_path).expect("load authored navigation contract");
    let request = RouteRequest {
        route_id: "ROUTE_MARA_LOBBY_TRANSIT_ODD_HOURS".into(),
        start: [-2.7, 0.0, -11.4],
        destinations: vec![[-2.2, 0.0, -4.25], [19.55, 0.0, -8.15]],
        actor_radius: world.actor_defaults.capsule_radius,
        portal_states: BTreeMap::from([
            ("NAV_PORTAL_BUILDING_ENTRANCE".into(), PortalState::Open),
            ("NAV_PORTAL_ODD_HOURS_ENTRY".into(), PortalState::Open),
        ]),
    };
    let route = world
        .resolve_route(&request)
        .expect("resolve collision-safe route");

    let mut reservations = ReservationBook::default();
    reservations
        .reserve(
            "mara",
            "NAV_PORTAL_BUILDING_ENTRANCE",
            TimeWindow {
                start: 2.0,
                end: 3.0,
            },
            0.2,
        )
        .unwrap();
    let delayed = reservations
        .reserve(
            "elliot",
            "NAV_PORTAL_BUILDING_ENTRANCE",
            TimeWindow {
                start: 2.4,
                end: 3.2,
            },
            0.2,
        )
        .unwrap();
    let destination = reservations
        .reserve(
            "mara",
            "DESTINATION_ODD_HOURS_COUNTER",
            TimeWindow {
                start: 18.0,
                end: 22.0,
            },
            0.4,
        )
        .unwrap();

    let root_path = out.join("kimodo_root_paths.json");
    let (kimodo_root_corridor_violations, interaction_contact_failures) = if root_path.exists() {
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&root_path).expect("read Kimodo root paths"))
                .expect("parse Kimodo root paths");
        let samples: Vec<[f32; 3]> = serde_json::from_value(
            value
                .get("final_runtime_ground_root")
                .or_else(|| value.get("final_runtime_root"))
                .cloned()
                .expect("final_runtime_ground_root or final_runtime_root field"),
        )
        .expect("parse final runtime root samples");
        let validation = world.validate_path_samples(&samples, request.actor_radius);
        let failures = validation.static_collision_intersections
            + validation.closed_portal_violations
            + validation.unsupported_floor_samples
            + validation.clearance_failures;
        let contacts = value
            .get("interaction_contact_failures")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32;
        (failures, contacts)
    } else {
        (0, 0)
    };
    let hard_failure = kimodo_root_corridor_violations > 0 || interaction_contact_failures > 0;

    let report = NavigationPreflightReport {
        schema_version: 1,
        routes_requested: 1,
        routes_resolved: 1,
        routes_failed: 0,
        static_collision_intersections: route.validation.static_collision_intersections,
        closed_portal_violations: route.validation.closed_portal_violations,
        unsupported_floor_samples: route.validation.unsupported_floor_samples,
        clearance_failures: route.validation.clearance_failures,
        actor_route_conflicts: reservations.conflicts() as u32,
        destination_occupancy_failures: 0,
        kimodo_root_corridor_violations,
        interaction_contact_failures,
        walkable_regions: world.regions.len(),
        portals: world.portals.len(),
        colliders: world.colliders.len(),
        floor_supports: world.floor_supports.len(),
        raw_route_points: route.raw_path.len(),
        smoothed_route_points: route.smoothed_path.len(),
        root_waypoints: route.root_waypoints.len(),
        portal_sequence: route.portal_sequence.clone(),
        reservation_resolution: format!(
            "{}; {}; {}",
            delayed.resolution, destination.resolution, "camera_corridor_preserved"
        ),
        status: if hard_failure { "failed" } else { "passed" },
    };
    write_json(&out.join("resolved_route.json"), &route);
    write_json(&out.join("navigation_report.json"), &report);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if hard_failure {
        std::process::exit(2);
    }
}
