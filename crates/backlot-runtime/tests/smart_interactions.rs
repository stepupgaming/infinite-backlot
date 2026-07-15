use backlot_runtime::smart_interactions::SmartInteractionCatalog;
use std::path::PathBuf;

fn catalog_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("assets/interactions/smart_interactions.json")
}

#[test]
fn catalog_is_data_driven_and_contains_reusable_initial_set() {
    let catalog = SmartInteractionCatalog::from_path(&catalog_path()).expect("catalog loads");
    catalog.validate().expect("catalog validates");
    assert!(catalog.interactions.len() >= 10);
    for id in [
        "SMART_PANEL_PRESS",
        "SMART_DOOR_OPEN",
        "SMART_DOOR_WALK_THROUGH",
        "SMART_SIT",
        "SMART_STAND",
        "SMART_PICKUP_SMALL",
        "SMART_HANDOFF",
        "SMART_BUS_STOP_WAIT",
        "SMART_LOOK_PAST",
        "SMART_STEP_ASIDE",
    ] {
        let interaction = catalog.get(id).unwrap_or_else(|| panic!("missing {id}"));
        assert!(!interaction.approach_regions.is_empty());
        assert!(!interaction.staging_slots.is_empty());
        assert!(!interaction.supported_motion_backends.is_empty());
        assert!(interaction.required_clearance > 0.0);
    }
}

#[test]
fn object_type_compatibility_is_checked_without_rust_match_tables() {
    let catalog = SmartInteractionCatalog::from_path(&catalog_path()).unwrap();
    assert!(catalog.compatible("SMART_PANEL_PRESS", "wall_panel"));
    assert!(catalog.compatible("SMART_DOOR_OPEN", "hinged_door"));
    assert!(!catalog.compatible("SMART_SIT", "wall_panel"));
}
