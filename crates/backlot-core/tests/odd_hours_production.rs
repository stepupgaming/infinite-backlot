use backlot_core::navigation::{NavigationWorld, PortalState, RouteRequest, RouteStatus};
use std::collections::BTreeMap;
use std::path::Path;

fn world() -> NavigationWorld {
    NavigationWorld::from_path(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("assets/world/navigation/odd_hours_production.json"),
    )
    .unwrap()
}

#[test]
fn odd_hours_routes_resolve_only_when_the_real_door_portal_is_open() {
    let navigation = world();
    let exterior = navigation
        .resolve_route(&RouteRequest {
            route_id: "exterior_approach".into(),
            start: [2.8, 0.0, 7.45],
            destinations: vec![[0.15, 0.0, 4.80]],
            actor_radius: 0.34,
            portal_states: BTreeMap::from([(
                "NAV_PORTAL_ODD_HOURS_FRONT_DOOR".into(),
                PortalState::Closed,
            )]),
        })
        .unwrap();
    assert_eq!(exterior.status, RouteStatus::Resolved);
    assert!(exterior.portal_sequence.is_empty());

    let closed = navigation.resolve_route(&RouteRequest {
        route_id: "closed_doorway".into(),
        start: [0.15, 0.0, 4.80],
        destinations: vec![[0.15, 0.0, 3.55]],
        actor_radius: 0.34,
        portal_states: BTreeMap::from([(
            "NAV_PORTAL_ODD_HOURS_FRONT_DOOR".into(),
            PortalState::Closed,
        )]),
    });
    assert!(closed.is_err());

    let open = navigation
        .resolve_route(&RouteRequest {
            route_id: "open_doorway".into(),
            start: [0.15, 0.0, 4.80],
            destinations: vec![[0.15, 0.0, 3.55]],
            actor_radius: 0.34,
            portal_states: BTreeMap::from([(
                "NAV_PORTAL_ODD_HOURS_FRONT_DOOR".into(),
                PortalState::Open,
            )]),
        })
        .unwrap();
    assert!(open
        .portal_sequence
        .contains(&"NAV_PORTAL_ODD_HOURS_FRONT_DOOR".into()));
    assert_eq!(open.validation.static_collision_intersections, 0);
    assert_eq!(open.validation.unsupported_floor_samples, 0);
}

#[test]
fn final_counter_route_stays_outside_the_counter_clearance() {
    let route = world()
        .resolve_route(&RouteRequest {
            route_id: "counter_approach".into(),
            start: [0.15, 0.0, 3.55],
            destinations: vec![[0.90, 0.0, -1.20], [0.90, 0.0, -1.90]],
            actor_radius: 0.34,
            portal_states: BTreeMap::from([(
                "NAV_PORTAL_ODD_HOURS_FRONT_DOOR".into(),
                PortalState::Open,
            )]),
        })
        .unwrap();
    assert_eq!(route.status, RouteStatus::Resolved);
    assert_eq!(route.validation.static_collision_intersections, 0);
    assert_eq!(route.validation.clearance_failures, 0);
    assert_eq!(route.validation.unsupported_floor_samples, 0);
    assert!(route.dense_root_path.len() > route.raw_path.len());
}
