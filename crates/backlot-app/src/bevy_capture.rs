//! Headless Bevy rendering path for episode production.
//!
//! This module renders the *authoritative* Bevy scene — a real GPU scene with
//! articulated humanoid rigs — to an offscreen image and reads each frame back
//! via Bevy's official `Screenshot` readback, producing the frame sequence that
//! `finalize_production` muxes into the vertical MP4.
//!
//! It shares exactly one world state with the CPU path: `prepare_production`
//! returns the same `Schedule`/`rigs`, and `evaluate_at` is the single source of
//! per-frame truth for both renderers. The only difference is *how* the frame is
//! drawn (Bevy GPU vs software rasterizer).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use bevy::asset::{AssetApp, RenderAssetUsages};
use bevy::camera::{ClearColorConfig, PerspectiveProjection, RenderTarget};
use bevy::render::camera::CameraRenderGraph;
use bevy::core_pipeline::Core3d;
use bevy::ecs::error::{BevyError, ErrorContext, FallbackErrorHandler, match_severity};
use bevy::ecs::observer::On;
use bevy::log::warn;
use bevy::math::Vec4;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::renderer::RenderDevice;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::WindowPlugin;

use backlot_core::author::EpisodeAuthor;
use backlot_core::avatar::{HumanoidRig, SemanticJoint};
use backlot_core::render::{finalize_production, prepare_production, write_png, ProduceConfig, ProduceReport};
use backlot_core::timeline::{evaluate_at, Schedule};
use backlot_core::world::WorldState;

/// Which primitive a body part is rendered as.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PartShape {
    Capsule,
    Sphere,
}

/// A body-part mesh bound to a semantic joint of a character.
#[derive(Component)]
struct RigPartTag {
    char_id: String,
    joint: SemanticJoint,
    shape: PartShape,
}

/// A prop mesh bound to a world prop id.
#[derive(Component)]
struct PropTag {
    prop_id: String,
}

/// Marker for the per-frame screenshot entity we spawn to trigger readback.
#[derive(Component)]
struct CaptureMarker;

/// Shared production plan + render target for the capture loop.
#[derive(Resource)]
struct CapturePlan {
    schedule: Schedule,
    rigs: HashMap<String, HumanoidRig>,
    world: WorldState,
    fps: u32,
    n_frames: u32,
    frames_dir: PathBuf,
    capture_image: Handle<Image>,
}

#[derive(Resource, Default)]
struct CaptureProgress {
    requested: u32,
    captured: u32,
    queue: VecDeque<u32>,
}

/// Produce one episode by rendering the real Bevy scene headless.
pub fn produce_episode_bevy(
    cfg: ProduceConfig,
    author: Box<dyn EpisodeAuthor>,
) -> backlot_core::error::Result<ProduceReport> {
    use backlot_core::package::TimingReport;
    let total_start = std::time::Instant::now();
    let started_at = chrono::Utc::now().to_rfc3339();
    let ProduceConfig { config, require_llm, world, seed, episode_number, .. } = cfg;
    let out_dir = config.runtime.output_dir.clone();
    let ep_dir = Path::new(&out_dir).join("episodes").join(backlot_core::serial_id("episode", episode_number, 6));
    let frames_dir = ep_dir.join("frames");
    std::fs::create_dir_all(&frames_dir).map_err(io_err(&frames_dir))?;

    // Stage 1: shared authoring/validation/TTS/schedule/rigs.
    let prep = prepare_production(&config, require_llm, &world, seed, episode_number, &*author)?;

    let fps = config.runtime.frame_rate.max(1);
    // Render at half vertical-master resolution; the encoder upscales to 1080x1920.
    let (rw, rh) = (
        config.runtime.resolution.0 / 2,
        config.runtime.resolution.1 / 2,
    );
    let n_frames = (prep.schedule.duration * fps as f32).ceil() as u32;

    let mut app = App::new();
    // Use the real GPU. On this machine Bevy auto-selects the discrete
    // NVIDIA adapter (Vulkan/DX12); we must NOT force the WARP software
    // fallback adapter, because a CPU software rasterizer is not an acceptable
    // production renderer for this pipeline. The offscreen `RenderTarget::Image`
    // is what we actually capture; the primary window only exists so Bevy can
    // initialize the `RenderDevice`. We keep it hidden so production runs
    // headlessly without popping a visible window.
    app.add_plugins(
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                visible: false,
                ..default()
            }),
            ..default()
        }),
    )
    .insert_resource(ClearColor(Color::srgb(0.03, 0.03, 0.05)));

    // Targeted error handler: tolerate ONLY the transient "Resource does not
    // exist" validation errors that fire on the first `update()` calls while the
    // GPU `RenderDevice` is still being unpacked into the main world. Every other
    // error (including real panics) defers to `match_severity` and aborts loudly.
    app.insert_resource(FallbackErrorHandler(tolerate_startup_resource_error));

    // Ensure skinned-mesh inverse-bindpose assets are registered so the PBR
    // skin extraction system has its `Assets<SkinnedMeshInverseBindposes>`
    // resource present (it is otherwise only registered by loaders like glTF).
    app.init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>();

    // Create the offscreen capture image inside the app's asset store.
    let cap_handle: Handle<Image> = {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let mut img = Image::new(
            Extent3d { width: rw, height: rh, depth_or_array_layers: 1 },
            TextureDimension::D2,
            vec![0u8; (rw * rh * 4) as usize],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        img.texture_descriptor.usage = TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::COPY_SRC
            | TextureUsages::TEXTURE_BINDING;
        images.add(img)
    };

    // Spawn PBR scene content + capture systems, then run Bevy's renderer
    // lifecycle. `App::run` performs `finish()` then `cleanup()` before looping
    // `update()`; `cleanup()` is what spawns the render thread and inserts the
    // `RenderAppChannels` resource the render-extract step depends on.
    spawn_scene(&mut app, &prep, &world, rw, rh, cap_handle.clone());
    app.insert_resource(CapturePlan {
        schedule: prep.schedule.clone(),
        rigs: prep.rigs.clone(),
        world: prep.world_before.clone(),
        fps,
        n_frames,
        frames_dir: frames_dir.clone(),
        capture_image: cap_handle,
    });
    app.insert_resource(CaptureProgress::default());
    app.add_systems(Update, apply_frame_system);
    app.add_observer(on_captured);

    // Run Bevy's renderer lifecycle once (this is what `App::run` does).
    app.finish();
    app.cleanup();

    // Readiness: poll until the render thread has unpacked `RenderDevice` into
    // the main world. After `cleanup()` the `RenderApp` sub-app lives on the
    // render thread, so readiness is checked via the mirrored main-world
    // `RenderDevice`, not `app.sub_app(RenderApp)`.
    let mut readiness_attempts = 0usize;
    while !render_device_ready(&app) && readiness_attempts < 600 {
        app.update();
        readiness_attempts += 1;
    }
    if !render_device_ready(&app) {
        return Err(backlot_core::error::CoreError::Msg(
            "bevy renderer did not initialize a RenderDevice within 600 updates".into(),
        ));
    }
    tracing::info!("GPU RenderDevice ready after {readiness_attempts} startup updates");

    // Run the deterministic fixed-step capture loop.
    let t_capture_start = std::time::Instant::now();
    let mut guard = 0usize;
    loop {
        let captured = app.world().resource::<CaptureProgress>().captured;
        if captured >= n_frames {
            break;
        }
        if guard > (n_frames as usize) * 3 + 200 {
            tracing::warn!(
                "bevy capture stalled: captured {captured}/{n_frames}; stopping",
                captured = captured,
                n_frames = n_frames
            );
            break;
        }
        app.update();
        guard += 1;
        if guard % 60 == 0 {
            let c = app.world().resource::<CaptureProgress>().captured;
            tracing::info!("bevy capture progress: requested up to {}, captured {c}/{n_frames}", app.world().resource::<CaptureProgress>().requested);
        }
    }
    let bevy_capture_secs = t_capture_start.elapsed().as_secs_f32();
    let effective_fps = if bevy_capture_secs > 0.0 {
        Some(app.world().resource::<CaptureProgress>().captured as f32 / bevy_capture_secs)
    } else {
        None
    };

    let captured = app.world().resource::<CaptureProgress>().captured;
    if captured == 0 {
        return Err(backlot_core::error::CoreError::Msg(
            "bevy renderer produced no frames (capture readback failed)".into(),
        ));
    }
    tracing::info!(
        "bevy capture complete: {captured}/{n_frames} frames, effective {effective_fps:?} fps",
        effective_fps = effective_fps
    );

    // Stage 3: shared mix/encode/verify/package with timing tracked.
    let t_pkg_start = std::time::Instant::now();
    let mut report = finalize_production(&config, require_llm, &prep, &frames_dir, captured, "bevy")?;
    let packaging_and_mix = t_pkg_start.elapsed().as_secs_f32();
    let total_end_to_end_secs = total_start.elapsed().as_secs_f32();
    let ended_at = chrono::Utc::now().to_rfc3339();

    // Attach timing to the persisted diagnostics if available, then re-write.
    {
        let ep_dir = Path::new(&config.runtime.output_dir).join("episodes").join(&report.episode_id);
        let diag_path = ep_dir.join("diagnostics.json");
        if let Ok(raw) = std::fs::read_to_string(&diag_path) {
            if let Ok(mut diag) = serde_json::from_str::<backlot_core::package::Diagnostics>(&raw) {
                diag.timing = Some(TimingReport {
                    llm_authoring_secs: prep.prep_timings.llm_authoring_secs,
                    tts_generation_secs: prep.prep_timings.tts_generation_secs,
                    timeline_prep_secs: prep.prep_timings.timeline_prep_secs,
                    bevy_capture_secs,
                    audio_mixing_secs: 0.0,
                    ffmpeg_encode_secs: 0.0,
                    packaging_secs: packaging_and_mix,
                    total_end_to_end_secs,
                    effective_fps,
                    started_at: started_at.clone(),
                    ended_at: ended_at.clone(),
                });
                let json = serde_json::to_string_pretty(&diag).unwrap_or_default();
                let _ = std::fs::write(&diag_path, &json);
                report.issues = diag.issues.clone();
            }
        }
    }

    if !config.runtime.capture_frames && captured > 0 {
        let _ = std::fs::remove_dir_all(&frames_dir);
    }
    Ok(report)
}

/// Spawn the static + dynamic scene elements into the app world.
fn render_device_ready(app: &App) -> bool {
    // After `cleanup()`, the `RenderApp` sub-app lives on the render thread, so
    // readiness is read from the `RenderDevice` mirrored into the main world by
    // the render thread once the GPU device initializes.
    app.world().get_resource::<RenderDevice>().is_some()
}

/// Targeted error handler: tolerate ONLY the transient "Resource does not exist"
/// validation errors that occur on the first `update()` calls while the GPU
/// `RenderDevice` is still being unpacked into the main world. Every other error
/// (including real panics) defers to the default `match_severity` and aborts.
fn tolerate_startup_resource_error(err: BevyError, ctx: ErrorContext) {
    if err.to_string().contains("Resource does not exist") {
        warn!(
            "tolerating transient startup resource error in {}: {}",
            ctx.name(),
            err
        );
        return;
    }
    match_severity(err, ctx);
}

fn spawn_scene(
    app: &mut App,
    prep: &backlot_core::render::PreparedProduction,
    world: &WorldState,
    rw: u32,
    rh: u32,
    cap_handle: Handle<Image>,
) {
    let bw = &mut app.world_mut();

    let unit_cube = {
        let mut meshes = bw.resource_mut::<Assets<Mesh>>();
        meshes.add(Cuboid::new(1.0, 1.0, 1.0))
    };
    let sphere = {
        let mut meshes = bw.resource_mut::<Assets<Mesh>>();
        meshes.add(Sphere::new(0.18))
    };
    // Unit primitives for articulated humanoid performers. The rig is an
    // articulated skeleton; we render each joint's body part as a capsule
    // (limbs/torso) or sphere (head/jaw/eyes) so the figures read as humanoid
    // rather than as blocky cuboids. Size is baked per-part in the frame system
    // via non-uniform scale from the rig's `half` extents.
    let unit_sphere = {
        let mut meshes = bw.resource_mut::<Assets<Mesh>>();
        meshes.add(Sphere::new(0.5))
    };
    let unit_capsule = {
        let mut meshes = bw.resource_mut::<Assets<Mesh>>();
        meshes.add(Capsule3d::new(0.5, 0.5))
    };

    let floor_mat = {
        let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.22, 0.20, 0.24),
            perceptual_roughness: 0.85,
            ..default()
        })
    };
    let wall_mat = {
        let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.34, 0.32, 0.38),
            perceptual_roughness: 0.95,
            ..default()
        })
    };
    let trim_mat = {
        let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.50, 0.44),
            perceptual_roughness: 0.7,
            ..default()
        })
    };
    let elevator_mat = {
        let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.44, 0.48),
            perceptual_roughness: 0.4,
            metallic: 0.4,
            ..default()
        })
    };
    let ceiling_mat = {
        let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.30, 0.28, 0.30),
            perceptual_roughness: 1.0,
            ..default()
        })
    };

    // --- Floor ---
    let floor_size = 24.0;
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(floor_mat),
        Transform {
            translation: Vec3::new(0.0, -0.05, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(floor_size, 0.1, floor_size),
        },
    ));

    // --- Back + side walls (a simple studio backlot) ---
    let wall_h = 6.0;
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(wall_mat.clone()),
        Transform {
            translation: Vec3::new(0.0, wall_h / 2.0, -6.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(16.0, wall_h, 0.3),
        },
    ));
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(wall_mat.clone()),
        Transform {
            translation: Vec3::new(-8.0, wall_h / 2.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(0.3, wall_h, 16.0),
        },
    ));
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(wall_mat.clone()),
        Transform {
            translation: Vec3::new(8.0, wall_h / 2.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(0.3, wall_h, 16.0),
        },
    ));

    // --- Ceiling (closes the corridor so lights read as interior) ---
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(ceiling_mat),
        Transform {
            translation: Vec3::new(0.0, wall_h, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(16.0, 0.2, 16.0),
        },
    ));

    // --- Baseboards along the back + side walls (depth + material separation) ---
    for (tx, tz, sx, sz) in [
        (0.0f32, -5.85, 16.0f32, 0.12f32),
        (-7.85, 0.0, 0.12, 16.0),
        (7.85, 0.0, 0.12, 16.0),
    ] {
        bw.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(trim_mat.clone()),
            Transform {
                translation: Vec3::new(tx, 0.15, tz),
                rotation: Quat::IDENTITY,
                scale: Vec3::new(sx, 0.3, sz),
            },
        ));
    }

    // --- Elevator box prop (set piece) ---
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(elevator_mat),
        Transform {
            translation: Vec3::new(-5.5, 1.4, -5.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(2.2, 2.8, 1.6),
        },
    ));

    // --- Apartment hallway dressing ---
    // Corridor carpet runner leading to the elevator.
    let carpet_mat = {
        let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.22, 0.10, 0.12),
            perceptual_roughness: 1.0,
            ..default()
        })
    };
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(carpet_mat),
        Transform {
            translation: Vec3::new(0.0, 0.01, -2.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(3.0, 0.04, 12.0),
        },
    ));

    // Apartment doors along the back wall, each with a frame.
    let door_mat = {
        let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.36, 0.24, 0.14),
            perceptual_roughness: 0.85,
            ..default()
        })
    };
    let frame_mat = {
        let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.55, 0.58),
            perceptual_roughness: 0.9,
            ..default()
        })
    };
    let indicator_mat = {
        let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.85, 0.2),
            emissive: Color::srgb(0.9, 0.8, 0.2).into(),
            perceptual_roughness: 0.4,
            ..default()
        })
    };
    for dx in [2.5f32, 4.5, 6.5] {
        // Door frame (sits just inside the wall).
        bw.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(frame_mat.clone()),
            Transform {
                translation: Vec3::new(dx, 1.1, -5.9),
                rotation: Quat::IDENTITY,
                scale: Vec3::new(1.4, 2.6, 0.08),
            },
        ));
        // Door panel (flush with the wall face at z = -5.85).
        bw.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(door_mat.clone()),
            Transform {
                translation: Vec3::new(dx, 1.1, -5.84),
                rotation: Quat::IDENTITY,
                scale: Vec3::new(1.1, 2.2, 0.12),
            },
        ));
    }

    // Elevator: door panel + glowing floor indicator.
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(frame_mat.clone()),
        Transform {
            translation: Vec3::new(-5.5, 1.4, -4.19),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(1.9, 2.6, 0.06),
        },
    ));
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(indicator_mat),
        Transform {
            translation: Vec3::new(-5.5, 2.65, -4.16),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(0.18, 0.18, 0.06),
        },
    ));

    // --- Lights ---
    // Key directional light (warm, angled to carve performers from the set).
    bw.spawn((
        DirectionalLight { illuminance: 2400.0, color: Color::srgb(1.0, 0.95, 0.88), ..default() },
        Transform::from_xyz(4.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // Ambient fill lifts the whole corridor out of near-black.
    bw.spawn(AmbientLight {
        color: Color::srgb(0.62, 0.60, 0.66),
        brightness: 1.4,
        affects_lightmapped_meshes: false,
    });
    // Hallway practical: warm ceiling fixture above the staging area.
    bw.spawn((
        PointLight {
            intensity: 2200.0,
            color: Color::srgb(1.0, 0.88, 0.72),
            range: 14.0,
            radius: 0.4,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, 5.2, 0.0),
    ));
    // Elevator interior light (reads the indicator + frame clearly).
    bw.spawn((
        PointLight {
            intensity: 1400.0,
            color: Color::srgb(0.85, 0.90, 1.0),
            range: 8.0,
            radius: 0.3,
            ..default()
        },
        Transform::from_xyz(-5.5, 2.4, -4.6),
    ));
    // Soft cool fill from the opposite side to separate characters from walls.
    bw.spawn((
        PointLight {
            intensity: 900.0,
            color: Color::srgb(0.7, 0.78, 0.95),
            range: 12.0,
            radius: 0.3,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(3.0, 3.0, 3.0),
    ));

    // --- Characters: articulated rigs rendered as capsule/sphere humanoids ---
    for rig in prep.rigs.values() {
        for part in &rig.parts {
            let mat = {
                let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
                mats.add(StandardMaterial {
                    base_color: clamp_color(part.color),
                    perceptual_roughness: 0.7,
                    ..default()
                })
            };
            let (mesh, shape) = match part.joint {
                SemanticJoint::Head
                | SemanticJoint::Jaw
                | SemanticJoint::LeftEye
                | SemanticJoint::RightEye
                | SemanticJoint::Gaze
                | SemanticJoint::PropGrip => (unit_sphere.clone(), PartShape::Sphere),
                _ => (unit_capsule.clone(), PartShape::Capsule),
            };
            bw.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mat),
                RigPartTag {
                    char_id: rig.character_id.clone(),
                    joint: part.joint,
                    shape,
                },
                Transform::IDENTITY,
            ));
        }
    }

    // --- Props ---
    for p in world.props.values() {
        let mat = {
            let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
            mats.add(StandardMaterial {
                base_color: Color::srgb(0.9, 0.7, 0.2),
                perceptual_roughness: 0.5,
                ..default()
            })
        };
        bw.spawn((
            Mesh3d(sphere.clone()),
            MeshMaterial3d(mat),
            PropTag { prop_id: p.id.clone() },
            Transform::IDENTITY,
        ));
    }

    // --- Camera: renders to the offscreen capture image ---
    // `Camera3d` is a marker whose required `CameraRenderGraph` component is added
    // automatically. Spawn it FIRST so that component exists before `Camera`'s
    // hook checks for a render graph (otherwise a benign warning is emitted).
    let aspect = rw as f32 / rh as f32;
    bw.spawn((
        CameraRenderGraph::new(Core3d),
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.03, 0.03, 0.05)),
            ..default()
        },
        RenderTarget::Image(cap_handle.into()),
        Projection::Perspective(PerspectiveProjection {
            fov: 50.0_f32.to_radians(),
            aspect_ratio: aspect,
            near: 0.1,
            far: 100.0,
            near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -0.1),
        }),
        Transform::from_xyz(0.0, 3.0, 7.0).looking_at(Vec3::new(0.0, 1.2, 0.0), Vec3::Y),
    ));
}

/// Apply the authoritative per-frame state to the scene, then request capture.
fn apply_frame_system(
    mut commands: Commands,
    plan: Res<CapturePlan>,
    mut progress: ResMut<CaptureProgress>,
    mut parts: Query<(&RigPartTag, &mut Transform), (Without<PropTag>, Without<Camera3d>)>,
    mut props: Query<(&PropTag, &mut Transform), (Without<RigPartTag>, Without<Camera3d>)>,
    mut cam: Query<&mut Transform, (With<Camera3d>, Without<RigPartTag>, Without<PropTag>)>,
) {
    let n = plan.n_frames.max(1);
    let req = progress.requested;
    let idx = req.min(n - 1);
    let t = idx as f32 / plan.fps as f32;

    let state = evaluate_at(&plan.schedule, &plan.rigs, &plan.world, t);

    // Characters: drive each rig part from the shared world state.
    for (cf, pose) in &state.chars {
        if let Some(rig) = plan.rigs.get(&cf.id) {
            let wm = rig.world_matrices(&cf.root, pose);
            for (tag, mut tr) in parts.iter_mut() {
                if tag.char_id != cf.id {
                    continue;
                }
                let Some(rw) = wm.get(&tag.joint) else { continue };
                let half = rig
                    .parts
                    .iter()
                    .find(|p| p.joint == tag.joint)
                    .map(|p| p.half)
                    .unwrap_or([0.1, 0.1, 0.1]);
                tr.translation = Vec3::new(rw.pos[0], rw.pos[1], rw.pos[2]);
                tr.rotation = mat3_to_quat(rw.rot);
                // Capsule unit is radius 0.5 / half-length 0.5 (Y-long); sphere
                // unit is radius 0.5. Scale from the rig `half` extents so each
                // bone reads as a rounded limb/torso, not a cuboid.
                tr.scale = match tag.shape {
                    PartShape::Sphere => {
                        let r = half[0].max(half[1]).max(half[2]);
                        Vec3::splat(r * 2.0)
                    }
                    PartShape::Capsule => {
                        let r = half[0].min(half[2]);
                        Vec3::new(r * 2.0, half[1] * 2.0, r * 2.0)
                    }
                };
            }
        }
    }

    // Props.
    for (tag, mut tr) in props.iter_mut() {
        if let Some(pf) = state.props.iter().find(|p| p.id == tag.prop_id) {
            tr.translation = Vec3::new(pf.pos[0], pf.pos[1], pf.pos[2]);
        }
    }

    // Camera.
    if let Ok(mut cam_tr) = cam.single_mut() {
        cam_tr.translation = Vec3::new(
            state.camera_eye[0],
            state.camera_eye[1],
            state.camera_eye[2],
        );
        cam_tr.look_at(
            Vec3::new(state.camera_look[0], state.camera_look[1], state.camera_look[2]),
            Vec3::Y,
        );
    }

    // Request a capture for this frame (read back on the next update).
    if req < plan.n_frames {
        commands.spawn((CaptureMarker, Screenshot(RenderTarget::Image(plan.capture_image.clone().into()))));
        progress.queue.push_back(req);
        progress.requested = req + 1;
    }
}

/// Observer: write the read-back frame to a PNG as soon as it is available.
fn on_captured(
    trigger: On<ScreenshotCaptured>,
    mut progress: ResMut<CaptureProgress>,
    plan: Res<CapturePlan>,
) {
    let ev = trigger.event();
    let frame = progress.queue.pop_front().unwrap_or(0);
    let w = ev.image.texture_descriptor.size.width;
    let h = ev.image.texture_descriptor.size.height;
    let format = ev.image.texture_descriptor.format;
    let rgba = to_rgba(ev.image.data.as_deref().unwrap_or(&[]), format);
    let path = plan.frames_dir.join(format!("frame_{:06}.png", frame + 1));
    if let Err(e) = write_png(&path, w, h, &rgba) {
        tracing::warn!("bevy frame write failed: {e}");
    } else {
        progress.captured += 1;
    }
}

/// Convert raw GPU bytes to RGBA, handling BGRA source formats.
fn to_rgba(data: &[u8], format: TextureFormat) -> Vec<u8> {
    match format {
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => data
            .chunks_exact(4)
            .flat_map(|c| [c[2], c[1], c[0], c[3]])
            .collect(),
        _ => data.to_vec(),
    }
}

/// Convert a 3x3 rotation matrix (row-major) to a quaternion.
fn mat3_to_quat(m: [[f32; 3]; 3]) -> Quat {
    let m00 = m[0][0];
    let m01 = m[0][1];
    let m02 = m[0][2];
    let m10 = m[1][0];
    let m11 = m[1][1];
    let m12 = m[1][2];
    let m20 = m[2][0];
    let m21 = m[2][1];
    let m22 = m[2][2];
    let tr = m00 + m11 + m22;
    if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        Quat::from_xyzw((m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s)
    } else if (m00 > m11) && (m00 > m22) {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        Quat::from_xyzw(0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s)
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        Quat::from_xyzw((m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s)
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        Quat::from_xyzw((m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s)
    }
}

fn clamp_color(c: [f32; 3]) -> Color {
    Color::srgb(
        c[0].clamp(0.0, 1.0),
        c[1].clamp(0.0, 1.0),
        c[2].clamp(0.0, 1.0),
    )
}

fn io_err<'a>(p: &'a Path) -> impl FnOnce(std::io::Error) -> backlot_core::error::CoreError + 'a {
    move |source| backlot_core::error::CoreError::Io {
        path: p.to_path_buf(),
        source,
    }
}
