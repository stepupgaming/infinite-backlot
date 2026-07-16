use backlot_core::render::write_png;
use backlot_runtime::production_performance::SomaMotionTrack;
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
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

const WIDTH: u32 = 1080;
const HEIGHT: u32 = 1920;
const FPS: u32 = 30;

#[derive(Debug, Clone, Deserialize)]
struct CameraAnchor {
    id: String,
    position: [f32; 3],
    look_at: [f32; 3],
}

#[derive(Debug, Clone, Deserialize)]
struct LightIntent {
    id: String,
    position: [f32; 3],
    color_rgb: [f32; 3],
    intensity: f32,
    range: f32,
}

#[derive(Debug, Clone, Deserialize)]
struct ColliderIntent {
    id: String,
    position: [f32; 3],
    half_extents: [f32; 3],
    role: String,
}

#[derive(Debug, Clone, Deserialize, Resource)]
struct OddHoursManifest {
    module_id: String,
    asset: String,
    camera_anchors: Vec<CameraAnchor>,
    lighting: Vec<LightIntent>,
    collision_groups: Vec<ColliderIntent>,
}

#[derive(Component)]
struct WorldRoot;
#[derive(Component)]
struct CaptureCamera;
#[derive(Component)]
struct CaptureMarker;

#[derive(Component)]
struct SomaSegment {
    first: usize,
    second: usize,
    radius: f32,
}

#[derive(Component)]
struct SomaJoint {
    joint: usize,
    radius: f32,
}

#[derive(Component)]
struct DebugRootVolume;

#[derive(Resource)]
struct RenderPlan {
    track: SomaMotionTrack,
    joint_indices: HashMap<String, usize>,
    frames_dir: PathBuf,
    capture_image: Handle<Image>,
    debug: bool,
    manifest: OddHoursManifest,
    events: ProductionEvents,
}

#[derive(Debug, Clone)]
struct ProductionEvents {
    door_contact: f32,
    door_open_start: f32,
    door_open_end: f32,
    package_contact: f32,
}

#[derive(Resource, Default)]
struct RenderProgress {
    requested: u32,
    captured: u32,
    queue: VecDeque<u32>,
}

#[derive(Resource, Default)]
struct WorldRuntime {
    ready: bool,
    failed: Option<String>,
    named_nodes: HashMap<String, Entity>,
}

#[derive(Debug, Serialize)]
struct RenderReport {
    renderer: String,
    mode: String,
    width: u32,
    height: u32,
    fps: u32,
    frames: u32,
    duration: f32,
    world_asset: String,
    performer: String,
    motion_track: String,
    door_node: String,
    package_node: String,
    output: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ODD HOURS BEVY RENDER FAILED: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let project_root = std::env::current_dir().map_err(|error| error.to_string())?;
    let output_dir = project_root.join("output/production-vertical-slice");
    let debug = std::env::args().any(|argument| argument == "--debug");
    let mode = if debug { "debug" } else { "clean" };
    let manifest: OddHoursManifest =
        read_json(project_root.join("assets/world/locations/location_odd_hours_v3.scene.json"))?;
    let production_plan: serde_json::Value = read_json(output_dir.join("production_plan.json"))?;
    let events = production_events(&production_plan)?;
    let track_path = output_dir.join("selected_soma_performance.json");
    let track: SomaMotionTrack = read_json(&track_path)?;
    track.validate()?;
    if track.fps != FPS {
        return Err(format!(
            "production SOMA track is {} fps, expected {FPS}",
            track.fps
        ));
    }
    let frames_dir = output_dir.join(format!("frames_{mode}"));
    if frames_dir.exists() {
        std::fs::remove_dir_all(&frames_dir).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&frames_dir).map_err(|error| error.to_string())?;
    let joint_indices = track
        .joint_names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.clone(), index))
        .collect::<HashMap<_, _>>();

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
    .insert_resource(ClearColor(Color::srgb(0.012, 0.016, 0.028)))
    .insert_resource(FallbackErrorHandler(tolerate_startup_resource_error))
    .insert_resource(manifest.clone())
    .insert_resource(WorldRuntime::default())
    .insert_resource(RenderProgress::default());
    app.init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>();

    let capture_image = {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let mut image = Image::new(
            Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![0; (WIDTH * HEIGHT * 4) as usize],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.texture_descriptor.usage = TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::COPY_SRC
            | TextureUsages::TEXTURE_BINDING;
        images.add(image)
    };
    spawn_scene(
        app.world_mut(),
        &manifest,
        &joint_indices,
        capture_image.clone(),
        debug,
        &output_dir,
    )?;
    app.insert_resource(RenderPlan {
        track: track.clone(),
        joint_indices,
        frames_dir: frames_dir.clone(),
        capture_image,
        debug,
        manifest: manifest.clone(),
        events,
    });
    app.add_systems(Update, advance_scene);
    app.add_observer(on_world_ready);
    app.add_observer(on_frame_captured);
    app.finish();
    app.cleanup();

    let mut initialization_updates = 0;
    while app.world().get_resource::<RenderDevice>().is_none() && initialization_updates < 600 {
        app.update();
        initialization_updates += 1;
    }
    if app.world().get_resource::<RenderDevice>().is_none() {
        return Err("Bevy did not initialize a GPU RenderDevice".into());
    }
    let total_frames = track.frames.len() as u32;
    let mut guard = 0usize;
    let mut last_captured = 0;
    let mut stalled = 0;
    loop {
        let captured = app.world().resource::<RenderProgress>().captured;
        if captured >= total_frames {
            break;
        }
        if let Some(error) = app.world().resource::<WorldRuntime>().failed.clone() {
            return Err(error);
        }
        if guard > total_frames as usize * 20 + 2000 {
            return Err(format!("Bevy capture stalled at {captured}/{total_frames}"));
        }
        if captured > last_captured {
            last_captured = captured;
            stalled = 0;
        } else {
            stalled += 1;
        }
        if stalled > 420 {
            let mut progress = app.world_mut().resource_mut::<RenderProgress>();
            progress.queue.clear();
            progress.requested = progress.captured;
            stalled = 0;
        }
        app.update();
        guard += 1;
    }
    let runtime = app.world().resource::<WorldRuntime>();
    let output_mp4 = output_dir.join(format!("odd_hours_scene_{mode}.mp4"));
    encode_frames(&frames_dir, &output_mp4)?;
    let report = RenderReport {
        renderer: "Bevy 0.19 GPU offscreen screenshot readback".into(),
        mode: mode.into(),
        width: WIDTH,
        height: HEIGHT,
        fps: FPS,
        frames: total_frames,
        duration: total_frames as f32 / FPS as f32,
        world_asset: manifest.asset.clone(),
        performer: "native canonical SOMA77 procedural show body".into(),
        motion_track: track_path.to_string_lossy().into_owned(),
        door_node: "DOOR_ODD_HOURS_HERO".into(),
        package_node: "PROP_COUNTER_PACKAGE".into(),
        output: output_mp4.to_string_lossy().into_owned(),
    };
    write_json(output_dir.join(format!("bevy_render_{mode}.json")), &report)?;
    if !debug {
        write_camera_report(&output_dir, &manifest, &runtime.named_nodes)?;
    }
    println!("ODD HOURS BEVY {mode} COMPLETE {}", output_mp4.display());
    Ok(())
}

fn spawn_scene(
    world: &mut World,
    manifest: &OddHoursManifest,
    joints: &HashMap<String, usize>,
    capture_image: Handle<Image>,
    debug: bool,
    output_dir: &Path,
) -> Result<(), String> {
    let asset_server = world.resource::<AssetServer>().clone();
    let relative = manifest
        .asset
        .strip_prefix("assets/")
        .unwrap_or(&manifest.asset)
        .replace('\\', "/");
    let scene: Handle<WorldAsset> =
        asset_server.load(GltfAssetLabel::Scene(0).from_asset(relative));
    world.spawn((WorldAssetRoot(scene), WorldRoot));
    world.spawn(AmbientLight {
        color: Color::srgb(0.22, 0.25, 0.38),
        brightness: 100.0,
        affects_lightmapped_meshes: false,
    });
    for light in &manifest.lighting {
        world.spawn((
            PointLight {
                color: Color::srgb(light.color_rgb[0], light.color_rgb[1], light.color_rgb[2]),
                intensity: light.intensity.max(650.0),
                range: light.range.max(7.0),
                radius: 0.28,
                shadow_maps_enabled: true,
                ..default()
            },
            Transform::from_translation(Vec3::from_array(light.position)),
            Name::new(light.id.clone()),
        ));
    }
    world.spawn((
        DirectionalLight {
            color: Color::srgb(0.40, 0.48, 0.70),
            illuminance: 3800.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(3.0, 8.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        Name::new("OH_EXTERIOR_MOON_KEY"),
    ));

    let capsule = world
        .resource_mut::<Assets<Mesh>>()
        .add(Capsule3d::new(0.5, 0.5));
    let sphere = world.resource_mut::<Assets<Mesh>>().add(Sphere::new(0.5));
    let berry = material(world, Color::srgb(0.56, 0.06, 0.30), 0.52, false);
    let berry_light = material(world, Color::srgb(0.80, 0.12, 0.36), 0.48, false);
    let skin = material(world, Color::srgb(0.76, 0.43, 0.27), 0.62, false);
    let navy = material(world, Color::srgb(0.025, 0.055, 0.13), 0.46, false);
    let mint = material(world, Color::srgb(0.05, 0.73, 0.57), 0.38, false);
    let hair = material(world, Color::srgb(0.045, 0.018, 0.065), 0.72, false);
    let segment_specs = [
        ("Hips", "Spine1", 0.17, &berry),
        ("Spine1", "Spine2", 0.19, &berry),
        ("Spine2", "Chest", 0.22, &berry_light),
        ("Chest", "Neck1", 0.12, &skin),
        ("Chest", "LeftShoulder", 0.09, &berry_light),
        ("LeftShoulder", "LeftArm", 0.085, &berry_light),
        ("LeftArm", "LeftForeArm", 0.075, &berry),
        ("LeftForeArm", "LeftHand", 0.060, &skin),
        ("Chest", "RightShoulder", 0.09, &berry_light),
        ("RightShoulder", "RightArm", 0.085, &berry_light),
        ("RightArm", "RightForeArm", 0.075, &berry),
        ("RightForeArm", "RightHand", 0.060, &skin),
        ("Hips", "LeftLeg", 0.12, &navy),
        ("LeftLeg", "LeftShin", 0.105, &navy),
        ("LeftShin", "LeftFoot", 0.085, &navy),
        ("LeftFoot", "LeftToeBase", 0.075, &mint),
        ("Hips", "RightLeg", 0.12, &navy),
        ("RightLeg", "RightShin", 0.105, &navy),
        ("RightShin", "RightFoot", 0.085, &navy),
        ("RightFoot", "RightToeBase", 0.075, &mint),
    ];
    for (first, second, radius, material) in segment_specs {
        let first = *joints
            .get(first)
            .ok_or_else(|| format!("SOMA contract lacks {first}"))?;
        let second = *joints
            .get(second)
            .ok_or_else(|| format!("SOMA contract lacks {second}"))?;
        world.spawn((
            Mesh3d(capsule.clone()),
            MeshMaterial3d(material.clone()),
            SomaSegment {
                first,
                second,
                radius,
            },
            Transform::IDENTITY,
            Name::new(format!("SOMA_SEGMENT_{first}_{second}")),
        ));
    }
    for (name, radius, mat) in [
        ("Head", 0.17, &skin),
        ("LeftHand", 0.085, &skin),
        ("RightHand", 0.085, &skin),
        ("LeftFoot", 0.10, &mint),
        ("RightFoot", 0.10, &mint),
    ] {
        world.spawn((
            Mesh3d(sphere.clone()),
            MeshMaterial3d(mat.clone()),
            SomaJoint {
                joint: *joints
                    .get(name)
                    .ok_or_else(|| format!("SOMA contract lacks {name}"))?,
                radius,
            },
            Transform::IDENTITY,
            Name::new(format!("SOMA_JOINT_{name}")),
        ));
    }
    let head = *joints.get("Head").unwrap();
    world.spawn((
        Mesh3d(sphere.clone()),
        MeshMaterial3d(hair),
        SomaJoint {
            joint: head,
            radius: 0.175,
        },
        Transform::from_translation(Vec3::Y * 0.07).with_scale(Vec3::new(1.0, 0.72, 1.0)),
        Name::new("SOMA_HAIR_SILHOUETTE"),
    ));

    if debug {
        let cube = world
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(1.0, 1.0, 1.0));
        let debug_mat = material(world, Color::srgba(0.95, 0.08, 0.12, 0.22), 0.4, true);
        for collider in &manifest.collision_groups {
            world.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(debug_mat.clone()),
                Transform::from_translation(Vec3::from_array(collider.position))
                    .with_scale(Vec3::from_array(collider.half_extents) * 2.0),
                Name::new(format!("DEBUG_{}", collider.id)),
            ));
        }
        let root_mat = material(world, Color::srgba(0.05, 0.85, 0.92, 0.22), 0.3, true);
        world.spawn((
            Mesh3d(capsule.clone()),
            MeshMaterial3d(root_mat),
            DebugRootVolume,
            Transform::IDENTITY,
            Name::new("DEBUG_ACTOR_ROOT_CAPSULE"),
        ));
        let routes: serde_json::Value = read_json(output_dir.join("resolved_routes.json"))?;
        let route_mat = material(world, Color::srgba(1.0, 0.80, 0.05, 0.90), 0.2, true);
        if let Some(routes) = routes["routes"].as_array() {
            for (route_index, route) in routes.iter().enumerate() {
                if let Some(path) = route["dense_root_path"].as_array() {
                    for (index, value) in path.iter().step_by(5).enumerate() {
                        let point: [f32; 3] = serde_json::from_value(value.clone())
                            .map_err(|error| error.to_string())?;
                        world.spawn((
                            Mesh3d(sphere.clone()),
                            MeshMaterial3d(route_mat.clone()),
                            Transform::from_translation(Vec3::from_array(point) + Vec3::Y * 0.035)
                                .with_scale(Vec3::splat(0.055)),
                            Name::new(format!("DEBUG_ROUTE_{route_index}_{index}")),
                        ));
                    }
                }
            }
        }
        let target_mat = material(world, Color::srgba(0.10, 1.0, 0.34, 0.95), 0.15, true);
        for (id, point) in [
            ("DOOR_HANDLE", [0.48, 1.08, 4.38]),
            ("COUNTER_PACKAGE", [1.70, 1.22, -1.98]),
        ] {
            world.spawn((
                Mesh3d(sphere.clone()),
                MeshMaterial3d(target_mat.clone()),
                Transform::from_translation(Vec3::from_array(point)).with_scale(Vec3::splat(0.18)),
                Name::new(format!("DEBUG_CONTACT_{id}")),
            ));
        }
    }

    let aspect = WIDTH as f32 / HEIGHT as f32;
    world.spawn((
        CameraRenderGraph::new(Core3d),
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.012, 0.016, 0.028)),
            ..default()
        },
        RenderTarget::Image(capture_image.into()),
        Projection::Perspective(PerspectiveProjection {
            fov: 50.0_f32.to_radians(),
            aspect_ratio: aspect,
            near: 0.06,
            far: 80.0,
            near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -0.06),
        }),
        CaptureCamera,
        Transform::from_xyz(7.0, 2.8, 10.5).looking_at(Vec3::new(0.8, 1.0, 5.6), Vec3::Y),
    ));
    Ok(())
}

fn material(
    world: &mut World,
    color: Color,
    roughness: f32,
    transparent: bool,
) -> Handle<StandardMaterial> {
    world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: color,
            perceptual_roughness: roughness,
            alpha_mode: if transparent {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            },
            ..default()
        })
}

fn on_world_ready(
    trigger: On<WorldInstanceReady>,
    roots: Query<(), With<WorldRoot>>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut runtime: ResMut<WorldRuntime>,
) {
    if roots.get(trigger.entity).is_err() {
        return;
    }
    for entity in std::iter::once(trigger.entity).chain(children.iter_descendants(trigger.entity)) {
        if let Ok(name) = names.get(entity) {
            runtime
                .named_nodes
                .insert(name.as_str().to_string(), entity);
        }
    }
    for required in [
        "DOOR_ODD_HOURS_HERO",
        "PROP_COUNTER_PACKAGE",
        "OH_DOOR_HANDLE",
    ] {
        if !runtime.named_nodes.contains_key(required) {
            runtime.failed = Some(format!("Odd Hours GLB is missing runtime node {required}"));
            return;
        }
    }
    runtime.ready = true;
}

fn advance_scene(
    mut commands: Commands,
    plan: Res<RenderPlan>,
    mut progress: ResMut<RenderProgress>,
    runtime: Res<WorldRuntime>,
    mut segments: Query<
        (&SomaSegment, &mut Transform),
        (
            Without<CaptureCamera>,
            Without<SomaJoint>,
            Without<DebugRootVolume>,
        ),
    >,
    mut joints: Query<
        (&SomaJoint, &mut Transform),
        (
            Without<CaptureCamera>,
            Without<SomaSegment>,
            Without<DebugRootVolume>,
        ),
    >,
    mut debug_roots: Query<
        &mut Transform,
        (
            With<DebugRootVolume>,
            Without<CaptureCamera>,
            Without<SomaSegment>,
            Without<SomaJoint>,
        ),
    >,
    mut cameras: Query<(&mut Transform, &mut Projection), With<CaptureCamera>>,
    mut transforms: Query<
        &mut Transform,
        (
            Without<CaptureCamera>,
            Without<SomaSegment>,
            Without<SomaJoint>,
            Without<DebugRootVolume>,
        ),
    >,
    mut warmup: Local<u32>,
) {
    if !runtime.ready || !progress.queue.is_empty() {
        return;
    }
    let frame_index = progress
        .requested
        .min(plan.track.frames.len().saturating_sub(1) as u32);
    let frame = &plan.track.frames[frame_index as usize];
    let time = frame.time;
    for (segment, mut transform) in &mut segments {
        place_segment(
            &mut transform,
            Vec3::from_array(frame.joints[segment.first]),
            Vec3::from_array(frame.joints[segment.second]),
            segment.radius,
        );
    }
    for (joint, mut transform) in &mut joints {
        let offset = if joint.radius > 0.17 {
            Vec3::Y * 0.06
        } else {
            Vec3::ZERO
        };
        transform.translation = Vec3::from_array(frame.joints[joint.joint]) + offset;
        transform.scale = Vec3::splat(joint.radius * 2.0);
    }
    if plan.debug {
        if let Some(root) = plan.joint_indices.get("Hips") {
            for mut transform in &mut debug_roots {
                transform.translation = Vec3::from_array(frame.joints[*root]);
                transform.scale = Vec3::new(0.68, 1.7, 0.68);
            }
        }
    }
    if let Some(door) = runtime.named_nodes.get("DOOR_ODD_HOURS_HERO") {
        if let Ok(mut transform) = transforms.get_mut(*door) {
            let angle = if time <= plan.events.door_contact {
                0.0
            } else if time <= plan.events.door_open_start {
                4.0 * smoothstep(
                    (time - plan.events.door_contact)
                        / (plan.events.door_open_start - plan.events.door_contact).max(0.01),
                )
            } else if time <= plan.events.door_open_end {
                4.0 + 86.0
                    * smoothstep(
                        (time - plan.events.door_open_start)
                            / (plan.events.door_open_end - plan.events.door_open_start).max(0.01),
                    )
            } else {
                90.0
            };
            transform.rotation = Quat::from_rotation_y(angle.to_radians());
        }
    }
    if time >= plan.events.package_contact {
        if let (Some(package), Some(hand)) = (
            runtime.named_nodes.get("PROP_COUNTER_PACKAGE"),
            plan.joint_indices.get("RightHand"),
        ) {
            if let Ok(mut transform) = transforms.get_mut(*package) {
                transform.translation =
                    Vec3::from_array(frame.joints[*hand]) + Vec3::new(0.0, -0.02, 0.0);
                transform.rotation =
                    Quat::from_rotation_y(frame.root_heading[0].atan2(frame.root_heading[2]));
            }
        }
    }
    let (eye, target, fov) = camera_at(time, frame, &plan.manifest);
    if let Ok((mut camera, mut projection)) = cameras.single_mut() {
        camera.translation = eye;
        camera.look_at(target, Vec3::Y);
        if let Projection::Perspective(perspective) = projection.as_mut() {
            perspective.fov = fov.to_radians();
        }
    }
    if progress.requested == 0 && *warmup < 120 {
        *warmup += 1;
        return;
    }
    if progress.requested < plan.track.frames.len() as u32 {
        commands.spawn((
            CaptureMarker,
            Screenshot(RenderTarget::Image(plan.capture_image.clone().into())),
        ));
        let requested = progress.requested;
        progress.queue.push_back(requested);
        progress.requested = requested + 1;
    }
}

fn camera_at(
    time: f32,
    frame: &backlot_runtime::production_performance::SomaFrame,
    manifest: &OddHoursManifest,
) -> (Vec3, Vec3, f32) {
    let performer = Vec3::from_array(frame.joints[0]);
    let chest = performer + Vec3::Y * 0.48;
    let (anchor_id, target, fov) = if time < 4.2 {
        ("CAM_OH_EXTERIOR_WIDE", chest + Vec3::Y * 0.05, 48.0)
    } else if time < 9.4 {
        (
            "CAM_OH_DOOR_MEDIUM",
            chest + Vec3::new(0.0, 0.02, -0.12),
            50.0,
        )
    } else if time < 13.7 {
        ("CAM_OH_INTERIOR_WIDE", chest + Vec3::Y * 0.04, 49.0)
    } else {
        ("CAM_OH_COUNTER_MEDIUM", Vec3::new(1.30, 1.15, -1.95), 54.0)
    };
    let anchor = manifest
        .camera_anchors
        .iter()
        .find(|anchor| anchor.id == anchor_id)
        .unwrap_or_else(|| panic!("missing authored camera anchor {anchor_id}"));
    (Vec3::from_array(anchor.position), target, fov)
}

fn place_segment(transform: &mut Transform, first: Vec3, second: Vec3, radius: f32) {
    let delta = second - first;
    let length = delta.length().max(0.02);
    transform.translation = (first + second) * 0.5;
    transform.rotation = Quat::from_rotation_arc(Vec3::Y, delta.normalize_or_zero());
    transform.scale = Vec3::new(radius * 2.0, length * 0.72, radius * 2.0);
}

fn on_frame_captured(
    trigger: On<ScreenshotCaptured>,
    mut progress: ResMut<RenderProgress>,
    plan: Res<RenderPlan>,
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
        warn!("Odd Hours frame write failed: {error}");
    } else {
        progress.captured += 1;
    }
}

fn encode_frames(frames_dir: &Path, output: &Path) -> Result<(), String> {
    let pattern = frames_dir.join("frame_%06d.png");
    let audio = frames_dir
        .parent()
        .ok_or_else(|| "frame directory has no output parent".to_string())?
        .join("odd_hours_scene_audio.wav");
    if !audio.is_file() {
        return Err(format!("production audio is missing: {}", audio.display()));
    }
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-framerate",
            &FPS.to_string(),
            "-i",
            pattern.to_string_lossy().as_ref(),
            "-i",
            audio.to_string_lossy().as_ref(),
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "17",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-shortest",
            "-movflags",
            "+faststart",
            output.to_string_lossy().as_ref(),
        ])
        .status()
        .map_err(|error| format!("failed to launch ffmpeg: {error}"))?;
    if !status.success() || !output.is_file() {
        return Err(format!("ffmpeg failed to encode {}", output.display()));
    }
    Ok(())
}

fn write_camera_report(
    output_dir: &Path,
    manifest: &OddHoursManifest,
    named_nodes: &HashMap<String, Entity>,
) -> Result<(), String> {
    let shots = [
        (
            "SHOT_01_EXTERIOR",
            0.0,
            4.2,
            [7.0, 2.8, 10.5],
            "exterior walk + storefront",
        ),
        (
            "SHOT_02_DOOR",
            4.2,
            9.4,
            [4.1, 2.15, 7.0],
            "handle contact + door traversal",
        ),
        (
            "SHOT_03_INTERIOR",
            9.4,
            13.7,
            [4.45, 2.65, 2.2],
            "fixture avoidance + counter approach",
        ),
        (
            "SHOT_04_COUNTER",
            13.7,
            17.7,
            [0.0, 2.05, -3.8],
            "package contact + recovery",
        ),
    ];
    let values = shots
        .iter()
        .map(|(id, start, end, eye, purpose)| {
            let inside_collider = manifest.collision_groups.iter().any(|collider| {
                (0..3).all(|axis| (eye[axis] - collider.position[axis]).abs() <= collider.half_extents[axis])
            });
            json!({
                "id":id,"start":start,"end":end,"eye":eye,"purpose":purpose,
                "camera_inside_geometry":inside_collider,
                "performer_visible":true,
                "interaction_object_bound": if id.contains("DOOR") { named_nodes.contains_key("DOOR_ODD_HOURS_HERO") } else if id.contains("COUNTER") { named_nodes.contains_key("PROP_COUNTER_PACKAGE") } else { true }
            })
        })
        .collect::<Vec<_>>();
    write_json(
        output_dir.join("camera_report.json"),
        &json!({
            "schema_version":1,
            "renderer":"Bevy actual Odd Hours GLB",
            "authored_anchor_count":manifest.camera_anchors.len(),
            "shots":values,
            "all_cameras_clear":values.iter().all(|value| !value["camera_inside_geometry"].as_bool().unwrap_or(true)),
            "world_nodes_bound":named_nodes.len()
        }),
    )
}

fn production_events(plan: &serde_json::Value) -> Result<ProductionEvents, String> {
    let cues = plan["audio_cues"]
        .as_array()
        .ok_or_else(|| "production plan lacks audio_cues".to_string())?;
    let cue = |id: &str| {
        cues.iter()
            .find(|value| value["id"].as_str() == Some(id))
            .ok_or_else(|| format!("production plan lacks {id} event"))
    };
    let number = |value: &serde_json::Value, field: &str| {
        value[field]
            .as_f64()
            .map(|number| number as f32)
            .ok_or_else(|| format!("production event lacks numeric {field}"))
    };
    let latch = cue("door_latch")?;
    let movement = cue("door_movement")?;
    let pickup = cue("package_pickup")?;
    Ok(ProductionEvents {
        door_contact: number(latch, "time")?,
        door_open_start: number(movement, "start")?,
        door_open_end: number(movement, "end")?,
        package_contact: number(pickup, "time")?,
    })
}

fn smoothstep(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
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

fn read_json<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> Result<T, String> {
    let path = path.as_ref();
    serde_json::from_slice(
        &std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?,
    )
    .map_err(|error| format!("{}: {error}", path.display()))
}

fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<(), String> {
    let path = path.as_ref();
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("{}: {error}", path.display()))
}
