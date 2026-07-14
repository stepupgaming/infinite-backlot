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

use bevy::app::AnimationSystems;
use bevy::asset::{AssetApp, RenderAssetUsages};
use bevy::camera::{ClearColorConfig, PerspectiveProjection, RenderTarget};
use bevy::core_pipeline::Core3d;
use bevy::ecs::error::{match_severity, BevyError, ErrorContext, FallbackErrorHandler};
use bevy::ecs::observer::On;
use bevy::log::warn;
use bevy::math::Vec4;
use bevy::prelude::*;
use bevy::render::camera::CameraRenderGraph;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::renderer::RenderDevice;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::transform::TransformSystems;
use bevy::window::WindowPlugin;
use bevy::world_serialization::WorldInstanceReady;

use backlot_core::author::EpisodeAuthor;
use backlot_core::avatar::{HumanoidRig, PerformanceState, SemanticJoint};
use backlot_core::render::{
    finalize_production, prepare_production, write_png, ProduceConfig, ProduceReport,
    ProductionTimingContext,
};
use backlot_core::stage;
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

#[derive(Component)]
struct CastRoot {
    char_id: String,
}

#[derive(Component, Clone)]
struct PendingCastAnimation {
    char_id: String,
    graph: Handle<AnimationGraph>,
    nodes: Vec<AnimationNodeIndex>,
    clips: Vec<Handle<AnimationClip>>,
}

#[derive(Component, Clone)]
struct CastAnimationPlayer {
    char_id: String,
    nodes: Vec<AnimationNodeIndex>,
    clips: Vec<Handle<AnimationClip>>,
}

#[derive(Clone)]
struct CastBoneBinding {
    char_id: String,
    rest: Transform,
}

#[derive(Resource, Default)]
struct CastBoneBindings {
    bones: HashMap<Entity, CastBoneBinding>,
}

#[derive(Resource, Default)]
struct CastLoadState {
    expected: usize,
    ready: usize,
}

/// A prop mesh bound to a world prop id.
#[derive(Component)]
struct PropTag {
    prop_id: String,
}

/// One sliding elevator-door leaf driven by `FrameState::elevator_open`.
#[derive(Component)]
struct ElevatorDoor {
    closed_x: f32,
    direction: f32,
}

#[derive(Component)]
struct ElevatorIndicator;

#[derive(Component)]
struct ControlPanelButton;

#[derive(Component)]
struct ImpossibleFloorBackdrop;

#[derive(Component)]
struct ElevatorInteriorLight;

/// Marker for the per-frame screenshot entity we spawn to trigger readback.
#[derive(Component)]
struct CaptureMarker;

/// Shared production plan + render target for the capture loop.
#[derive(Resource)]
struct CapturePlan {
    schedule: Schedule,
    motion_plan: backlot_core::motion::ProductionMotionPlan,
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
    let total_start = std::time::Instant::now();
    let started_at = chrono::Utc::now().to_rfc3339();
    let ProduceConfig {
        config,
        require_llm,
        world,
        seed,
        episode_number,
        keep_frames,
    } = cfg;
    let out_dir = config.runtime.output_dir.clone();
    let ep_dir = Path::new(&out_dir)
        .join("episodes")
        .join(backlot_core::serial_id("episode", episode_number, 6));
    let frames_dir = ep_dir.join("frames");
    if frames_dir.exists() {
        std::fs::remove_dir_all(&frames_dir).map_err(io_err(&frames_dir))?;
    }
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
    let asset_root = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("assets");
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: asset_root.to_string_lossy().into_owned(),
                ..default()
            })
            .set(WindowPlugin {
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
            Extent3d {
                width: rw,
                height: rh,
                depth_or_array_layers: 1,
            },
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
        motion_plan: prep.motion_plan.clone(),
        rigs: prep.rigs.clone(),
        world: prep.world_before.clone(),
        fps,
        n_frames,
        frames_dir: frames_dir.clone(),
        capture_image: cap_handle,
    });
    app.insert_resource(CaptureProgress::default());
    let diagnostic_puppets = std::env::var("BACKLOT_DIAGNOSTIC_PUPPETS")
        .map(|value| value == "1")
        .unwrap_or(false);
    app.insert_resource(CastLoadState {
        expected: if diagnostic_puppets {
            0
        } else {
            prep.schedule.characters.len()
        },
        ready: 0,
    });
    app.insert_resource(CastBoneBindings::default());
    app.add_systems(Update, apply_frame_system);
    app.add_systems(
        PostUpdate,
        apply_cast_acting_overlay
            .after(AnimationSystems)
            .before(TransformSystems::Propagate),
    );
    app.add_observer(on_captured);
    app.add_observer(on_cast_instance_ready);

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
        if guard > (n_frames as usize) * 12 + 1000 {
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
            tracing::info!(
                "bevy capture progress: requested up to {}, captured {c}/{n_frames}",
                app.world().resource::<CaptureProgress>().requested
            );
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

    // Stage 3: shared mix/encode/verify/package. Pass renderer-owned timing in so
    // diagnostics.json and report.md are generated once from the same values.
    let timing = ProductionTimingContext {
        started_at,
        elapsed_before_finalize_secs: total_start.elapsed().as_secs_f32(),
        bevy_capture_secs,
        effective_fps,
    };
    let report = finalize_production(
        &config,
        require_llm,
        &prep,
        &frames_dir,
        captured,
        "bevy",
        Some(&timing),
    )?;

    if !keep_frames && captured > 0 {
        let _ = std::fs::remove_dir_all(&frames_dir);
    }
    Ok(report)
}

/// Canonical acting correction applied after imported clip evaluation. It
/// establishes a relaxed show-ready rest pose and layers bounded semantic
/// accents, so a missing/poor source curve cannot strand a performer in the
/// glTF bind pose. Kimodo-processed clips use the same late correction slot for
/// gaze, hand contact and foot locking.
fn apply_cast_acting_overlay(
    mut commands: Commands,
    plan: Res<CapturePlan>,
    progress: Res<CaptureProgress>,
    mut bones: Query<(Entity, &Name, &mut Transform)>,
    parents: Query<&ChildOf>,
    cast_roots: Query<&CastRoot>,
    mut bindings: ResMut<CastBoneBindings>,
    attachments: Query<(Entity, &Name)>,
    mut removed_attachments: Local<std::collections::HashSet<Entity>>,
    mut reported: Local<bool>,
) {
    if progress.requested == 0 {
        return;
    }
    let frame_number = progress
        .requested
        .saturating_sub(1)
        .min(plan.n_frames.saturating_sub(1));
    let time = frame_number as f32 / plan.fps.max(1) as f32;
    let state = evaluate_at(&plan.schedule, &plan.rigs, &plan.world, time);
    for (entity, name) in &attachments {
        if is_cast_hand_prop(name.as_str()) && removed_attachments.insert(entity) {
            commands.entity(entity).despawn();
        }
    }
    let mut matched_bones = 0usize;
    let mut corrected_bones = 0usize;
    let mut bound_characters = std::collections::HashSet::new();
    for (entity, name, mut transform) in &mut bones {
        let binding = if let Some(binding) = bindings.bones.get(&entity).cloned() {
            binding
        } else {
            // WorldInstanceReady can precede Bevy's final scene-world entity
            // replacement. Bind any live descendant lazily and freeze its
            // target-rig rest transform exactly once.
            let mut ancestor = entity;
            let mut owner = None;
            for _ in 0..32 {
                if let Ok(root) = cast_roots.get(ancestor) {
                    owner = Some(root.char_id.clone());
                    break;
                }
                let Ok(parent) = parents.get(ancestor) else {
                    break;
                };
                ancestor = parent.parent();
            }
            let Some(char_id) = owner else {
                continue;
            };
            let binding = CastBoneBinding {
                char_id,
                rest: *transform,
            };
            bindings.bones.insert(entity, binding.clone());
            binding
        };
        let char_id = binding.char_id.as_str();
        bound_characters.insert(char_id.to_string());
        matched_bones += 1;
        let Some((frame, _)) = state.chars.iter().find(|(frame, _)| frame.id == char_id) else {
            continue;
        };
        // Start from the immutable target-rig rest transform on every update.
        // Capture needs several app updates per output frame while the GPU
        // readback is in flight; additive multiplication against the previous
        // update would otherwise accumulate into flips and corkscrews.
        transform.translation = binding.rest.translation;
        transform.rotation = binding.rest.rotation;
        let weight = frame.action_weight.clamp(0.0, 1.0);
        let pulse = (time * 3.1 + if char_id == "mara" { 0.0 } else { 1.7 }).sin();
        let talk = if frame.state == PerformanceState::Talk {
            (frame.action_local_time * 5.2).sin() * weight
        } else {
            0.0
        };

        // KayKit's rig reference pose has both upper arms horizontal. Normalize
        // that cast-specific rest pose once before applying SOMA motion deltas;
        // this is retarget alignment, not a substitute animation layer.
        match name.as_str() {
            "upperarm.l" => transform.rotation *= Quat::from_rotation_z(1.12),
            "upperarm.r" => transform.rotation *= Quat::from_rotation_z(-1.12),
            _ => {}
        }

        // Sample approved SOMA/Kimodo motion as a delta over the cast's native
        // relaxed pose. Frame zero is the source rest reference, so the same
        // processed clip works on both approved cast rigs without replacing
        // their native proportions or secondary animation.
        if let Some((segment, clip)) = plan.motion_plan.active(char_id, time) {
            if let Some(track) = clip
                .tracks
                .iter()
                .find(|track| track.joint == name.as_str())
            {
                if let Some(first) = track.rotations.first() {
                    let normalized =
                        ((time - segment.start) / segment.duration.max(0.001)).clamp(0.0, 1.0);
                    let sample = ((normalized * clip.duration * clip.sample_rate).round() as usize)
                        .min(track.rotations.len().saturating_sub(1));
                    if let Some(current) = track.rotations.get(sample) {
                        let source_rest = Quat::from_xyzw(first[0], first[1], first[2], first[3]);
                        let source_pose =
                            Quat::from_xyzw(current[0], current[1], current[2], current[3]);
                        let delta = (source_rest.inverse() * source_pose).normalize();
                        transform.rotation *= Quat::IDENTITY.slerp(delta, weight);

                        let channels: &[usize] = match name.as_str() {
                            "foot.l" => &[0, 1, 2],
                            "foot.r" => &[3, 4, 5],
                            _ => &[],
                        };
                        if let Some(offsets) = clip.foot_lock_offsets.get(sample) {
                            let mut correction = Vec3::ZERO;
                            let mut count = 0.0;
                            for channel in channels {
                                if let Some(offset) = offsets.get(*channel) {
                                    correction += Vec3::from_array(*offset);
                                    count += 1.0;
                                }
                            }
                            if count > 0.0 {
                                transform.translation +=
                                    (correction / count).clamp_length_max(0.05);
                            }
                        }
                        continue;
                    }
                }
            }
        }

        // Imported animation is authoritative. Acting accents are additive and
        // action-local, so an overlay with zero weight returns exactly to the
        // sampled base pose instead of replacing it with a hard-coded bind pose.
        let additive = match name.as_str() {
            "upperarm.l" => {
                let lift = match frame.state {
                    PerformanceState::Gesture | PerformanceState::Point => 0.72 * weight,
                    PerformanceState::React => 0.42 * weight,
                    PerformanceState::Talk => 0.18 * talk.max(0.0),
                    _ => 0.0,
                };
                Quat::from_euler(
                    EulerRot::XYZ,
                    0.0,
                    0.025 * pulse,
                    -0.72 * lift.clamp(0.0, 1.0),
                )
            }
            "upperarm.r" => {
                let lift = match frame.state {
                    PerformanceState::Point => 0.92 * weight,
                    PerformanceState::Gesture => 0.68 * weight,
                    PerformanceState::React => 0.42 * weight,
                    PerformanceState::Talk => 0.20 * talk.max(0.0),
                    _ => 0.0,
                };
                Quat::from_euler(
                    EulerRot::XYZ,
                    0.0,
                    -0.025 * pulse,
                    0.72 * lift.clamp(0.0, 1.0),
                )
            }
            "lowerarm.l" => Quat::from_rotation_z(-0.18 * talk.max(0.0)),
            "lowerarm.r" => {
                let flex = if matches!(
                    frame.state,
                    PerformanceState::Gesture | PerformanceState::Point
                ) {
                    0.55 * weight
                } else {
                    0.22 * talk.max(0.0)
                };
                Quat::from_rotation_z(flex)
            }
            "head" => {
                let reaction_pitch = if frame.state == PerformanceState::React {
                    -0.12 * weight
                } else {
                    0.015 * pulse
                };
                Quat::from_euler(EulerRot::YXZ, 0.035 * talk, reaction_pitch, 0.0)
            }
            "spine" => Quat::from_euler(
                EulerRot::XYZ,
                0.018 * pulse,
                if frame.state == PerformanceState::Listen {
                    0.06
                } else {
                    0.0
                },
                0.025 * talk,
            ),
            _ => continue,
        };
        transform.rotation *= additive;
        corrected_bones += 1;
    }
    if !*reported {
        eprintln!(
            "CAST OVERLAY players={} matched_bones={} corrected_bones={}",
            bound_characters.len(),
            matched_bones,
            corrected_bones
        );
        *reported = true;
    }
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
            translation: Vec3::from_array(stage::ELEVATOR_CENTER),
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

    // Elevator: two sliding door leaves + glowing floor indicator. Their motion
    // is applied from the same authoritative FrameState used by both renderers.
    for (closed_x, direction) in [(-5.975f32, -1.0f32), (-5.025, 1.0)] {
        bw.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(frame_mat.clone()),
            ElevatorDoor {
                closed_x,
                direction,
            },
            Transform {
                translation: Vec3::new(closed_x, 1.4, -4.19),
                rotation: Quat::IDENTITY,
                scale: Vec3::new(0.94, 2.6, 0.06),
            },
        ));
    }
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(indicator_mat),
        ElevatorIndicator,
        Transform {
            translation: Vec3::from_array(stage::ELEVATOR_INDICATOR),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(0.18, 0.18, 0.06),
        },
    ));

    // Readable elevator interior: dark rear wall, brushed side returns, a
    // luminous threshold, and a control panel that physically responds.
    let interior_mat = {
        let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.10, 0.13, 0.18),
            metallic: 0.35,
            perceptual_roughness: 0.42,
            ..default()
        })
    };
    let impossible_mat = {
        let mut mats = bw.resource_mut::<Assets<StandardMaterial>>();
        mats.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.28, 0.34),
            emissive: Color::srgb(0.05, 0.8, 0.92).into(),
            perceptual_roughness: 0.25,
            ..default()
        })
    };
    for (translation, scale) in [
        (Vec3::new(-5.5, 1.35, -5.72), Vec3::new(1.9, 2.7, 0.08)),
        (Vec3::new(-6.47, 1.35, -5.0), Vec3::new(0.08, 2.7, 1.45)),
        (Vec3::new(-4.53, 1.35, -5.0), Vec3::new(0.08, 2.7, 1.45)),
        (Vec3::new(-5.5, 0.03, -5.0), Vec3::new(1.9, 0.06, 1.45)),
    ] {
        bw.spawn((
            Mesh3d(unit_cube.clone()),
            MeshMaterial3d(interior_mat.clone()),
            Transform {
                translation,
                scale,
                ..default()
            },
        ));
    }
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(impossible_mat.clone()),
        ImpossibleFloorBackdrop,
        Transform {
            translation: Vec3::from_array(stage::IMPOSSIBLE_FLOOR),
            scale: Vec3::new(0.01, 0.01, 0.01),
            ..default()
        },
    ));
    // Panel housing and a separate animated button.
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(frame_mat.clone()),
        Transform {
            translation: Vec3::new(
                stage::ELEVATOR_CONTROL_PANEL[0],
                1.3,
                stage::ELEVATOR_CONTROL_PANEL[2] - 0.08,
            ),
            scale: Vec3::new(0.38, 0.9, 0.10),
            ..default()
        },
    ));
    bw.spawn((
        Mesh3d(unit_cube.clone()),
        MeshMaterial3d(impossible_mat),
        ControlPanelButton,
        Transform {
            translation: Vec3::from_array(stage::ELEVATOR_CONTROL_PANEL),
            scale: Vec3::new(0.14, 0.14, 0.06),
            ..default()
        },
    ));

    // --- Lights ---
    // Key directional light (warm, angled to carve performers from the set).
    bw.spawn((
        DirectionalLight {
            illuminance: 2400.0,
            color: Color::srgb(1.0, 0.95, 0.88),
            ..default()
        },
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
        Transform::from_xyz(
            stage::ELEVATOR_CENTER[0],
            2.4,
            stage::ELEVATOR_DOORS[2] - 0.4,
        ),
        ElevatorInteriorLight,
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

    // Production cast: recognizable, skinned CC0 performers. Diagnostic
    // primitives remain available only when explicitly requested by tests.
    let diagnostic_puppets = std::env::var("BACKLOT_DIAGNOSTIC_PUPPETS")
        .map(|value| value == "1")
        .unwrap_or(false);
    if diagnostic_puppets {
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
    } else {
        let asset_server = bw.resource::<AssetServer>().clone();
        for track in &prep.schedule.characters {
            let Some(rig) = prep.rigs.get(&track.id) else {
                continue;
            };
            let asset_path = if rig.character_id == "mara" || rig.character_id == "nox" {
                "characters/mara.glb"
            } else {
                "characters/ellis.glb"
            };
            let scene: Handle<WorldAsset> =
                asset_server.load(GltfAssetLabel::Scene(0).from_asset(asset_path));
            let clip_handles = (0..76)
                .map(|clip_index| {
                    asset_server.load(GltfAssetLabel::Animation(clip_index).from_asset(asset_path))
                })
                .collect::<Vec<Handle<AnimationClip>>>();
            let (graph, nodes) = AnimationGraph::from_clips(clip_handles.clone());
            let graph = bw.resource_mut::<Assets<AnimationGraph>>().add(graph);
            bw.spawn((
                WorldAssetRoot(scene),
                // KayKit is authored at roughly 2.2 m including silhouette
                // accessories. Normalize once to the 1.8 m canonical stage rig.
                Transform::from_scale(Vec3::splat(0.82)),
                CastRoot {
                    char_id: rig.character_id.clone(),
                },
                PendingCastAnimation {
                    char_id: rig.character_id.clone(),
                    graph,
                    nodes,
                    clips: clip_handles,
                },
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
            PropTag {
                prop_id: p.id.clone(),
            },
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

fn on_cast_instance_ready(
    trigger: On<WorldInstanceReady>,
    mut commands: Commands,
    pending: Query<&PendingCastAnimation>,
    children: Query<&Children>,
    names: Query<&Name>,
    transforms: Query<&Transform>,
    mut players: Query<&mut AnimationPlayer>,
    mut load_state: ResMut<CastLoadState>,
    mut bindings: ResMut<CastBoneBindings>,
) {
    let Ok(pending) = pending.get(trigger.entity) else {
        return;
    };
    let mut attached = false;
    let mut named_descendants = 0usize;
    for child in children.iter_descendants(trigger.entity) {
        if names
            .get(child)
            .map(|name| is_cast_hand_prop(name.as_str()))
            .unwrap_or(false)
        {
            commands.entity(child).insert(Visibility::Hidden);
            continue;
        }
        if let (Ok(_), Ok(rest)) = (names.get(child), transforms.get(child)) {
            named_descendants += 1;
            bindings.bones.insert(
                child,
                CastBoneBinding {
                    char_id: pending.char_id.clone(),
                    rest: *rest,
                },
            );
        }
        if players.get_mut(child).is_ok() {
            // The graph handle is applied at the end of this observer. Playback
            // starts in `apply_frame_system` on the next update, when the handle
            // is guaranteed to be present on the player entity.
            commands.entity(child).insert((
                AnimationGraphHandle(pending.graph.clone()),
                CastAnimationPlayer {
                    char_id: pending.char_id.clone(),
                    nodes: pending.nodes.clone(),
                    clips: pending.clips.clone(),
                },
            ));
            attached = true;
        }
    }
    if attached {
        eprintln!(
            "CAST INSTANCE char={} named_descendants={} animation_player_attached=true",
            pending.char_id, named_descendants
        );
        load_state.ready += 1;
        commands
            .entity(trigger.entity)
            .remove::<PendingCastAnimation>();
    }
}

fn is_cast_hand_prop(name: &str) -> bool {
    matches!(
        name,
        "Spellbook"
            | "Spellbook_open"
            | "1H_Wand"
            | "2H_Staff"
            | "Knife_Offhand"
            | "1H_Crossbow"
            | "2H_Crossbow"
            | "Knife"
            | "Throwable"
    )
}

fn native_clip_index(state: PerformanceState) -> usize {
    match state {
        PerformanceState::Idle
        | PerformanceState::Listen
        | PerformanceState::Look
        | PerformanceState::Talk
        | PerformanceState::Gesture
        | PerformanceState::Point => 66,
        PerformanceState::Walk => 72,
        PerformanceState::React => 34,
    }
}

/// Apply the authoritative per-frame state to the scene, then request capture.
fn apply_frame_system(
    mut commands: Commands,
    plan: Res<CapturePlan>,
    mut progress: ResMut<CaptureProgress>,
    mut scene_warmup: Local<u32>,
    load_state: Res<CastLoadState>,
    mut cast_roots: Query<
        (&CastRoot, &mut Transform),
        (
            Without<Camera3d>,
            Without<RigPartTag>,
            Without<PropTag>,
            Without<ElevatorDoor>,
            Without<ElevatorIndicator>,
            Without<ControlPanelButton>,
            Without<ImpossibleFloorBackdrop>,
        ),
    >,
    mut cast_players: Query<(&CastAnimationPlayer, &mut AnimationPlayer)>,
    clip_assets: Res<Assets<AnimationClip>>,
    mut parts: Query<
        (&RigPartTag, &mut Transform),
        (
            Without<PropTag>,
            Without<ElevatorDoor>,
            Without<ElevatorIndicator>,
            Without<ControlPanelButton>,
            Without<ImpossibleFloorBackdrop>,
            Without<CastRoot>,
            Without<Camera3d>,
        ),
    >,
    mut props: Query<
        (&PropTag, &mut Transform),
        (
            Without<RigPartTag>,
            Without<ElevatorDoor>,
            Without<ElevatorIndicator>,
            Without<ControlPanelButton>,
            Without<ImpossibleFloorBackdrop>,
            Without<CastRoot>,
            Without<Camera3d>,
        ),
    >,
    mut elevator_doors: Query<
        (&ElevatorDoor, &mut Transform),
        (
            Without<RigPartTag>,
            Without<PropTag>,
            Without<ElevatorIndicator>,
            Without<ControlPanelButton>,
            Without<ImpossibleFloorBackdrop>,
            Without<CastRoot>,
            Without<Camera3d>,
        ),
    >,
    mut indicator: Query<
        &mut Transform,
        (
            With<ElevatorIndicator>,
            Without<ControlPanelButton>,
            Without<ImpossibleFloorBackdrop>,
            Without<RigPartTag>,
            Without<PropTag>,
            Without<ElevatorDoor>,
            Without<CastRoot>,
            Without<Camera3d>,
        ),
    >,
    mut panel: Query<
        &mut Transform,
        (
            With<ControlPanelButton>,
            Without<ElevatorIndicator>,
            Without<ImpossibleFloorBackdrop>,
            Without<RigPartTag>,
            Without<PropTag>,
            Without<ElevatorDoor>,
            Without<CastRoot>,
            Without<Camera3d>,
        ),
    >,
    mut reveal: Query<
        &mut Transform,
        (
            With<ImpossibleFloorBackdrop>,
            Without<ElevatorIndicator>,
            Without<ControlPanelButton>,
            Without<RigPartTag>,
            Without<PropTag>,
            Without<ElevatorDoor>,
            Without<CastRoot>,
            Without<Camera3d>,
        ),
    >,
    mut interior_light: Query<&mut PointLight, With<ElevatorInteriorLight>>,
    mut cam: Query<
        &mut Transform,
        (
            With<Camera3d>,
            Without<RigPartTag>,
            Without<PropTag>,
            Without<ElevatorDoor>,
        ),
    >,
) {
    if load_state.ready < load_state.expected {
        return;
    }
    // Screenshot readback is asynchronous. Keep exactly one frame in flight so
    // timeline state cannot advance while the GPU is still returning the
    // previous image; otherwise early frames are black and later PNGs are
    // paired with the wrong requested timestamp.
    if !progress.queue.is_empty() {
        return;
    }
    // WorldInstanceReady only guarantees that the scene hierarchy exists. The
    // graph's separately-labelled glTF clips may still be streaming. Advancing
    // deterministic capture before all clips are resident bakes the bind pose
    // into every frame because capture runs much faster than real time.
    let mut clips_ready = true;
    for (cast, _) in cast_players.iter_mut() {
        clips_ready &= cast
            .clips
            .iter()
            .all(|clip| clip_assets.get(clip).is_some());
    }
    if !clips_ready {
        return;
    }
    let n = plan.n_frames.max(1);
    let req = progress.requested;
    let idx = req.min(n - 1);
    let t = idx as f32 / plan.fps as f32;

    let state = evaluate_at(&plan.schedule, &plan.rigs, &plan.world, t);

    // Characters: drive each rig part from the shared world state.
    for (cf, pose) in &state.chars {
        for (cast, mut transform) in cast_roots.iter_mut() {
            if cast.char_id == cf.id {
                transform.translation = Vec3::new(cf.root.pos[0], cf.root.pos[1], cf.root.pos[2]);
                transform.rotation = Quat::from_euler(
                    EulerRot::XYZ,
                    cf.root.rot[0],
                    cf.root.rot[1],
                    cf.root.rot[2],
                );
            }
        }
        for (cast, mut player) in cast_players.iter_mut() {
            if cast.char_id == cf.id {
                let generated_motion_active = plan.motion_plan.active(&cf.id, t).is_some();
                // A reviewed Kimodo clip owns the complete visible performance.
                // KayKit contributes only a frozen neutral target-rig pose; its
                // stock locomotion/acting clips must never play underneath the
                // retargeted SOMA motion.
                let clip_index = if generated_motion_active {
                    native_clip_index(PerformanceState::Idle)
                } else {
                    native_clip_index(cf.state)
                };
                if let Some(node) = cast.nodes.get(clip_index).copied() {
                    let sample_time = if generated_motion_active {
                        0.05
                    } else {
                        match cf.state {
                            PerformanceState::Idle
                            | PerformanceState::Listen
                            | PerformanceState::Look
                            | PerformanceState::Talk
                            | PerformanceState::Walk => t + 0.25,
                            _ => cf.action_local_time.max(0.0) + 0.05,
                        }
                    };
                    player.stop_all();
                    player
                        .play(node)
                        .repeat()
                        .set_speed(0.0)
                        .seek_to(sample_time);
                }
            }
        }
        if let Some(rig) = plan.rigs.get(&cf.id) {
            let wm = rig.world_matrices(&cf.root, pose);
            for (tag, mut tr) in parts.iter_mut() {
                if tag.char_id != cf.id {
                    continue;
                }
                let Some(rw) = wm.get(&tag.joint) else {
                    continue;
                };
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

    // Elevator doors.
    for (door, mut tr) in elevator_doors.iter_mut() {
        let slide = 0.9 * state.elevator_open.clamp(0.0, 1.0);
        tr.translation.x = door.closed_x + door.direction * slide;
    }
    if let Ok(mut transform) = indicator.single_mut() {
        let pulse = if state.elevator_indicator.is_some() {
            1.0 + 0.16 * (t * 8.0).sin().abs()
        } else {
            1.0
        };
        transform.scale = Vec3::new(0.18 * pulse, 0.18 * pulse, 0.06);
    }
    if let Ok(mut transform) = panel.single_mut() {
        let active = state.panel_active.clamp(0.0, 1.0);
        transform.translation.z = stage::ELEVATOR_CONTROL_PANEL[2] - 0.025 * active;
        transform.scale = Vec3::new(0.14 + 0.04 * active, 0.14 + 0.04 * active, 0.06);
    }
    if let Ok(mut transform) = reveal.single_mut() {
        let amount = state.impossible_reveal.clamp(0.0, 1.0);
        transform.scale = Vec3::new((1.75 * amount).max(0.01), (2.55 * amount).max(0.01), 0.04);
    }
    if let Ok(mut light) = interior_light.single_mut() {
        light.intensity = 1400.0 + state.impossible_reveal.clamp(0.0, 1.0) * 2600.0;
        if state.flicker && (t * 18.0).sin() > 0.0 {
            light.intensity *= 0.25;
        }
    }

    // Camera.
    if let Ok(mut cam_tr) = cam.single_mut() {
        let mut eye = Vec3::new(
            state.camera_eye[0],
            state.camera_eye[1],
            state.camera_eye[2],
        );
        let mut look = Vec3::new(
            state.camera_look[0],
            state.camera_look[1],
            state.camera_look[2],
        );
        // Authored shot offsets are composed around staging marks. Follow the
        // actual evaluated actor root so locomotion cannot leave the subject
        // behind while preserving the chosen shot size and screen direction.
        if let Some(shot) = plan
            .schedule
            .camera_shots
            .iter()
            .find(|shot| t >= shot.start && t < shot.end)
        {
            if let Some((subject, _)) = state
                .chars
                .iter()
                .find(|(character, _)| character.id == shot.subject)
            {
                let desired_look = Vec3::new(
                    subject.root.pos[0],
                    subject.root.pos[1] + 1.23,
                    subject.root.pos[2],
                );
                let distance = if shot.intent.contains("group")
                    || shot.intent.contains("establish")
                    || shot.intent.contains("spatial")
                {
                    5.8
                } else if shot.intent.contains("closeup") {
                    3.4
                } else {
                    4.2
                };
                // The playable hallway is on the +Z side of every staging
                // mark. Re-anchor there instead of translating a candidate
                // across walls when the performer has changed marks.
                eye = Vec3::new(
                    desired_look.x,
                    desired_look.y + 0.3,
                    desired_look.z + distance,
                );
                look = desired_look;
            }
        }
        cam_tr.translation = eye;
        cam_tr.look_at(look, Vec3::Y);
    }

    // Request a capture for this frame (read back on the next update).
    // Give newly instantiated skinned worlds time to reach the render world.
    // No timeline time advances during this warmup, so frame zero remains
    // deterministic and the exported video does not begin with black frames.
    if req == 0 && *scene_warmup < 90 {
        *scene_warmup += 1;
        return;
    }
    if req < plan.n_frames {
        commands.spawn((
            CaptureMarker,
            Screenshot(RenderTarget::Image(plan.capture_image.clone().into())),
        ));
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
