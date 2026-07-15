use backlot_core::navigation::{
    NavigationWorld, PortalState, ReservationBook, RouteRequest, RouteStatus, TimeWindow,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn world_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("assets/world/navigation/connected_navigation.json")
}

#[test]
fn resolves_lobby_transit_store_route_around_authored_colliders() {
    let world = NavigationWorld::from_path(&world_path()).expect("navigation contract loads");
    let request = RouteRequest {
        route_id: "ROUTE_MARA_LOBBY_TRANSIT_ODD_HOURS".into(),
        start: [-2.7, 0.0, -11.4],
        destinations: vec![[-2.2, 0.0, -4.25], [19.55, 0.0, -8.15]],
        actor_radius: 0.34,
        portal_states: BTreeMap::from([
            ("NAV_PORTAL_BUILDING_ENTRANCE".into(), PortalState::Open),
            ("NAV_PORTAL_ODD_HOURS_ENTRY".into(), PortalState::Open),
        ]),
    };
    let route = world.resolve_route(&request).expect("route resolves");
    assert_eq!(route.status, RouteStatus::Resolved);
    assert!(
        route.raw_path.len() >= 8,
        "route needs meaningful obstacle turns"
    );
    assert!(route.smoothed_path.len() > route.raw_path.len());
    assert!(route
        .portal_sequence
        .contains(&"NAV_PORTAL_BUILDING_ENTRANCE".into()));
    assert!(route
        .portal_sequence
        .contains(&"NAV_PORTAL_ODD_HOURS_ENTRY".into()));
    assert!(route.region_sequence.contains(&"NAV_REGION_LOBBY".into()));
    assert!(route
        .region_sequence
        .contains(&"NAV_REGION_TRANSIT_POCKET".into()));
    assert!(route
        .region_sequence
        .contains(&"NAV_REGION_ODD_HOURS_INTERIOR".into()));
    assert_eq!(route.validation.static_collision_intersections, 0);
    assert_eq!(route.validation.closed_portal_violations, 0);
    assert_eq!(route.validation.unsupported_floor_samples, 0);
    assert_eq!(route.validation.clearance_failures, 0);
}

#[test]
fn closed_runtime_door_blocks_the_only_store_portal() {
    let world = NavigationWorld::from_path(&world_path()).expect("navigation contract loads");
    let request = RouteRequest {
        route_id: "ROUTE_CLOSED_STORE".into(),
        start: [3.0, 0.0, -4.6],
        destinations: vec![[19.55, 0.0, -8.15]],
        actor_radius: 0.34,
        portal_states: BTreeMap::from([("NAV_PORTAL_ODD_HOURS_ENTRY".into(), PortalState::Closed)]),
    };
    let failure = world
        .resolve_route(&request)
        .expect_err("closed door rejects route");
    assert!(failure.message.contains("NAV_PORTAL_ODD_HOURS_ENTRY"));
}

#[test]
fn temporal_reservations_delay_conflicting_portal_use_deterministically() {
    let mut reservations = ReservationBook::default();
    let first = reservations
        .reserve(
            "mara",
            "NAV_PORTAL_BUILDING_ENTRANCE",
            TimeWindow {
                start: 2.0,
                end: 3.2,
            },
            0.2,
        )
        .expect("first reservation succeeds");
    let second = reservations
        .reserve(
            "elliot",
            "NAV_PORTAL_BUILDING_ENTRANCE",
            TimeWindow {
                start: 2.4,
                end: 3.0,
            },
            0.2,
        )
        .expect("second reservation is delayed");
    assert_eq!(first.window.start, 2.0);
    assert_eq!(second.window.start, 3.4);
    assert_eq!(second.resolution, "delayed_after_mara");
}
