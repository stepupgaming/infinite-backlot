//! Greybox 3D scene: the surreal apartment building, its characters, the camera
//! rig, lights, and a minimal (text-free) operator overlay.

use crate::backlot_scene::BacklotSetMode;
use crate::state::*;
use backlot_core::avatar::{HumanoidRig, Pose, Xform};
use bevy::prelude::*;

pub struct HudEntities {
    pub top: Entity,
    pub bottom: Entity,
    pub indicator: Entity,
}

#[derive(Resource, Default)]
pub struct Hud(pub Option<HudEntities>);

pub fn hex_to_color(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&h.get(0..2).unwrap_or("80"), 16).unwrap_or(128);
    let g = u8::from_str_radix(&h.get(2..4).unwrap_or("80"), 16).unwrap_or(128);
    let b = u8::from_str_radix(&h.get(4..6).unwrap_or("80"), 16).unwrap_or(128);
    Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

/// Parse `#rrggbb` into linear-ish 0..1 RGB for the humanoid rig math.
fn hex_rgb(hex: &str) -> [f32; 3] {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&h.get(0..2).unwrap_or("80"), 16).unwrap_or(128);
    let g = u8::from_str_radix(&h.get(2..4).unwrap_or("80"), 16).unwrap_or(128);
    let b = u8::from_str_radix(&h.get(4..6).unwrap_or("80"), 16).unwrap_or(128);
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

fn v3(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// Spawn the static world + dynamic actors from `world`.
pub fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<CanonicalWorld>,
    mut scene: ResMut<SceneIndex>,
    mut hud: ResMut<Hud>,
    set_mode: Res<BacklotSetMode>,
) {
    scene.characters.clear();
    let greybox = *set_mode == BacklotSetMode::Greybox;
    if greybox {
        scene.props.clear();
        scene.marks.clear();
        scene.anchors.clear();
    }

    // The procedural set is an explicit diagnostic fallback only.
    if greybox {
        commands.spawn((
            PointLight {
                intensity: 1_200.0,
                ..default()
            },
            Transform::from_xyz(0.0, 5.0, 2.0),
            FlickerLight {
                base_intensity: 1_200.0,
                active: false,
                phase: 0.0,
            },
        ));
        commands.spawn(AmbientLight {
            color: Color::srgb(0.5, 0.5, 0.6),
            brightness: 0.6,
            affects_lightmapped_meshes: true,
        });
        commands.spawn((
            DirectionalLight {
                illuminance: 800.0,
                ..default()
            },
            Transform::from_xyz(2.0, 6.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));

        // Floor.
        commands.spawn((
            Mesh3d(meshes.add(Plane3d::default().mesh().size(24.0, 24.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.18, 0.18, 0.22),
                ..default()
            })),
            Transform::from_xyz(0.0, 0.0, -2.0)
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        ));

        // Back wall + simple side walls (greybox).
        let wall_mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.28, 0.28, 0.34),
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(20.0, 4.0, 0.3))),
            MeshMaterial3d(wall_mat.clone()),
            Transform::from_xyz(0.0, 2.0, -3.2),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.3, 4.0, 12.0))),
            MeshMaterial3d(wall_mat.clone()),
            Transform::from_xyz(8.0, 2.0, -2.0),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.3, 4.0, 12.0))),
            MeshMaterial3d(wall_mat),
            Transform::from_xyz(-8.0, 2.0, -2.0),
        ));

        // Elevator + doors from world data.
        let elevator = world.0.prop("elevator");
        if let Some(e) = elevator {
            if let Some(loc) = world.0.location(&e.location_id) {
                if let Some(m) = loc.staging_marks.iter().find(|m| m.id == e.home_mark) {
                    commands.spawn((
                        Mesh3d(meshes.add(Cuboid::new(1.6, 2.6, 0.6))),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: Color::srgb(0.45, 0.47, 0.5),
                            metallic: 0.9,
                            ..default()
                        })),
                        Transform::from_xyz(m.position[0], 1.3, m.position[2] + 0.2),
                    ));
                }
            }
        }

        // Props as small colored shapes at their home marks.
        for p in world.0.props.values() {
            let loc = match world.0.location(&p.location_id) {
                Some(l) => l,
                None => continue,
            };
            let mpos = match loc.staging_marks.iter().find(|m| m.id == p.home_mark) {
                Some(m) => m.position,
                None => continue,
            };
            let ent = commands
                .spawn((
                    Mesh3d(meshes.add(Sphere::new(0.22).mesh().ico(3).unwrap())),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgb(0.9, 0.75, 0.3),
                        ..default()
                    })),
                    Transform::from_xyz(mpos[0], 0.4, mpos[2] + 0.4),
                    PropMarker {
                        ids: vec![p.id.clone()],
                    },
                ))
                .id();
            scene.props.insert(p.id.clone(), ent);
            scene.marks.insert(p.home_mark.clone(), v3(mpos));
        }

        // Record staging marks + camera anchors.
        for l in world.0.locations.values() {
            for m in &l.staging_marks {
                scene.marks.insert(m.id.clone(), v3(m.position));
            }
            for a in &l.camera_anchors {
                scene.anchors.push((v3(a.position), v3(a.look_at)));
            }
        }
    }

    // Characters as humanoid performers built from the SAME semantic rig the
    // offline renderer uses (SMPL-X-compatible contract). A blocky box stand-in
    // stands in for a future skinned asset; the parent entity carries the
    // `CharacterAvatar` (driven by the existing movement/talk systems) and each
    // rig part is a child box positioned at its rest-pose offset.
    let start_marks = [
        "hall_center",
        "apt_3b_door",
        "maintenance_panel",
        "apt_4a_door",
    ];
    let ids: Vec<String> = world.0.characters.keys().cloned().collect();
    for (i, id) in ids.iter().enumerate() {
        let c = &world.0.characters[id];
        let mark = start_marks.get(i).copied().unwrap_or("hall_center");
        let pos = scene.marks.get(mark).copied().unwrap_or(Vec3::ZERO);

        let body = hex_rgb(&c.color_hex);
        let rig = HumanoidRig::default_humanoid(&c.id, &c.voice_id, body);
        let mats = rig.world_matrices(&Xform::identity(), &Pose::default());

        let parent = commands
            .spawn((
                Transform::from_xyz(pos.x, 0.0, pos.z),
                CharacterAvatar {
                    id: c.id.clone(),
                    display: c.display_name.clone(),
                    color: hex_to_color(&c.color_hex),
                    speed: 1.8,
                    nav_target: None,
                    speaking_until: -1.0,
                    emote: c
                        .emotion
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "neutral".into()),
                },
            ))
            .id();

        // One box per rig part, placed at its rest-pose position relative to the
        // parent's root (root sits on the floor at y=0).
        for part in &rig.parts {
            if let Some(w) = mats.get(&part.joint) {
                commands.spawn((
                    ChildOf(parent),
                    Mesh3d(meshes.add(Cuboid::new(
                        part.half[0] * 2.0,
                        part.half[1] * 2.0,
                        part.half[2] * 2.0,
                    ))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgb(part.color[0], part.color[1], part.color[2]),
                        ..default()
                    })),
                    Transform::from_xyz(w.pos[0], w.pos[1], w.pos[2]),
                ));
            }
        }
        scene.characters.insert(c.id.clone(), parent);
    }

    // Camera rig.
    let (cam_pos, cam_look) = scene
        .anchors
        .first()
        .copied()
        .unwrap_or((Vec3::new(0.0, 3.0, 7.0), Vec3::ZERO));
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 35.0_f32.to_radians() * 2.0,
            ..default()
        }),
        Transform::from_xyz(cam_pos.x, cam_pos.y, cam_pos.z).looking_at(cam_look, Vec3::Y),
        MainCamera,
        CameraRig {
            intent: "establish".into(),
            anchor_node: None,
            desired_pos: cam_pos,
            desired_look: cam_look,
            desired_fov: 50.0_f32.to_radians(),
            current_look: cam_look,
            anchors: scene.anchors.clone(),
        },
    ));

    // Speech indicator (a small floating marker shown above the active speaker).
    let indicator = commands
        .spawn((
            Mesh3d(meshes.add(Sphere::new(0.12).mesh().ico(2).unwrap())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 1.0, 1.0),
                emissive: Color::srgb(1.0, 0.9, 0.4).into(),
                ..default()
            })),
            Transform::from_xyz(0.0, -10.0, 0.0),
            SpeechIndicator,
        ))
        .id();

    // Minimal operator overlay (no text; conveys state + caption presence).
    let top = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(8.0),
                left: Val::Px(8.0),
                width: Val::Px(220.0),
                height: Val::Px(14.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.2, 0.8, 0.4)),
        ))
        .id();
    let bottom = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(40.0),
                left: Val::Px(40.0),
                width: Val::Px(0.0),
                height: Val::Px(36.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.05, 0.05, 0.08)),
        ))
        .id();
    hud.0 = Some(HudEntities {
        top,
        bottom,
        indicator,
    });
}
