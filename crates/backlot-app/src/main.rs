//! Infinite Backlot — local-first autonomous 3D content world.
//!
//! Entry point: builds configuration, the persistent world, and the episode
//! author (an OpenAI-compatible LLM director with deterministic fallback), then
//! runs the Bevy application state machine described in the PRD.

mod bevy_capture;
mod pipeline;
mod player;
mod scene;
mod state;

use backlot_core::author::{DeterministicAuthor, EpisodeAuthor};
use backlot_core::config::Config;
use backlot_core::director::DirectorContext;
use backlot_core::render::{produce_episode, ProduceConfig};
use backlot_core::world::build_default_world;
use backlot_llm::{LlmAuthor, LlmMetrics};
use bevy::prelude::*;
use bevy::window::{Window, WindowPlugin, WindowResolution};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

use crate::scene::Hud;
use state::*;

fn main() {
    let config = Config::load_or_default("data/config.toml");
    println!(
        "Infinite Backlot — base_url={} model={} force_fallback={}",
        config.llm.base_url, config.llm.model, config.director.force_fallback
    );

    let world = backlot_core::world::WorldState::load_initial("data/world/initial.json")
        .unwrap_or_else(|_| build_default_world());

    // --- Offline one-shot producer (no Bevy / no GPU / no window) ---
    let cli_args: Vec<String> = std::env::args().collect();
    let produce_one = cli_args.iter().any(|a| a == "--produce-one");
    let require_llm = cli_args.iter().any(|a| a == "--require-llm");
    let reuse_authored = cli_args.iter().any(|a| a == "--reuse-authored");
    let render_backend = cli_args
        .iter()
        .position(|a| a == "--render-backend")
        .and_then(|i| cli_args.get(i + 1).cloned())
        .unwrap_or_else(|| "cpu".to_string());
    // --- Authoring-only diagnostic (no rendering) ---
    if cli_args.iter().any(|a| a == "--diagnostic-authoring") {
        eprintln!("AUTHORING DIAGNOSTIC (no render)");
        let mut dir = config.director.clone();
        dir.require_llm = true;
        let out_dir = PathBuf::from("diagnostics/llm_authoring_packet");
        let author = match LlmAuthor::new_diagnostic(&config, dir, out_dir.clone()) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("diagnostic init failed: {e}");
                std::process::exit(2);
            }
        };
        let ctx = DirectorContext {
            world: world.clone(),
            episode_number: 1,
            seed: config.runtime.base_seed,
            target_duration: config.runtime.target_duration_secs,
            recent_summaries: vec![],
            tone: vec!["surreal".into(), "comedy".into()],
        };
        match author
            .runtime()
            .block_on(author.author_diagnostic(&ctx, &out_dir))
        {
            Ok(s) => {
                println!("DIAGNOSTIC COMPLETE");
                println!(
                    "  packet: {}",
                    out_dir.join("LLM_AUTHORING_DIAGNOSTIC_PACKET.md").display()
                );
                println!(
                    "  total_wire_calls={} total_logical_calls={}",
                    s.total_wire_calls, s.total_logical_calls
                );
                println!(
                    "  wall_ms={} prompt_tokens={} completion_tokens={}",
                    s.total_wall_ms, s.prompt_tokens, s.completion_tokens
                );
                println!(
                    "  produced={} estimated_duration={:?} status={}",
                    s.produced, s.estimated_duration_secs, s.duration_status
                );
                println!(
                    "  finish_reasons={:?} any_length_truncated={}",
                    s.finish_reasons, s.any_length_truncated
                );
                println!(
                    "  schema_repairs={} plan_title={:?} beats={}",
                    s.schema_repairs, s.plan_title, s.beat_count
                );
            }
            Err(e) => {
                eprintln!("DIAGNOSTIC FAILED: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    if produce_one {
        let author: Box<dyn EpisodeAuthor> = if require_llm {
            // CLI flag overrides the config file: in REQUIRE-LLM production mode
            // any LLM failure must be fatal (never silently fall back to the
            // deterministic director). We propagate the flag into the author so
            // `author()` returns an error instead of a fallback plan.
            let mut dir = config.director.clone();
            dir.require_llm = true;
            match LlmAuthor::new(&config, dir) {
                Ok(mut a) => {
                    if reuse_authored {
                        a.set_reuse_path(PathBuf::from("data/last_authored_episode.json"));
                    }
                    Box::new(a)
                }
                Err(e) => {
                    eprintln!("REQUIRE-LLM mode: LLM client could not be initialized: {e}");
                    std::process::exit(2);
                }
            }
        } else if config.director.force_fallback {
            Box::new(DeterministicAuthor)
        } else {
            match LlmAuthor::new(&config, config.director.clone()) {
                Ok(mut a) => {
                    if reuse_authored {
                        a.set_reuse_path(PathBuf::from("data/last_authored_episode.json"));
                    }
                    Box::new(a)
                }
                Err(_) => Box::new(DeterministicAuthor),
            }
        };
        let cfg = ProduceConfig {
            config: config.clone(),
            require_llm,
            world: world.clone(),
            seed: config.runtime.base_seed,
            episode_number: 1,
            keep_frames: true,
        };
        let report_res: backlot_core::error::Result<backlot_core::render::ProduceReport> =
            if render_backend == "bevy" {
                eprintln!("RENDER BACKEND: bevy (real GPU scene)");
                bevy_capture::produce_episode_bevy(cfg, author)
            } else {
                produce_episode(cfg, author)
            };
        match report_res {
            Ok(report) => {
                println!("PRODUCED {}", report.episode_id);
                println!("  captioned  : {}", report.mp4_captioned);
                println!("  clean      : {}", report.mp4_clean);
                println!("  duration   : {:.1}s", report.duration_secs);
                println!("  frames     : {}", report.frames);
                println!("  require_llm: {}", report.require_llm);
                println!("  plan_src   : {}", report.plan_author_source);
                println!("  llm_used   : {}", report.llm_used);
                println!(
                    "  tts        : {} (real={})",
                    report.tts_provider, report.tts_real
                );
                println!("  mp4_ok     : {}", report.mp4_produced);
                println!("  ffprobe_ok : {}", report.ffprobe_ok);
                if !report.issues.is_empty() {
                    println!("  issues     : {:?}", report.issues);
                }
                if !report.mp4_produced {
                    std::process::exit(3);
                }
                return;
            }
            Err(e) => {
                eprintln!("PRODUCTION FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    // Build the author. Prefer the LLM director; fall back to deterministic.
    let mut using_llm = false;
    let mut metrics: Option<Arc<Mutex<LlmMetrics>>> = None;
    let author: Arc<dyn EpisodeAuthor + Send + Sync> = if config.director.force_fallback {
        Arc::new(DeterministicAuthor)
    } else {
        match LlmAuthor::new(&config, config.director.clone()) {
            Ok(a) => {
                using_llm = true;
                metrics = Some(a.metrics_arc());
                Arc::new(a)
            }
            Err(e) => {
                tracing::warn!("LLM client init failed ({e}); using deterministic director only");
                Arc::new(DeterministicAuthor)
            }
        }
    };

    // Author worker thread: Bevy's main thread never blocks on an LLM request.
    let (req_tx, req_rx) = mpsc::channel::<DirectorContextMsg>();
    let (resp_tx, resp_rx) = mpsc::channel::<AuthorMsg>();
    let author_thread = author.clone();
    std::thread::spawn(move || {
        for msg in req_rx {
            let ctx = msg.to_context();
            let res = author_thread.author(&ctx);
            let (planned, auth) = match res {
                Ok((p, a)) => (Ok(p), Some(a)),
                Err(e) => (Err(e.to_string()), None),
            };
            let _ = resp_tx.send(AuthorMsg {
                planned,
                auth,
                metrics: None,
            });
        }
    });

    let handle = AuthorHandle {
        tx: req_tx,
        rx: Arc::new(Mutex::new(resp_rx)),
        pending: false,
        metrics,
        using_llm,
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Infinite Backlot".into(),
            resolution: WindowResolution::new(540, 960), // vertical master framing
            ..default()
        }),
        ..default()
    }))
    .insert_resource(RunControl::from(&config))
    .insert_resource(CanonicalWorld(world))
    .insert_resource(handle)
    .insert_resource(CurrentEpisode::default())
    .insert_resource(Player::default())
    .insert_resource(EpisodeClock::default())
    .insert_resource(RehearsalLog::default())
    .insert_resource(ActiveCaption::default())
    .insert_resource(SceneIndex::default())
    .insert_resource(Hud::default())
    .insert_resource(CurrentMetrics::default())
    .init_state::<AppState>();

    // State transitions.
    app.add_systems(OnEnter(AppState::Boot), boot_to_loading)
        .add_systems(
            OnEnter(AppState::AssetLoading),
            pipeline::asset_loading_system,
        )
        .add_systems(
            Update,
            pipeline::idle_to_selecting.run_if(in_state(AppState::Idle)),
        )
        .add_systems(
            OnEnter(AppState::EpisodeSelecting),
            pipeline::episode_selecting_system,
        )
        .add_systems(
            OnEnter(AppState::EpisodePlanning),
            pipeline::request_plan_system,
        )
        .add_systems(
            Update,
            pipeline::poll_plan_system.run_if(in_state(AppState::EpisodePlanning)),
        )
        .add_systems(
            OnEnter(AppState::PlanValidation),
            pipeline::plan_validation_system,
        )
        .add_systems(
            OnEnter(AppState::ErrorRecovery),
            pipeline::error_recovery_system,
        )
        .add_systems(
            OnEnter(AppState::Rehearsing),
            pipeline::start_rehearsal_system,
        )
        .add_systems(
            OnEnter(AppState::EpisodeReady),
            pipeline::episode_ready_system,
        )
        .add_systems(OnEnter(AppState::Rendering), pipeline::start_render_system)
        .add_systems(OnEnter(AppState::Committing), pipeline::commit_system)
        .add_systems(OnEnter(AppState::Reviewing), pipeline::review_enter_system)
        .add_systems(
            Update,
            pipeline::review_input_system.run_if(in_state(AppState::Reviewing)),
        );

    // Continuous simulation systems (rehearsal + render passes).
    app.add_systems(
        Update,
        (
            player::player_system,
            player::navigation_system,
            player::camera_system,
            player::flicker_system,
            player::hud_system,
        )
            .run_if(in_state(AppState::Rehearsing).or_eager(in_state(AppState::Rendering))),
    );

    app.run();
}

fn boot_to_loading(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::AssetLoading);
}
