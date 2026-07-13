//! Headless end-to-end check of the autonomous episode pipeline.
//!
//! This exercises everything the Bevy app would, minus the renderer: the
//! director authors a structured plan, it is validated + each beat resolved,
//! persistent changes are applied to a working world copy, and the result is
//! committed as a machine-readable episode package to disk.

use backlot_core::author::{DeterministicAuthor, EpisodeAuthor, PlannedEpisode};
use backlot_core::director::DirectorContext;
use backlot_core::package::{empty_package, EpisodePackage};
use backlot_core::story::apply_persistent_changes;
use backlot_core::validation::{validate_beat_command, validate_plan};
use backlot_core::world::build_default_world;
use std::fs;

fn author_episode(seed: u64, episode_number: u64) -> PlannedEpisode {
    let world = build_default_world();
    let director = DeterministicAuthor;
    let ctx = DirectorContext {
        world: world.clone(),
        episode_number,
        seed,
        target_duration: 70.0,
        recent_summaries: vec![],
        tone: vec!["surreal".into(), "comedy".into()],
    };
    let (planned, _auth) = director
        .author(&ctx)
        .expect("deterministic director authors an episode");
    planned
}

#[test]
fn deterministic_plan_is_valid_and_executable() {
    let world = build_default_world();
    let planned = author_episode(0xC0FFEE, 1);

    assert!(!planned.plan.beats.is_empty(), "episode must have beats");
    assert!(
        !planned.plan.active_characters.is_empty(),
        "episode must name active characters"
    );

    let vplan = validate_plan(&world, &planned.plan).expect("plan validates");
    assert_eq!(vplan.resolved_beats.len(), planned.plan.beats.len());

    // Every beat command must resolve against the world.
    for beat in &planned.plan.beats {
        let cmd = planned
            .commands
            .get(&beat.id)
            .unwrap_or_else(|| panic!("missing command for beat {}", beat.id));
        let rb = validate_beat_command(&world, &planned.plan, cmd)
            .unwrap_or_else(|e| panic!("beat {} failed resolution: {:?}", beat.id, e));
        assert!(
            !rb.resolved_actions.is_empty(),
            "beat {} must resolve to at least one action",
            beat.id
        );
    }
}

#[test]
fn same_seed_is_reproducible() {
    let a = author_episode(0xC0FFEE, 3);
    let b = author_episode(0xC0FFEE, 3);
    assert_eq!(
        serde_json::to_string(&a.plan).unwrap(),
        serde_json::to_string(&b.plan).unwrap(),
        "same seed + episode number must be deterministic"
    );
}

#[test]
fn pipeline_commits_a_package_to_disk() {
    let world = build_default_world();
    let planned = author_episode(0xC0FFEE, 1);
    let vplan = validate_plan(&world, &planned.plan).expect("plan validates");

    // Resolve every beat command into the validated plan.
    let mut resolved = vplan.resolved_beats;
    for beat in &planned.plan.beats {
        if let Some(cmd) = planned.commands.get(&beat.id) {
            if let Ok(rb) = validate_beat_command(&world, &planned.plan, cmd) {
                if let Some(slot) = resolved.iter_mut().find(|r| r.outline.id == beat.id) {
                    *slot = rb;
                }
            }
        }
    }
    let _ = &resolved;

    // Apply the episode's persistent changes to a working world copy.
    let mut after = world.clone();
    let _delta = apply_persistent_changes(&mut after, &planned.plan.persistent_changes);

    // Commit it as a package.
    let id = "episode000001";
    let dir = std::env::temp_dir().join("backlot_pipeline_test");
    let _ = fs::remove_dir_all(&dir);
    let mut pkg: EpisodePackage = empty_package(id, &planned.plan, &world);
    pkg.world_after = after;
    pkg.build_report();
    assert!(
        !pkg.report_md.is_empty(),
        "report markdown must be generated"
    );
    pkg.write(dir.to_str().unwrap())
        .expect("package writes to disk");

    let base = dir.join("episodes").join(id);
    for name in [
        "episode.json",
        "plan.json",
        "world_before.json",
        "world_after.json",
        "events.jsonl",
        "dialogue.json",
        "captions.json",
        "camera_plan.json",
        "render_manifest.json",
        "diagnostics.json",
        "gemmy_manifest.json",
        "report.md",
    ] {
        let p = base.join(name);
        assert!(p.exists(), "expected artifact {name} to be written");
    }

    let _ = fs::remove_dir_all(&dir);
}
