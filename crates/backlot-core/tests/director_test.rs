//! Smoke test: deterministic director must produce a fully valid, executable
//! episode plan + beat commands against the default world.

use backlot_core::director::{DeterministicDirector, Director, DirectorContext};
use backlot_core::validation::validate_beat_command;
use backlot_core::world::build_default_world;

#[test]
fn deterministic_episode_is_valid_and_executable() {
    let world = build_default_world();
    let dir = DeterministicDirector;
    let ctx = DirectorContext {
        world: world.clone(),
        episode_number: 1,
        seed: 0xC0FFEE,
        target_duration: 75.0,
        recent_summaries: vec![],
        tone: vec!["surreal".into()],
    };

    let plan = dir.plan_episode(&ctx).expect("plan");
    assert!(!plan.beats.is_empty(), "plan must have beats");
    assert!(!plan.payoff.trim().is_empty(), "payoff required");

    // Every active character must exist, and the primary location too.
    for c in &plan.active_characters {
        assert!(world.character(c).is_some(), "unknown active char {c}");
    }
    assert!(world.location(&plan.primary_location).is_some());

    for beat in &plan.beats {
        let cmd = dir
            .plan_beat(&ctx, &plan, beat)
            .unwrap_or_else(|e| panic!("beat {} failed: {e:?}", beat.id));
        let resolved = validate_beat_command(&world, &plan, &cmd)
            .unwrap_or_else(|e| panic!("validate beat {} failed: {e:?}", beat.id));
        // At least the hook beat must contain a spoken line (objective clarity).
        let has_speak = resolved
            .resolved_actions
            .iter()
            .any(|a| a.action == "speak" && a.text.is_some());
        if beat.beat_type == "hook" {
            assert!(has_speak, "hook should establish spoken objective");
        }
    }
}

#[test]
fn deterministic_episode_is_reproducible() {
    let world = build_default_world();
    let dir = DeterministicDirector;
    let ctx = || DirectorContext {
        world: world.clone(),
        episode_number: 7,
        seed: 0xABCD,
        target_duration: 60.0,
        recent_summaries: vec![],
        tone: vec![],
    };
    let a = dir.plan_episode(&ctx()).unwrap();
    let b = dir.plan_episode(&ctx()).unwrap();
    assert_eq!(a.episode_title, b.episode_title);
    assert_eq!(a.beats.len(), b.beats.len());
}
