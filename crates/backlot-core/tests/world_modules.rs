use backlot_core::world_modules::{DemonstrationLayout, WorldModuleRegistry};
use backlot_motion::library::MotionLibrary;
use std::path::PathBuf;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn committed_registry_loads_without_custom_rust_scene_code() {
    let registry = WorldModuleRegistry::load(project_root().join("assets/world/registry.json"))
        .expect("load committed module registry");
    assert_eq!(registry.modules.len(), 30);
    registry
        .validate(&project_root())
        .expect("valid module registry");
    assert!(registry
        .modules
        .iter()
        .any(|module| module.category == "lobby"));
    assert!(registry
        .modules
        .iter()
        .any(|module| module.category == "street"));
    assert!(registry
        .modules
        .iter()
        .any(|module| module.module_id == "infinite_backlot_block"
            && module.category == "connected_neighborhood"));
}

#[test]
fn demonstration_layout_resolves_modules_versions_and_sockets() {
    let registry = WorldModuleRegistry::load(project_root().join("assets/world/registry.json"))
        .expect("load committed module registry");
    let layout =
        DemonstrationLayout::load(project_root().join("data/world/demo_world_seed_424242.json"))
            .expect("load committed demonstration layout");
    layout
        .validate(&registry)
        .expect("layout resolves against registry");
    assert_eq!(layout.world_seed, 424242);
    assert!(layout.instances.len() >= 12);
    assert!(layout.connections.len() >= 10);
}

#[test]
fn interaction_regions_generate_distinct_staging_slots() {
    let registry = WorldModuleRegistry::load(project_root().join("assets/world/registry.json"))
        .expect("load committed module registry");
    let module = registry
        .modules
        .iter()
        .find(|module| module.module_id == "apartment_elevator_lobby_a")
        .expect("elevator module");
    let slots = module
        .generate_staging_slots("INTERACT_ELEVATOR_PANEL", 4, 1.0)
        .expect("generate slots around panel");
    assert_eq!(slots.len(), 4);
    for (index, a) in slots.iter().enumerate() {
        for b in slots.iter().skip(index + 1) {
            let distance = ((a.position[0] - b.position[0]).powi(2)
                + (a.position[2] - b.position[2]).powi(2))
            .sqrt();
            assert!(distance >= 0.8, "slots overlap: {a:?} {b:?}");
        }
    }
}

#[test]
fn generated_motion_batch_is_loadable_but_not_automatically_approved() {
    let library = MotionLibrary::scan(&project_root().join("assets/animations/library"))
        .expect("scan processed motion library");
    assert!(!library.pending("confident_walk").is_empty());
    assert!(library.approved("confident_walk").is_empty());
    assert!(!library.pending("move_between").is_empty());
}
