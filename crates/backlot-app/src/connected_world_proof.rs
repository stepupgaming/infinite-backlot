//! Short deterministic Bevy GPU proof for the Blender-authored connected world.

use crate::connected_world::{
    load_connected_world_contract, lobby_to_odd_hours_path, sample_proof_path,
    ConnectedWorldManifest, ProofPathPoint, RuntimeLightIntent, CONNECTED_WORLD_MODULE_ID,
};
use backlot_core::render::write_png;
use bevy::asset::{AssetApp, AssetPlugin, RenderAssetUsages};
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
use bevy::window::{Window, WindowPlugin};
use bevy::world_serialization::WorldInstanceReady;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProofTiming {
    pub duration_secs: f32,
    pub fps: u32,
    pub frames: u32,
    pub width: u32,
    pub height: u32,
}

impl ProofTiming {
    pub fn production() -> Self {
        let duration_secs = 20.0;
        let fps = 12;
        Self {
            duration_secs,
            fps,
            frames: (duration_secs * fps as f32) as u32,
            width: 960,
            height: 540,
        }
    }

    pub fn lighting_preview() -> Self {
        let duration_secs = 4.0;
        let fps = 8;
        Self {
            duration_secs,
            fps,
            frames: (duration_secs * fps as f32) as u32,
            width: 640,
            height: 360,
        }
    }
}

pub fn proof_camera_anchor_at(time: f32) -> &'static str {
    if time < 5.0 {
        "CAM_MASTER_LOBBY"
    } else if time < 10.0 {
        "CAM_MASTER_ENTRANCE"
    } else if time < 15.0 {
        "CAM_SIDEWALK_TWO_SHOT"
    } else {
        "CAM_MASTER_STORE"
    }
}

fn proof_camera_shot_start(time: f32) -> f32 {
    if time < 5.0 {
        0.0
    } else if time < 10.0 {
        5.0
    } else if time < 15.0 {
        10.0
    } else {
        15.0
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectedWorldProofSummary {
    pub module_id: String,
    pub runtime_glb: String,
    pub registry_version: u32,
    pub renderer: String,
    pub timing: ProofTiming,
    pub captured_frames: u32,
    pub named_world_nodes: usize,
    pub hidden_semantic_helpers: usize,
    pub runtime_controls_bound: usize,
    pub runtime_lights_spawned: usize,
    pub staging_marks_registered: usize,
    pub camera_anchors_registered: usize,
    pub interactions_registered: usize,
    pub transitions_registered: usize,
    pub collision_proxies_registered: usize,
    pub actor_asset: String,
    pub locomotion: String,
    pub material_contract: String,
    pub output_mp4: String,
}

#[derive(Component)]
struct ConnectedWorldRoot;

#[derive(Component)]
struct ProofActorRoot;

#[derive(Component)]
struct ProofCamera;

#[derive(Component)]
struct ProofCaptureMarker;

#[derive(Component)]
struct SemanticRegistration {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    kind: String,
}

#[derive(Component, Clone)]
struct PendingProofActor {
    graph: Handle<AnimationGraph>,
    nodes: Vec<AnimationNodeIndex>,
    clips: Vec<Handle<AnimationClip>>,
}

#[derive(Component, Clone)]
struct ProofActorPlayer {
    nodes: Vec<AnimationNodeIndex>,
    clips: Vec<Handle<AnimationClip>>,
}

#[derive(Resource, Default, Debug)]
struct ConnectedWorldRuntime {
    ready: bool,
    failed: Option<String>,
    named_nodes: HashMap<String, Entity>,
    hidden_helpers: usize,
    controls_bound: usize,
}

#[derive(Resource, Default, Debug)]
struct ProofActorRuntime {
    ready: bool,
}

#[derive(Resource)]
struct ProofPlan {
    timing: ProofTiming,
    path: Vec<ProofPathPoint>,
    frames_dir: PathBuf,
    capture_image: Handle<Image>,
    lighting_preview: bool,
}

#[derive(Resource, Default)]
struct ProofProgress {
    requested: u32,
    captured: u32,
    queue: VecDeque<u32>,
}

pub fn produce_connected_world_proof(
    project_root: &Path,
    output_dir: &Path,
    lighting_preview: bool,
) -> Result<ConnectedWorldProofSummary, String> {
    let contract = load_connected_world_contract(project_root, CONNECTED_WORLD_MODULE_ID)
        .map_err(|error| error.to_string())?;
    let manifest = contract.manifest.clone();
    let path = lobby_to_odd_hours_path(&manifest).map_err(|error| error.to_string())?;
    let timing = if lighting_preview {
        ProofTiming::lighting_preview()
    } else {
        ProofTiming::production()
    };
    let frames_dir = output_dir.join("frames");
    if frames_dir.exists() {
        std::fs::remove_dir_all(&frames_dir).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&frames_dir).map_err(|error| error.to_string())?;

    let asset_root = project_root.join("assets");
    let mut app = App::new();
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
    .insert_resource(ClearColor(Color::srgb(0.018, 0.024, 0.035)))
    .insert_resource(FallbackErrorHandler(tolerate_startup_resource_error))
    .insert_resource(manifest.clone())
    .insert_resource(ConnectedWorldRuntime::default())
    .insert_resource(ProofActorRuntime::default())
    .insert_resource(ProofProgress::default());
    app.init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>();

    let capture_image = {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let mut image = Image::new(
            Extent3d {
                width: timing.width,
                height: timing.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![0; (timing.width * timing.height * 4) as usize],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.texture_descriptor.usage = TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::COPY_SRC
            | TextureUsages::TEXTURE_BINDING;
        images.add(image)
    };

    spawn_connected_world(
        app.world_mut(),
        &manifest,
        timing,
        capture_image.clone(),
        lighting_preview,
    );
    app.insert_resource(ProofPlan {
        timing,
        path: path.clone(),
        frames_dir: frames_dir.clone(),
        capture_image,
        lighting_preview,
    });
    app.add_systems(Update, advance_connected_world_proof);
    app.add_observer(on_world_instance_ready);
    app.add_observer(on_proof_actor_ready);
    app.add_observer(on_proof_frame_captured);

    app.finish();
    app.cleanup();
    let mut readiness_updates = 0usize;
    while app.world().get_resource::<RenderDevice>().is_none() && readiness_updates < 600 {
        app.update();
        readiness_updates += 1;
    }
    if app.world().get_resource::<RenderDevice>().is_none() {
        return Err("Bevy renderer did not initialize a GPU RenderDevice".into());
    }

    let mut guard = 0usize;
    let mut last_captured = 0u32;
    let mut stalled = 0usize;
    loop {
        let captured = app.world().resource::<ProofProgress>().captured;
        if captured >= timing.frames {
            break;
        }
        if let Some(error) = app
            .world()
            .resource::<ConnectedWorldRuntime>()
            .failed
            .clone()
        {
            return Err(error);
        }
        if guard > timing.frames as usize * 16 + 1600 {
            return Err(format!(
                "connected-world capture stalled at {captured}/{} frames",
                timing.frames
            ));
        }
        if captured > last_captured {
            last_captured = captured;
            stalled = 0;
        } else {
            stalled += 1;
        }
        if stalled > 360 {
            let mut progress = app.world_mut().resource_mut::<ProofProgress>();
            progress.queue.clear();
            progress.requested = progress.captured;
            stalled = 0;
        }
        app.update();
        guard += 1;
    }

    let runtime = app.world().resource::<ConnectedWorldRuntime>();
    let captured = app.world().resource::<ProofProgress>().captured;
    let summary = ConnectedWorldProofSummary {
        module_id: manifest.module_id.clone(),
        runtime_glb: manifest.runtime_glb.to_string_lossy().into_owned(),
        registry_version: contract.registry_version,
        renderer: "Bevy 0.19 GPU offscreen screenshot readback".into(),
        timing,
        captured_frames: captured,
        named_world_nodes: runtime.named_nodes.len(),
        hidden_semantic_helpers: runtime.hidden_helpers,
        runtime_controls_bound: runtime.controls_bound,
        runtime_lights_spawned: manifest.lighting.len(),
        staging_marks_registered: manifest.staging_marks.len(),
        camera_anchors_registered: manifest.camera_anchors.len(),
        interactions_registered: manifest.interactions.len(),
        transitions_registered: manifest
            .interactions
            .iter()
            .filter(|point| point.id.starts_with("TRANSITION_"))
            .count(),
        collision_proxies_registered: manifest.collision_groups.len(),
        actor_asset: "assets/characters/mara.glb".into(),
        locomotion: "KayKit native walk clip 72 sampled deterministically over authored path-warped staging marks".into(),
        material_contract: "Blender master GLB materials and object transforms loaded directly; no Rust environment duplicate".into(),
        output_mp4: output_dir
            .join(if lighting_preview {
                "lighting_preview.mp4"
            } else {
                "lobby_to_odd_hours.mp4"
            })
            .to_string_lossy()
            .into_owned(),
    };

    std::fs::write(
        output_dir.join("path_and_marks.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "module_id": manifest.module_id,
            "path": path,
            "interactions": manifest.interactions,
            "camera_schedule": [
                {"start":0.0,"end":5.0,"anchor":"CAM_MASTER_LOBBY"},
                {"start":5.0,"end":10.0,"anchor":"CAM_MASTER_ENTRANCE"},
                {"start":10.0,"end":15.0,"anchor":"CAM_SIDEWALK_TWO_SHOT"},
                {"start":15.0,"end":20.0,"anchor":"CAM_MASTER_STORE"}
            ]
        }))
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(
        output_dir.join("runtime_scene_report.json"),
        serde_json::to_vec_pretty(&summary).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    let output_mp4 = PathBuf::from(&summary.output_mp4);
    let pattern = frames_dir.join("frame_%06d.png");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-framerate",
            &timing.fps.to_string(),
            "-i",
            &pattern.to_string_lossy(),
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            &output_mp4.to_string_lossy(),
        ])
        .status()
        .map_err(|error| format!("failed to start ffmpeg: {error}"))?;
    if !status.success() || !output_mp4.is_file() {
        return Err("ffmpeg failed to encode connected-world proof".into());
    }
    Ok(summary)
}

fn spawn_connected_world(
    world: &mut World,
    manifest: &ConnectedWorldManifest,
    timing: ProofTiming,
    capture_image: Handle<Image>,
    lighting_preview: bool,
) {
    let asset_server = world.resource::<AssetServer>().clone();
    let world_path = manifest
        .runtime_glb
        .to_string_lossy()
        .replace('\\', "/")
        .strip_prefix("assets/")
        .unwrap_or(&manifest.runtime_glb.to_string_lossy())
        .to_string();
    let scene: Handle<WorldAsset> =
        asset_server.load(GltfAssetLabel::Scene(0).from_asset(world_path));
    world.spawn((WorldAssetRoot(scene), ConnectedWorldRoot));

    for (kind, points) in [
        ("socket", manifest.sockets.as_slice()),
        ("staging_mark", manifest.staging_marks.as_slice()),
        ("camera_anchor", manifest.camera_anchors.as_slice()),
        ("interaction", manifest.interactions.as_slice()),
        ("collision", manifest.collision_groups.as_slice()),
        ("cutaway", manifest.cutaway_groups.as_slice()),
    ] {
        for point in points {
            world.spawn((
                SemanticRegistration {
                    id: point.id.clone(),
                    kind: kind.to_string(),
                },
                Transform::from_translation(Vec3::from_array(point.position)),
                Visibility::Hidden,
            ));
        }
    }
    for light in &manifest.lighting {
        spawn_runtime_light(world, light);
    }
    world.spawn(AmbientLight {
        color: Color::srgb(0.18, 0.22, 0.30),
        brightness: 85.0,
        affects_lightmapped_meshes: false,
    });

    let actor_scene: Handle<WorldAsset> =
        asset_server.load(GltfAssetLabel::Scene(0).from_asset("characters/mara.glb"));
    let clips = (0..76)
        .map(|index| {
            asset_server.load(GltfAssetLabel::Animation(index).from_asset("characters/mara.glb"))
        })
        .collect::<Vec<Handle<AnimationClip>>>();
    let (graph, nodes) = AnimationGraph::from_clips(clips.clone());
    let graph = world.resource_mut::<Assets<AnimationGraph>>().add(graph);
    world.spawn((
        WorldAssetRoot(actor_scene),
        Transform::from_scale(Vec3::splat(0.82)),
        ProofActorRoot,
        PendingProofActor {
            graph,
            nodes,
            clips,
        },
    ));

    let aspect = timing.width as f32 / timing.height as f32;
    world.spawn((
        CameraRenderGraph::new(Core3d),
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.018, 0.024, 0.035)),
            ..default()
        },
        RenderTarget::Image(capture_image.into()),
        Projection::Perspective(PerspectiveProjection {
            fov: 46.0_f32.to_radians(),
            aspect_ratio: aspect,
            near: 0.08,
            far: 160.0,
            near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -0.08),
        }),
        ProofCamera,
        Transform::from_xyz(0.0, 1.7, -8.0).looking_at(Vec3::new(0.0, 1.2, -11.0), Vec3::Y),
    ));

    if lighting_preview {
        info!("connected-world lighting preview mode enabled");
    }
}

fn spawn_runtime_light(world: &mut World, intent: &RuntimeLightIntent) {
    let color = Color::srgb(
        intent.color_rgb[0].clamp(0.0, 1.0),
        intent.color_rgb[1].clamp(0.0, 1.0),
        intent.color_rgb[2].clamp(0.0, 1.0),
    );
    let position = Vec3::from_array(intent.position);
    let direction = Vec3::from_array(intent.direction).normalize_or_zero();
    match intent.light_type.as_str() {
        "directional" => {
            world.spawn((
                DirectionalLight {
                    color,
                    illuminance: intent.intensity,
                    shadow_maps_enabled: true,
                    ..default()
                },
                Transform::from_translation(position).looking_to(
                    if direction.length_squared() > 0.5 {
                        direction
                    } else {
                        -Vec3::Y
                    },
                    Vec3::Y,
                ),
                Name::new(intent.id.clone()),
            ));
        }
        "spot" => {
            world.spawn((
                SpotLight {
                    color,
                    intensity: intent.intensity,
                    range: intent.range,
                    outer_angle: intent.spot_angle_degrees.unwrap_or(45.0).to_radians(),
                    inner_angle: intent.spot_angle_degrees.unwrap_or(45.0).to_radians() * 0.72,
                    shadow_maps_enabled: true,
                    ..default()
                },
                Transform::from_translation(position).looking_to(
                    if direction.length_squared() > 0.5 {
                        direction
                    } else {
                        -Vec3::Y
                    },
                    Vec3::Y,
                ),
                Name::new(intent.id.clone()),
            ));
        }
        _ => {
            world.spawn((
                PointLight {
                    color,
                    intensity: intent.intensity,
                    range: intent.range,
                    radius: 0.35,
                    shadow_maps_enabled: true,
                    ..default()
                },
                Transform::from_translation(position),
                Name::new(intent.id.clone()),
            ));
        }
    }
}

fn on_world_instance_ready(
    trigger: On<WorldInstanceReady>,
    mut commands: Commands,
    roots: Query<(), With<ConnectedWorldRoot>>,
    children: Query<&Children>,
    names: Query<&Name>,
    manifest: Res<ConnectedWorldManifest>,
    mut runtime: ResMut<ConnectedWorldRuntime>,
) {
    if roots.get(trigger.entity).is_err() {
        return;
    }
    let mut names_by_entity = HashMap::new();
    for entity in std::iter::once(trigger.entity).chain(children.iter_descendants(trigger.entity)) {
        if let Ok(name) = names.get(entity) {
            names_by_entity.insert(entity, name.as_str().to_string());
        }
    }
    runtime.named_nodes = names_by_entity
        .iter()
        .map(|(entity, name)| (name.clone(), *entity))
        .collect();
    let helper_prefixes = [
        "MARK_",
        "CAM_",
        "INTERACT_",
        "TRANSITION_",
        "COLLIDER_",
        "SOCKET_",
        "LIGHT_",
    ];
    for (entity, name) in &names_by_entity {
        if helper_prefixes
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            commands.entity(*entity).insert(Visibility::Hidden);
            commands
                .entity(*entity)
                .remove::<Camera>()
                .remove::<Camera3d>();
            runtime.hidden_helpers += 1;
        }
    }
    let control_nodes = manifest
        .runtime_controls
        .iter()
        .map(|control| control.node.as_str())
        .collect::<HashSet<_>>();
    for control in &manifest.runtime_controls {
        let Some(entity) = runtime.named_nodes.get(&control.node).copied() else {
            runtime.failed = Some(format!(
                "connected master GLB is missing runtime control node {}",
                control.node
            ));
            return;
        };
        if control.kind == "door" && control.default_state == "open" {
            commands.entity(entity).insert(Visibility::Hidden);
        }
        runtime.controls_bound += 1;
    }
    if control_nodes.len() != manifest.runtime_controls.len() {
        runtime.failed = Some("runtime control nodes are not unique".into());
        return;
    }
    runtime.ready = true;
    info!(
        "connected master '{}' ready with {} named nodes and {} controls",
        manifest.module_id,
        runtime.named_nodes.len(),
        runtime.controls_bound
    );
}

fn on_proof_actor_ready(
    trigger: On<WorldInstanceReady>,
    mut commands: Commands,
    pending: Query<&PendingProofActor>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut players: Query<&mut AnimationPlayer>,
    mut runtime: ResMut<ProofActorRuntime>,
) {
    let Ok(pending) = pending.get(trigger.entity) else {
        return;
    };
    let mut attached = false;
    for entity in children.iter_descendants(trigger.entity) {
        if names
            .get(entity)
            .map(|name| is_hand_prop(name.as_str()))
            .unwrap_or(false)
        {
            commands.entity(entity).despawn();
            continue;
        }
        if players.get_mut(entity).is_ok() {
            commands.entity(entity).insert((
                AnimationGraphHandle(pending.graph.clone()),
                ProofActorPlayer {
                    nodes: pending.nodes.clone(),
                    clips: pending.clips.clone(),
                },
            ));
            attached = true;
        }
    }
    if attached {
        runtime.ready = true;
        commands
            .entity(trigger.entity)
            .remove::<PendingProofActor>();
    }
}

fn is_hand_prop(name: &str) -> bool {
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

fn advance_connected_world_proof(
    mut commands: Commands,
    plan: Res<ProofPlan>,
    mut progress: ResMut<ProofProgress>,
    world_runtime: Res<ConnectedWorldRuntime>,
    actor_runtime: Res<ProofActorRuntime>,
    manifest: Res<ConnectedWorldManifest>,
    clips: Res<Assets<AnimationClip>>,
    mut actors: Query<&mut Transform, (With<ProofActorRoot>, Without<ProofCamera>)>,
    mut players: Query<(&ProofActorPlayer, &mut AnimationPlayer)>,
    mut cameras: Query<
        (&mut Transform, &mut Projection),
        (With<ProofCamera>, Without<ProofActorRoot>),
    >,
    mut warmup: Local<u32>,
) {
    if !world_runtime.ready || !actor_runtime.ready || !progress.queue.is_empty() {
        return;
    }
    if players.iter().any(|(player, _)| {
        player
            .clips
            .iter()
            .any(|handle| clips.get(handle).is_none())
    }) {
        return;
    }
    let frame_index = progress.requested.min(plan.timing.frames.saturating_sub(1));
    let time = frame_index as f32 / plan.timing.fps as f32;
    let walk_normalized = if plan.lighting_preview {
        0.62
    } else {
        ((time - 1.0) / 17.0).clamp(0.0, 1.0)
    };
    let Ok(sample) = sample_proof_path(&plan.path, walk_normalized) else {
        return;
    };
    if let Ok(mut actor) = actors.single_mut() {
        actor.translation = Vec3::from_array(sample.position) + Vec3::Y * 0.02;
        actor.rotation = Quat::from_rotation_y(sample.forward.x.atan2(sample.forward.z));
        actor.scale = Vec3::splat(0.82);
    }
    let walking = !plan.lighting_preview && (1.0..18.0).contains(&time);
    for (player, mut animation) in &mut players {
        let clip_index = if walking { 72 } else { 36 };
        if let Some(node) = player.nodes.get(clip_index).copied() {
            animation.stop_all();
            animation
                .play(node)
                .repeat()
                .set_speed(0.0)
                .seek_to(if walking { time + 0.25 } else { 0.05 });
        }
    }
    if let Ok((mut camera, mut projection)) = cameras.single_mut() {
        let anchor_id = if plan.lighting_preview {
            "CAM_MASTER_STREET_WIDE"
        } else {
            proof_camera_anchor_at(time)
        };
        if let Some(anchor) = manifest.camera(anchor_id) {
            let actor_target = Vec3::from_array(sample.position) + Vec3::Y * 1.22;
            let anchor_eye = Vec3::from_array(anchor.position);
            let shot_start = proof_camera_shot_start(time);
            let shot_start_normalized = ((shot_start - 1.0) / 17.0).clamp(0.0, 1.0);
            let shot_start_target = sample_proof_path(&plan.path, shot_start_normalized)
                .map(|sample| Vec3::from_array(sample.position) + Vec3::Y * 1.22)
                .unwrap_or(actor_target);
            let mut authored_offset = anchor_eye - shot_start_target;
            let distance = authored_offset.length();
            if distance > 0.001 {
                authored_offset = authored_offset.normalize() * distance.clamp(4.0, 8.0);
            } else {
                authored_offset = Vec3::new(-3.0, 1.2, 3.0);
            }
            // Preserve each Blender-authored eyeline as a tracking offset so the
            // performer cannot walk through a fixed camera during the long route.
            camera.translation =
                actor_target + authored_offset + Vec3::Y * (0.05 * (time * 0.7).sin());
            camera.look_at(actor_target, Vec3::Y);
            if let Projection::Perspective(perspective) = projection.as_mut() {
                let lens = anchor
                    .metadata
                    .get("lens_mm")
                    .and_then(|value| value.as_f64())
                    .unwrap_or(42.0) as f32;
                perspective.fov = 2.0 * (18.0 / lens.max(1.0)).atan();
            }
        }
    }
    if progress.requested == 0 && *warmup < 100 {
        *warmup += 1;
        return;
    }
    if progress.requested < plan.timing.frames {
        commands.spawn((
            ProofCaptureMarker,
            Screenshot(RenderTarget::Image(plan.capture_image.clone().into())),
        ));
        let requested = progress.requested;
        progress.queue.push_back(requested);
        progress.requested = requested + 1;
    }
}

fn on_proof_frame_captured(
    trigger: On<ScreenshotCaptured>,
    mut progress: ResMut<ProofProgress>,
    plan: Res<ProofPlan>,
) {
    let event = trigger.event();
    let Some(frame) = progress.queue.pop_front() else {
        return;
    };
    let rgba = to_rgba(
        event.image.data.as_deref().unwrap_or_default(),
        event.image.texture_descriptor.format,
    );
    let path = plan.frames_dir.join(format!("frame_{:06}.png", frame + 1));
    if let Err(error) = write_png(
        &path,
        event.image.texture_descriptor.size.width,
        event.image.texture_descriptor.size.height,
        &rgba,
    ) {
        warn!("connected-world frame write failed: {error}");
    } else {
        progress.captured += 1;
    }
}

fn to_rgba(data: &[u8], format: TextureFormat) -> Vec<u8> {
    match format {
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => data
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]])
            .collect(),
        _ => data.to_vec(),
    }
}

fn tolerate_startup_resource_error(error: BevyError, context: ErrorContext) {
    if error.to_string().contains("Resource does not exist") {
        warn!(
            "tolerating transient startup resource error in {}: {}",
            context.name(),
            error
        );
        return;
    }
    match_severity(error, context);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_camera_schedule_crosses_interior_exterior_and_store() {
        assert_eq!(proof_camera_anchor_at(0.0), "CAM_MASTER_LOBBY");
        assert_eq!(proof_camera_anchor_at(6.0), "CAM_MASTER_ENTRANCE");
        assert_eq!(proof_camera_anchor_at(12.0), "CAM_SIDEWALK_TWO_SHOT");
        assert_eq!(proof_camera_anchor_at(17.0), "CAM_MASTER_STORE");
    }

    #[test]
    fn proof_timing_is_short_and_deterministic() {
        let timing = ProofTiming::production();
        assert!((15.0..=25.0).contains(&timing.duration_secs));
        assert_eq!(timing.frames, timing.fps * timing.duration_secs as u32);
        assert_eq!(timing.width, 960);
        assert_eq!(timing.height, 540);
    }
}
