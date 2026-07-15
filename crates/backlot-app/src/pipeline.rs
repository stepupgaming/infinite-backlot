//! Production pipeline: orchestrates the application state machine and turns a
//! planned episode into a committed, packaged artifact.

use crate::scene::Hud;
use crate::state::*;
use backlot_core::author::{AuthorSource, DeterministicAuthor, EpisodeAuthor, PlannedEpisode};
use backlot_core::package::{Diagnostics, EpisodeMetrics, EpisodePackage, GemmyManifest};
use backlot_core::serial_id;
use backlot_core::story::apply_persistent_changes;
use backlot_core::validation::{validate_beat_command, validate_plan, ValidatedPlan};
use backlot_core::world::WorldState;
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Boot / asset loading
// ---------------------------------------------------------------------------

pub fn asset_loading_system(
    commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    world: Res<CanonicalWorld>,
    scene: ResMut<SceneIndex>,
    hud: ResMut<Hud>,
    set_mode: Res<crate::backlot_scene::BacklotSetMode>,
    runtime: Res<crate::backlot_scene::BacklotSceneRuntime>,
    manifest: Option<Res<crate::backlot_scene::BacklotSceneManifest>>,
    mut next: ResMut<NextState<AppState>>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    match &runtime.status {
        crate::backlot_scene::BacklotLoadStatus::Loading => return,
        crate::backlot_scene::BacklotLoadStatus::Failed(message) => {
            panic!("backlot set contract failed after GLB instantiation: {message}");
        }
        crate::backlot_scene::BacklotLoadStatus::Ready => {}
    }
    let mut scene = scene;
    if let Some(manifest) = manifest {
        crate::backlot_scene::populate_scene_index(&mut scene, &manifest, &runtime);
    }
    crate::scene::spawn_scene(commands, meshes, materials, world, scene, hud, set_mode);
    *spawned = true;
    next.set(AppState::Idle);
    tracing::info!("scene loaded — entering idle");
}

// ---------------------------------------------------------------------------
// Episode selection + planning request
// ---------------------------------------------------------------------------

pub fn idle_to_selecting(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::EpisodeSelecting);
}

pub fn episode_selecting_system(
    mut current: ResMut<CurrentEpisode>,
    mut run: ResMut<RunControl>,
    world: Res<CanonicalWorld>,
    mut next: ResMut<NextState<AppState>>,
) {
    run.replaying = false;
    current.episode_number = run.episodes_done as u64 + 1;
    current.episode_id = serial_id("episode", current.episode_number, 6);
    current.world_before = Some(world.0.clone());
    current.planned = None;
    current.validated = None;
    current.approved = true;
    let summary = format!(
        "[{:03}] planning '{}' (episodes done: {})",
        current.episode_number,
        world
            .0
            .threads
            .values()
            .next()
            .map(|t| t.summary.clone())
            .unwrap_or_default(),
        run.episodes_done
    );
    tracing::info!("{summary}");
    next.set(AppState::EpisodePlanning);
}

pub fn request_plan_system(
    mut handle: ResMut<AuthorHandle>,
    current: Res<CurrentEpisode>,
    run: Res<RunControl>,
    world: Res<CanonicalWorld>,
) {
    if handle.pending {
        return;
    }
    let msg = DirectorContextMsg {
        world: world.0.clone(),
        episode_number: current.episode_number,
        seed: run
            .config
            .runtime
            .base_seed
            .wrapping_add(current.episode_number * 2654435761),
        target_duration: run.config.runtime.target_duration_secs,
        recent_summaries: run.recent_summaries.clone(),
        tone: vec!["surreal".into(), "comedy".into()],
    };
    if handle.tx.send(msg).is_ok() {
        handle.pending = true;
        println!(
            "▶ Requesting episode plan from {} director…",
            if handle.using_llm {
                "LLM"
            } else {
                "deterministic"
            }
        );
    }
}

pub fn poll_plan_system(
    mut handle: ResMut<AuthorHandle>,
    mut current: ResMut<CurrentEpisode>,
    world: Res<CanonicalWorld>,
    mut next: ResMut<NextState<AppState>>,
) {
    let msg = {
        let rx = handle.rx.lock().unwrap();
        match rx.try_recv() {
            Ok(m) => m,
            Err(_) => return,
        }
    };
    handle.pending = false;

    let planned = match msg.planned {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("author failed: {e}; entering error recovery");
            next.set(AppState::ErrorRecovery);
            return;
        }
    };
    match build_validated(&world.0, &planned) {
        Some(v) => {
            current.planned = Some(planned);
            current.validated = Some(v);
            current.auth = msg.auth;
            next.set(AppState::PlanValidation);
        }
        None => {
            tracing::error!("plan validation failed; entering error recovery");
            next.set(AppState::ErrorRecovery);
        }
    }
}

/// Validate a planned episode into a `ValidatedPlan`, resolving each beat.
fn build_validated(world: &WorldState, planned: &PlannedEpisode) -> Option<ValidatedPlan> {
    let vplan = validate_plan(world, &planned.plan).ok()?;
    let mut resolved = vplan.resolved_beats;
    for (i, beat) in planned.plan.beats.iter().enumerate() {
        if let Some(cmd) = planned.commands.get(&beat.id) {
            if let Ok(rb) = validate_beat_command(world, &planned.plan, cmd) {
                if i < resolved.len() {
                    resolved[i] = rb;
                }
            }
        }
    }
    Some(ValidatedPlan {
        plan: planned.plan.clone(),
        resolved_beats: resolved,
    })
}

// ---------------------------------------------------------------------------
// Validation gate + error recovery
// ---------------------------------------------------------------------------

pub fn plan_validation_system(current: Res<CurrentEpisode>, mut next: ResMut<NextState<AppState>>) {
    if current.validated.is_some() && current.planned.is_some() {
        next.set(AppState::Rehearsing);
    } else {
        next.set(AppState::ErrorRecovery);
    }
}

pub fn error_recovery_system(
    mut current: ResMut<CurrentEpisode>,
    world: Res<CanonicalWorld>,
    mut next: ResMut<NextState<AppState>>,
) {
    tracing::warn!("error recovery: falling back to deterministic director");
    let ctx = backlot_core::director::DirectorContext {
        world: world.0.clone(),
        episode_number: current.episode_number,
        seed: 0xDEAD,
        target_duration: 60.0,
        recent_summaries: vec![],
        tone: vec![],
    };
    match DeterministicAuthor.author(&ctx) {
        Ok((planned, auth)) => {
            if let Some(v) = build_validated(&world.0, &planned) {
                current.planned = Some(planned);
                current.auth = Some(auth);
                current.validated = Some(v);
                next.set(AppState::Rehearsing);
                return;
            }
        }
        Err(e) => tracing::error!("deterministic fallback also failed: {e}"),
    }
    // Last resort: cannot proceed; idle and retry later.
    next.set(AppState::Idle);
}

// ---------------------------------------------------------------------------
// Rehearsal + render passes
// ---------------------------------------------------------------------------

pub fn start_rehearsal_system(
    mut player: ResMut<Player>,
    mut clock: ResMut<EpisodeClock>,
    mut log: ResMut<RehearsalLog>,
) {
    player.active = true;
    player.render_pass = false;
    player.beat_index = 0;
    player.initialized_beat = None;
    player.beat_elapsed = 0.0;
    player.action_cursor = 0;
    player.finished = false;
    player.since_event = 0.0;
    player.quality = 1.0;
    clock.elapsed = 0.0;
    clock.scale = 1.0;
    log.events.clear();
    log.dialogue.clear();
    log.captions.clear();
    log.camera.clear();
    log.hook_time = None;
    log.objective_time = None;
    log.dead_air_max = 0.0;
    log.visual_changes = 0;
    log.story_changes = 0;
    log.repairs = 0;
    log.validation_errors.clear();
    println!("● Rehearsing episode…");
}

pub fn start_render_system(mut player: ResMut<Player>, mut clock: ResMut<EpisodeClock>) {
    player.active = true;
    player.render_pass = true;
    player.beat_index = 0;
    player.initialized_beat = None;
    player.beat_elapsed = 0.0;
    player.action_cursor = 0;
    player.finished = false;
    player.since_event = 0.0;
    player.quality = 2.0; // "higher quality" pass
    clock.elapsed = 0.0;
    clock.scale = 1.0;
    println!("◎ Rendering (deterministic replay, no LLM)…");
}

pub fn episode_ready_system(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::Rendering);
}

// ---------------------------------------------------------------------------
// Commit + package
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn commit_system(
    mut run: ResMut<RunControl>,
    mut world: ResMut<CanonicalWorld>,
    mut current: ResMut<CurrentEpisode>,
    log: Res<RehearsalLog>,
    handle: Res<AuthorHandle>,
    mut next: ResMut<NextState<AppState>>,
) {
    let planned = match &current.planned {
        Some(p) => p,
        None => {
            next.set(AppState::Idle);
            return;
        }
    };
    let plan = planned.plan.clone();

    // Apply persistent changes to a working copy of the world.
    let mut after = current
        .world_before
        .clone()
        .unwrap_or_else(WorldState::default);
    let delta = apply_persistent_changes(&mut after, &plan.persistent_changes);
    let _ = &delta;

    current.world_after = Some(after.clone());

    // Metrics.
    let dur = plan.target_duration_seconds.max(1.0);
    let mut m = EpisodeMetrics::default();
    m.hook_latency_secs = log.hook_time.unwrap_or(dur);
    m.objective_understandable_secs = log.objective_time.unwrap_or(dur);
    m.dead_air_secs = log.dead_air_max;
    m.avg_shot_duration = if log.camera.is_empty() {
        0.0
    } else {
        log.camera.iter().map(|c| c.end - c.start).sum::<f32>() / log.camera.len() as f32
    };
    m.longest_shot_duration = log
        .camera
        .iter()
        .map(|c| c.end - c.start)
        .fold(0.0_f32, f32::max);
    m.visual_changes_per_min = log.visual_changes as f32 / (dur / 60.0);
    m.story_changes_per_min = log.story_changes as f32 / (dur / 60.0);
    m.deterministic_repairs = log.repairs;
    m.payoff_complete = !plan.payoff.trim().is_empty();
    m.persistent_consequence = !plan.persistent_changes.is_empty();
    m.model_validation_failures = handle
        .metrics
        .as_ref()
        .map(|mt| mt.lock().unwrap().schema_repairs)
        .unwrap_or(0);

    // Truthful director/author-source resolution (never claim LLM on fallback).
    let (director_name, plan_source, llm_used, llm_reqs, llm_fails) = match &current.auth {
        Some(a) => {
            let label = if a.all_llm() {
                "llm"
            } else if a.plan_source == AuthorSource::Deterministic {
                "deterministic"
            } else {
                "deterministic_fallback"
            };
            let used = a.plan_source == AuthorSource::Llm
                || a.beats.iter().any(|b| b.source == AuthorSource::Llm);
            let reqs = a.attempts + a.beats.iter().map(|b| b.attempts).sum::<u32>();
            let fails = a
                .beats
                .iter()
                .filter(|b| b.source == AuthorSource::DeterministicFallback)
                .count() as u32;
            (
                label.to_string(),
                a.plan_source.as_str().to_string(),
                used,
                reqs,
                fails,
            )
        }
        None => {
            let l = if handle.using_llm {
                "llm"
            } else {
                "deterministic"
            };
            let reqs = handle
                .metrics
                .as_ref()
                .map(|mt| mt.lock().unwrap().requests)
                .unwrap_or(0);
            let fails = handle
                .metrics
                .as_ref()
                .map(|mt| mt.lock().unwrap().failures)
                .unwrap_or(0);
            (l.to_string(), l.to_string(), handle.using_llm, reqs, fails)
        }
    };

    let transcript: String = log
        .dialogue
        .iter()
        .map(|d| format!("{}: {}", d.actor, d.text))
        .collect::<Vec<_>>()
        .join("\n");

    let gemmy = GemmyManifest {
        title: plan.episode_title.clone(),
        summary: plan.logline.clone(),
        hook_text: log
            .captions
            .first()
            .map(|c| c.text.clone())
            .unwrap_or_default(),
        duration_secs: dur,
        characters: plan.active_characters.clone(),
        transcript: transcript.clone(),
        caption_style: run.config.runtime.caption_style.clone(),
        render_paths: vec![
            "output/vertical_captioned.mp4".into(),
            "output/vertical_clean.mp4".into(),
        ],
        thumbnail_candidates: vec!["output/thumbnail_01.png".into()],
        story_tags: plan.tone.clone(),
        quality_scores: Default::default(),
        detected_issues: vec![],
        canonical: current.approved,
        suggested_posting_caption: format!("{} #shorts", plan.episode_title),
        suggested_compilation_category: "surreal-comedy".into(),
    };

    let mut pkg = EpisodePackage {
        id: current.episode_id.clone(),
        title: plan.episode_title.clone(),
        logline: plan.logline.clone(),
        duration_secs: dur,
        canonical: current.approved,
        plan: plan.clone(),
        world_before: current.world_before.clone().unwrap_or_default(),
        world_after: after.clone(),
        events: log.events.clone(),
        dialogue: log.dialogue.clone(),
        captions: log.captions.clone(),
        camera_plan: log.camera.clone(),
        metrics: m.clone(),
        diagnostics: Diagnostics {
            episode_id: current.episode_id.clone(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            director: director_name.clone(),
            llm_requests: llm_reqs,
            llm_failures: llm_fails,
            validation_errors: log.validation_errors.clone(),
            repairs: log.repairs,
            metrics: m.clone(),
            issues: vec![],
            require_llm: run.config.director.require_llm,
            llm_used,
            plan_author_source: plan_source.clone(),
            authorship: current.auth.clone(),
            tts_provider: "estimating".into(),
            tts_provenance: None,
            tts_real: false,
            audio_real: false,
            frames_captured: run.config.runtime.capture_frames,
            mp4_produced: false,
            ffmpeg_command: None,
            ffprobe_ok: false,
            replay_no_llm: true,
            render_backend: "bevy".into(),
            timing: None,
        },
        gemmy,
        report_md: String::new(),
    };
    pkg.build_report();

    let out_dir = &run.config.runtime.output_dir;
    match pkg.write(out_dir) {
        Ok(_) => {
            println!(
                "✓ Episode {} committed → {}/episodes/{}/",
                current.episode_id, out_dir, current.episode_id
            );
            println!("  title: {}", plan.episode_title);
            println!("  payoff: {}", plan.payoff);
            println!("  persistent changes: {}", plan.persistent_changes.len());
        }
        Err(e) => tracing::error!("package write failed: {e}"),
    }

    // Persist canonical world for the next episode.
    world.0 = after;

    run.episodes_done += 1;
    run.last_summary = Some(plan.logline.clone());
    run.recent_summaries.push(plan.logline.clone());
    if run.recent_summaries.len() > 8 {
        run.recent_summaries.remove(0);
    }

    next.set(AppState::Reviewing);
}

// ---------------------------------------------------------------------------
// Review
// ---------------------------------------------------------------------------

pub fn review_enter_system(
    current: Res<CurrentEpisode>,
    run: Res<RunControl>,
    log: Res<RehearsalLog>,
) {
    if let Some(p) = &current.planned {
        println!(
            "\n=== REVIEW {:03} | {} | {}s | beats={} | dead-air={:.1}s | repairs={} ===",
            current.episode_number,
            p.plan.episode_title,
            p.plan.target_duration_seconds as u32,
            p.plan.beats.len(),
            log.dead_air_max,
            log.repairs,
        );
        println!("  [N] next episode   [R] replay render   [Q] quit\n");
    }
    let _ = &run;
}

pub fn review_input_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<RunControl>,
    current: ResMut<CurrentEpisode>,
    mut next: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
    world: Res<CanonicalWorld>,
    handle: Res<AuthorHandle>,
) {
    if keys.just_pressed(KeyCode::KeyQ) {
        next.set(AppState::Shutdown);
        exit.write(AppExit::Success);
        return;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        // Re-run the render pass of the last committed episode (no new LLM, no commit).
        if current.planned.is_some() && current.validated.is_some() {
            run.replaying = true;
            next.set(AppState::Rendering);
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyN) || run.auto {
        // Continue to the next episode, honoring the configured run length.
        if run.episodes_to_run == 0 || run.episodes_done < run.episodes_to_run {
            next.set(AppState::EpisodeSelecting);
        } else {
            println!(
                "✓ Reached configured episode limit ({}).",
                run.episodes_to_run
            );
            next.set(AppState::Shutdown);
            exit.write(AppExit::Success);
        }
        let _ = (&world, &handle);
    }
}
