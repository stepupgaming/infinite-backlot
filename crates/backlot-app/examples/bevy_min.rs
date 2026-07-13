//! Minimal end-to-end Bevy offscreen-capture smoke test.
//!
//! Reproduces the production capture setup WITHOUT the LLM authoring or the
//! scene rigs, to validate that the renderer initialization + readiness +
//! offscreen `Screenshot` readback pipeline actually produces frames. Mirrors
//! the lifecycle Bevy's own runner uses: `finish()` + `cleanup()` before the
//! `app.update()` loop (calling `update()` alone never runs `cleanup()`, which
//! is what spawns the render thread and inserts `RenderAppChannels`).
//!
//! Run with `RUST_BACKTRACE=1` to surface any panic.

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::ecs::error::{match_severity, BevyError, ErrorContext, FallbackErrorHandler};
use bevy::log::warn;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::renderer::RenderDevice;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};

/// Targeted error handler: tolerate ONLY the transient "Resource does not exist"
/// validation errors that occur while the GPU `RenderDevice` is still being
/// unpacked into the main world on the first `update()` calls. Every other error
/// (including real panics) defers to `match_severity` and aborts.
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

#[derive(Resource, Default)]
struct Progress {
    requested: u32,
    captured: u32,
}

#[derive(Resource)]
struct CaptureImage(Handle<Image>);

#[derive(Component)]
struct CaptureMarker;

fn main() {
    eprintln!("[bevy_min] building app");
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(bevy::window::WindowPlugin {
        primary_window: Some(Window {
            visible: false,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.03, 0.03, 0.05)));

    // Tolerate transient frame-1 device-init resource errors.
    app.insert_resource(FallbackErrorHandler(tolerate_startup_resource_error));

    let rw = 540u32;
    let rh = 960u32;
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

    // Simple scene: a lit cube + camera rendering to the offscreen image.
    {
        let bw = &mut app.world_mut();
        let cube = bw
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(1.0, 1.0, 1.0));
        let cube_mat = bw
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::srgb(0.8, 0.2, 0.2),
                ..default()
            });
        bw.spawn((
            Mesh3d(cube),
            MeshMaterial3d(cube_mat),
            Transform::from_xyz(0.0, 0.5, 0.0),
        ));
        bw.spawn((
            DirectionalLight {
                illuminance: 1500.0,
                ..default()
            },
            Transform::from_xyz(4.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        bw.spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::srgb(0.03, 0.03, 0.05)),
                ..default()
            },
            RenderTarget::Image(cap_handle.clone().into()),
            Projection::Perspective(PerspectiveProjection {
                fov: 50.0_f32.to_radians(),
                aspect_ratio: rw as f32 / rh as f32,
                near: 0.1,
                far: 100.0,
                near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -0.1),
            }),
            Transform::from_xyz(0.0, 3.0, 7.0).looking_at(Vec3::new(0.0, 1.2, 0.0), Vec3::Y),
        ));
    }

    app.insert_resource(Progress::default());
    app.insert_resource(CaptureImage(cap_handle.clone()));
    app.add_systems(Update, request_capture_system);
    app.add_observer(on_captured);

    // --- Bevy lifecycle: finish + cleanup BEFORE the update loop. This is what
    // `App::run` does; without it `RenderAppChannels` is never inserted and the
    // render extract step panics. ---
    eprintln!("[bevy_min] finish + cleanup");
    app.finish();
    app.cleanup();

    // --- Readiness: poll until the render thread has unpacked `RenderDevice`
    // into the main world. ---
    let mut readiness = 0usize;
    while app.world().get_resource::<RenderDevice>().is_none() && readiness < 600 {
        app.update();
        readiness += 1;
    }
    let ready = app.world().get_resource::<RenderDevice>().is_some();
    eprintln!("[bevy_min] RenderDevice ready={ready} after {readiness} updates");

    // --- Capture loop: request + read back a handful of frames. ---
    let n_frames: u32 = 5;
    let mut guard = 0usize;
    loop {
        let captured = app.world().resource::<Progress>().captured;
        if captured >= n_frames {
            break;
        }
        if guard > (n_frames as usize) * 20 + 200 {
            eprintln!("[bevy_min] capture stalled at {captured}/{n_frames}");
            break;
        }
        app.update();
        guard += 1;
    }

    let captured = app.world().resource::<Progress>().captured;
    eprintln!("[bevy_min] CAPTURED {captured}/{n_frames} frames");
    if captured == 0 {
        eprintln!("[bevy_min] FAIL: no frames captured");
        std::process::exit(1);
    }
    eprintln!("[bevy_min] OK");
}

fn request_capture_system(
    mut commands: Commands,
    mut progress: ResMut<Progress>,
    cap: Res<CaptureImage>,
) {
    let req = progress.requested;
    if req < 5 {
        commands.spawn((
            CaptureMarker,
            Screenshot(RenderTarget::Image(cap.0.clone().into())),
        ));
        progress.requested = req + 1;
    }
}

fn on_captured(trigger: On<ScreenshotCaptured>, mut progress: ResMut<Progress>) {
    let _ = trigger;
    progress.captured += 1;
    eprintln!("[bevy_min] frame captured ({})", progress.captured);
}
