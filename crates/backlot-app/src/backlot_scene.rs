//! Shared Blender-authored backlot set integration.

use crate::state::{PropMarker, SceneIndex};
use bevy::ecs::observer::On;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

pub const DEFAULT_MANIFEST_PATH: &str = "assets/scenes/apartment_floor_03.scene.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub enum BacklotSetMode {
    Imported,
    Greybox,
}

pub fn select_set_mode(value: Option<&str>) -> Result<BacklotSetMode, BacklotSceneError> {
    match value {
        None | Some("") => Ok(BacklotSetMode::Imported),
        Some("greybox") => Ok(BacklotSetMode::Greybox),
        Some(other) => Err(BacklotSceneError::new(format!(
            "BACKLOT_SET_FALLBACK must be unset or exactly 'greybox', got '{other}'"
        ))),
    }
}

#[derive(Debug, Clone)]
pub struct BacklotSceneError(String);

impl BacklotSceneError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for BacklotSceneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for BacklotSceneError {}

#[derive(Debug, Clone, Deserialize)]
pub struct SceneNodeRecord {
    pub node: String,
    #[serde(default)]
    pub kind: String,
    pub position: [f32; 3],
    #[serde(default)]
    pub dimensions: Option<[f32; 3]>,
    #[serde(default)]
    pub look_at: Option<[f32; 3]>,
    #[serde(default)]
    pub lens_mm: Option<f32>,
    #[serde(default)]
    pub collision_role: Option<String>,
    #[serde(default)]
    pub default_visible: Option<bool>,
    #[serde(default)]
    pub closed_position_bevy: Option<[f32; 3]>,
    #[serde(default)]
    pub open_axis_bevy: Option<[f32; 3]>,
    #[serde(default)]
    pub travel_m: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Resource)]
pub struct BacklotSceneManifest {
    pub schema_version: u32,
    pub set_id: String,
    pub runtime_glb: String,
    pub required_nodes: Vec<String>,
    #[serde(default)]
    pub static_geometry: Vec<SceneNodeRecord>,
    #[serde(default)]
    pub dynamic_objects: Vec<SceneNodeRecord>,
    #[serde(default)]
    pub interactables: Vec<SceneNodeRecord>,
    #[serde(default)]
    pub props: Vec<SceneNodeRecord>,
    #[serde(default)]
    pub staging_marks: Vec<SceneNodeRecord>,
    #[serde(default)]
    pub camera_anchors: Vec<SceneNodeRecord>,
    #[serde(default)]
    pub colliders: Vec<SceneNodeRecord>,
    #[serde(default)]
    pub cutaways: Vec<SceneNodeRecord>,
    #[serde(default)]
    pub lighting_references: Vec<SceneNodeRecord>,
}

impl BacklotSceneManifest {
    pub fn camera_anchor(&self, node: &str) -> Option<&SceneNodeRecord> {
        self.camera_anchors
            .iter()
            .find(|record| record.node == node)
    }

    pub fn dynamic_object(&self, node: &str) -> Option<&SceneNodeRecord> {
        self.dynamic_objects
            .iter()
            .find(|record| record.node == node)
    }

    pub fn staging_mark(&self, node: &str) -> Option<&SceneNodeRecord> {
        self.staging_marks.iter().find(|record| record.node == node)
    }

    pub fn prop_position(&self, prop_id: &str) -> Option<[f32; 3]> {
        let node = match prop_id {
            "elevator" | "elevator_frame" => "SET_Elevator_Frame_Left",
            "elevator_doors" => "DOOR_Elevator_Left",
            "elevator_indicator" => "PROP_Floor_Indicator_Glyph",
            "elevator_panel" | "elevator_control_panel" | "control_panel" | "maintenance_panel" => {
                "PROP_Elevator_Panel"
            }
            "hallway_light" | "flickering_light" => "LIGHT_Hall_Warm_2",
            "strange_plant" => "PROP_Plant_Pot",
            _ => return None,
        };
        self.all_records()
            .into_iter()
            .find(|record| record.node == node)
            .map(|record| record.position)
    }

    fn all_records(&self) -> Vec<&SceneNodeRecord> {
        self.static_geometry
            .iter()
            .chain(&self.dynamic_objects)
            .chain(&self.interactables)
            .chain(&self.props)
            .chain(&self.staging_marks)
            .chain(&self.camera_anchors)
            .chain(&self.colliders)
            .chain(&self.cutaways)
            .chain(&self.lighting_references)
            .collect()
    }
}

pub fn parse_manifest_str(text: &str) -> Result<BacklotSceneManifest, BacklotSceneError> {
    let manifest: BacklotSceneManifest = serde_json::from_str(text).map_err(|error| {
        BacklotSceneError::new(format!("invalid backlot sidecar JSON: {error}"))
    })?;
    if manifest.schema_version != 1 {
        return Err(BacklotSceneError::new(format!(
            "unsupported backlot sidecar schema_version {}; expected 1",
            manifest.schema_version
        )));
    }
    if manifest.set_id != "apartment_floor_03" {
        return Err(BacklotSceneError::new(format!(
            "unexpected backlot set_id '{}'; expected apartment_floor_03",
            manifest.set_id
        )));
    }
    if !manifest.runtime_glb.ends_with(".glb") {
        return Err(BacklotSceneError::new(
            "backlot sidecar runtime_glb must name a .glb asset",
        ));
    }
    let declared = manifest
        .all_records()
        .into_iter()
        .map(|record| record.node.as_str())
        .collect::<HashSet<_>>();
    let missing_declarations = manifest
        .required_nodes
        .iter()
        .filter(|node| !declared.contains(node.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_declarations.is_empty() {
        return Err(BacklotSceneError::new(format!(
            "backlot sidecar required_nodes are not described by semantic records: {}",
            missing_declarations.join(", ")
        )));
    }
    for door_name in ["DOOR_Elevator_Left", "DOOR_Elevator_Right"] {
        let Some(door) = manifest.dynamic_object(door_name) else {
            return Err(BacklotSceneError::new(format!(
                "backlot sidecar is missing dynamic door record {door_name}"
            )));
        };
        if door.closed_position_bevy.is_none()
            || door.open_axis_bevy.is_none()
            || door.travel_m.is_none()
        {
            return Err(BacklotSceneError::new(format!(
                "dynamic door {door_name} is missing closed_position_bevy, open_axis_bevy, or travel_m"
            )));
        }
    }
    Ok(manifest)
}

pub fn validate_required_nodes(
    manifest: &BacklotSceneManifest,
    instantiated_names: &HashSet<String>,
) -> Result<(), BacklotSceneError> {
    let missing = manifest
        .required_nodes
        .iter()
        .filter(|node| !instantiated_names.contains(*node))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(BacklotSceneError::new(format!(
            "backlot GLB is missing required node(s): {}",
            missing.join(", ")
        )))
    }
}

pub fn dynamic_door_translation(
    door: &SceneNodeRecord,
    open: f32,
) -> Result<[f32; 3], BacklotSceneError> {
    let closed = door.closed_position_bevy.ok_or_else(|| {
        BacklotSceneError::new(format!(
            "dynamic door {} has no closed transform",
            door.node
        ))
    })?;
    let axis = door.open_axis_bevy.ok_or_else(|| {
        BacklotSceneError::new(format!("dynamic door {} has no open axis", door.node))
    })?;
    let travel = door.travel_m.ok_or_else(|| {
        BacklotSceneError::new(format!("dynamic door {} has no travel distance", door.node))
    })?;
    let amount = open.clamp(0.0, 1.0) * travel;
    Ok([
        closed[0] + axis[0] * amount,
        closed[1] + axis[1] * amount,
        closed[2] + axis[2] * amount,
    ])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraSubjectKind {
    Character,
    CharacterPair,
    CharacterGroup,
    Prop,
    EnvironmentFeature,
    StagingRegion,
    Missing,
}

#[derive(Debug, Clone)]
pub struct CameraSubject {
    pub id: String,
    pub kind: CameraSubjectKind,
    pub points: Vec<[f32; 3]>,
}

impl CameraSubject {
    pub fn character(id: impl Into<String>, point: [f32; 3]) -> Self {
        Self {
            id: id.into(),
            kind: CameraSubjectKind::Character,
            points: vec![point],
        }
    }

    pub fn group(id: impl Into<String>, points: Vec<[f32; 3]>) -> Self {
        Self {
            id: id.into(),
            kind: if points.len() == 2 {
                CameraSubjectKind::CharacterPair
            } else {
                CameraSubjectKind::CharacterGroup
            },
            points,
        }
    }

    pub fn feature(id: impl Into<String>, point: [f32; 3], kind: CameraSubjectKind) -> Self {
        Self {
            id: id.into(),
            kind,
            points: vec![point],
        }
    }

    pub fn missing(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: CameraSubjectKind::Missing,
            points: Vec::new(),
        }
    }

    pub fn center(&self) -> Option<[f32; 3]> {
        if self.points.is_empty() {
            return None;
        }
        let sum = self.points.iter().fold([0.0; 3], |mut sum, point| {
            sum[0] += point[0];
            sum[1] += point[1];
            sum[2] += point[2];
            sum
        });
        let count = self.points.len() as f32;
        Some([sum[0] / count, sum[1] / count, sum[2] / count])
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CameraRejection {
    NonFiniteTransform,
    InsideWallCollider(String),
    Occluded(String),
    TooClose(f32),
    SubjectTooSmall(f32),
    RepeatedComposition(String),
    MissingSubject(String),
}

impl fmt::Display for CameraRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteTransform => formatter.write_str("camera transform is non-finite"),
            Self::InsideWallCollider(node) => write!(formatter, "camera is inside {node}"),
            Self::Occluded(node) => write!(formatter, "line of sight is blocked by {node}"),
            Self::TooClose(distance) => write!(formatter, "camera is too close ({distance:.2}m)"),
            Self::SubjectTooSmall(distance) => {
                write!(formatter, "subject would be too small ({distance:.2}m)")
            }
            Self::RepeatedComposition(node) => {
                write!(formatter, "composition repeats adjacent anchor {node}")
            }
            Self::MissingSubject(id) => write!(formatter, "missing named subject {id}"),
        }
    }
}

pub fn validate_camera_candidate(
    manifest: &BacklotSceneManifest,
    anchor: &SceneNodeRecord,
    subject: &CameraSubject,
    previous_anchor: Option<&str>,
) -> Result<(), CameraRejection> {
    let look = anchor.look_at.unwrap_or(anchor.position);
    if !anchor
        .position
        .into_iter()
        .chain(look)
        .chain(anchor.lens_mm)
        .all(f32::is_finite)
    {
        return Err(CameraRejection::NonFiniteTransform);
    }
    let center = subject
        .center()
        .ok_or_else(|| CameraRejection::MissingSubject(subject.id.clone()))?;
    for collider in &manifest.colliders {
        if collider.collision_role.as_deref() != Some("wall_boundary") {
            continue;
        }
        let dimensions = collider.dimensions.unwrap_or([0.0; 3]);
        let inside = (0..3).all(|axis| {
            let half = dimensions[axis] * 0.5;
            anchor.position[axis] >= collider.position[axis] - half
                && anchor.position[axis] <= collider.position[axis] + half
        });
        if inside {
            return Err(CameraRejection::InsideWallCollider(collider.node.clone()));
        }
    }
    for geometry in manifest.static_geometry.iter().chain(&manifest.colliders) {
        let Some(dimensions) = geometry.dimensions else {
            continue;
        };
        if geometry.node.to_ascii_lowercase().contains("floor")
            || geometry.node.to_ascii_lowercase().contains("ceiling")
        {
            continue;
        }
        if point_inside_aabb(center, geometry.position, dimensions, 0.18) {
            continue;
        }
        if segment_intersects_aabb(anchor.position, center, geometry.position, dimensions) {
            return Err(CameraRejection::Occluded(geometry.node.clone()));
        }
    }
    let delta = [
        anchor.position[0] - center[0],
        anchor.position[1] - center[1],
        anchor.position[2] - center[2],
    ];
    let distance = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
    let minimum_distance = match subject.kind {
        CameraSubjectKind::Character
        | CameraSubjectKind::CharacterPair
        | CameraSubjectKind::CharacterGroup => 1.5,
        _ => 1.2,
    };
    if distance < minimum_distance {
        return Err(CameraRejection::TooClose(distance));
    }
    if distance > 14.0 {
        return Err(CameraRejection::SubjectTooSmall(distance));
    }
    if let Some(previous) = previous_anchor {
        if previous == anchor.node {
            return Err(CameraRejection::RepeatedComposition(anchor.node.clone()));
        }
        if let Some(previous) = manifest.camera_anchor(previous) {
            let eye_delta = Vec3::from_array(previous.position) - Vec3::from_array(anchor.position);
            let previous_direction =
                (Vec3::from_array(previous.look_at.unwrap_or(previous.position))
                    - Vec3::from_array(previous.position))
                .normalize_or_zero();
            let direction =
                (Vec3::from_array(look) - Vec3::from_array(anchor.position)).normalize_or_zero();
            if eye_delta.length() < 0.5 && previous_direction.dot(direction) > 0.98 {
                return Err(CameraRejection::RepeatedComposition(anchor.node.clone()));
            }
        }
    }
    Ok(())
}

fn point_inside_aabb(point: [f32; 3], center: [f32; 3], dimensions: [f32; 3], pad: f32) -> bool {
    (0..3).all(|axis| {
        let half = dimensions[axis] * 0.5 + pad;
        point[axis] >= center[axis] - half && point[axis] <= center[axis] + half
    })
}

fn segment_intersects_aabb(
    start: [f32; 3],
    end: [f32; 3],
    center: [f32; 3],
    dimensions: [f32; 3],
) -> bool {
    let mut entry = 0.0f32;
    let mut exit = 1.0f32;
    for axis in 0..3 {
        let half = dimensions[axis].abs() * 0.5;
        let min = center[axis] - half;
        let max = center[axis] + half;
        let delta = end[axis] - start[axis];
        if delta.abs() < 1e-6 {
            if start[axis] < min || start[axis] > max {
                return false;
            }
            continue;
        }
        let mut near = (min - start[axis]) / delta;
        let mut far = (max - start[axis]) / delta;
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        entry = entry.max(near);
        exit = exit.min(far);
        if entry > exit {
            return false;
        }
    }
    exit >= 0.02 && entry <= 0.98
}

fn preferred_anchor_names(intent: &str, subject: &CameraSubject) -> Vec<&'static str> {
    let intent = intent.to_ascii_lowercase();
    let id = subject.id.to_ascii_lowercase();
    if intent.contains("elevator_interior") || id == "elevator_interior" {
        return vec!["CAM_Elevator_Interior", "CAM_Elevator_Reveal", "CAM_Payoff"];
    }
    if intent.contains("panel")
        || id.contains("panel")
        || id.contains("indicator")
        || (intent.contains("insert") && (id.contains("button") || id.contains("panel")))
    {
        return vec!["CAM_Panel_Insert", "CAM_Elevator_Reveal", "CAM_Payoff"];
    }
    if intent.contains("elevator_reveal") || (intent.contains("reveal") && id.contains("elevator"))
    {
        return vec!["CAM_Elevator_Reveal", "CAM_Payoff", "CAM_Elevator_Interior"];
    }
    if intent.contains("elevator_blocking_wide") {
        return vec![
            "CAM_Elevator_Blocking_Wide",
            "CAM_Hallway_Wide",
            "CAM_Payoff",
        ];
    }
    if let Some(center) = subject.center() {
        if center[2] < -2.5 && center[0] < -3.0 {
            return vec!["CAM_Elevator_Reveal", "CAM_Payoff", "CAM_Elevator_Interior"];
        }
        if center[2] < -2.5 {
            return vec!["CAM_Hallway_Depth", "CAM_TwoShot_A", "CAM_Hallway_Wide"];
        }
        if center[0] > 5.0 {
            return vec!["CAM_Side_Corridor", "CAM_Hallway_Depth", "CAM_Hallway_Wide"];
        }
    }
    if intent.contains("payoff") || intent.contains("cliffhanger") {
        return vec!["CAM_Payoff", "CAM_Elevator_Reveal", "CAM_Hallway_Wide"];
    }
    if intent.contains("over_the_shoulder") || intent == "ots" {
        return vec!["CAM_OTS_Right", "CAM_OTS_Left", "CAM_TwoShot_A"];
    }
    if intent.contains("reaction") {
        return vec!["CAM_Reaction_Right", "CAM_Reaction_Left", "CAM_OTS_Right"];
    }
    if intent.contains("tension") {
        return vec!["CAM_TwoShot_A", "CAM_OTS_Left", "CAM_Hallway_Depth"];
    }
    if intent.contains("speaker") || intent.contains("closeup") {
        return vec!["CAM_OTS_Left", "CAM_OTS_Right", "CAM_TwoShot_A"];
    }
    if intent.contains("two_shot")
        || intent.contains("conversation")
        || intent.contains("group")
        || matches!(
            subject.kind,
            CameraSubjectKind::CharacterPair | CameraSubjectKind::CharacterGroup
        )
    {
        return vec!["CAM_TwoShot_A", "CAM_Hallway_Wide", "CAM_Hallway_Depth"];
    }
    if intent.contains("depth")
        || intent.contains("full_body")
        || intent.contains("follow")
        || intent.contains("exit")
    {
        return vec!["CAM_Hallway_Depth", "CAM_Hallway_Wide", "CAM_TwoShot_A"];
    }
    if intent.contains("establish") || intent.contains("wide") || intent.contains("spatial") {
        return vec!["CAM_Hallway_Wide", "CAM_Hallway_Depth", "CAM_Side_Corridor"];
    }
    match subject.kind {
        CameraSubjectKind::Prop | CameraSubjectKind::EnvironmentFeature => {
            vec!["CAM_Side_Corridor", "CAM_Panel_Insert", "CAM_Hallway_Depth"]
        }
        CameraSubjectKind::StagingRegion => {
            vec!["CAM_Hallway_Depth", "CAM_Hallway_Wide", "CAM_Side_Corridor"]
        }
        _ => vec!["CAM_TwoShot_A", "CAM_Hallway_Depth", "CAM_Hallway_Wide"],
    }
}

#[derive(Debug)]
pub struct CameraSelectionResult<'a> {
    pub anchor: &'a SceneNodeRecord,
    pub rejected_candidates: Vec<String>,
    pub valid_candidate_count: usize,
}

fn camera_candidate_score(
    anchor: &SceneNodeRecord,
    intent: &str,
    subject: &CameraSubject,
    preference_rank: usize,
) -> f32 {
    let center = subject.center().unwrap_or(anchor.position);
    let dx = anchor.position[0] - center[0];
    let dy = anchor.position[1] - center[1];
    let dz = anchor.position[2] - center[2];
    let distance = (dx * dx + dy * dy + dz * dz).sqrt();
    let ideal_distance = if intent.contains("insert") || intent.contains("closeup") {
        4.0
    } else if intent.contains("wide") || intent.contains("full_body") || intent.contains("group") {
        9.0
    } else {
        6.0
    };
    let distance_score = 24.0 - (distance - ideal_distance).abs() * 2.5;
    let lens = anchor.lens_mm.unwrap_or(42.0);
    let lens_score = if intent.contains("wide") {
        8.0 - (lens - 32.0).abs() * 0.2
    } else if intent.contains("insert") {
        8.0 - (lens - 55.0).abs() * 0.15
    } else {
        8.0 - (lens - 45.0).abs() * 0.15
    };
    100.0 - preference_rank as f32 * 30.0 + distance_score + lens_score
}

pub fn select_camera_anchor_with_report<'a>(
    manifest: &'a BacklotSceneManifest,
    intent: &str,
    subject: &CameraSubject,
    previous_anchor: Option<&str>,
) -> Result<CameraSelectionResult<'a>, BacklotSceneError> {
    if subject.points.is_empty() {
        return Err(BacklotSceneError::new(format!(
            "camera shot rejected: missing named subject {}",
            subject.id
        )));
    }
    let mut candidates = preferred_anchor_names(intent, subject);
    candidates.extend([
        "CAM_Hallway_Wide",
        "CAM_Hallway_Depth",
        "CAM_TwoShot_A",
        "CAM_Reaction_Left",
        "CAM_Reaction_Right",
        "CAM_OTS_Left",
        "CAM_OTS_Right",
        "CAM_Elevator_Reveal",
        "CAM_Elevator_Interior",
        "CAM_Panel_Insert",
        "CAM_Payoff",
        "CAM_Side_Corridor",
    ]);
    let mut seen = HashSet::new();
    let mut rejections = Vec::new();
    let mut valid = Vec::new();
    for (preference_rank, name) in candidates.into_iter().enumerate() {
        if !seen.insert(name) {
            continue;
        }
        let Some(anchor) = manifest.camera_anchor(name) else {
            rejections.push(format!("{name}: missing authored anchor"));
            continue;
        };
        match validate_camera_candidate(manifest, anchor, subject, previous_anchor) {
            Ok(()) => valid.push((
                camera_candidate_score(anchor, intent, subject, preference_rank),
                anchor,
            )),
            Err(reason) => rejections.push(format!("{name}: {reason}")),
        }
    }
    valid.sort_by(|a, b| b.0.total_cmp(&a.0));
    if let Some((_, anchor)) = valid.first() {
        return Ok(CameraSelectionResult {
            anchor,
            rejected_candidates: rejections,
            valid_candidate_count: valid.len(),
        });
    }
    Err(BacklotSceneError::new(format!(
        "no valid authored camera for intent '{intent}' subject '{}': {}",
        subject.id,
        rejections.join("; ")
    )))
}

pub fn select_camera_anchor<'a>(
    manifest: &'a BacklotSceneManifest,
    intent: &str,
    subject: &CameraSubject,
    previous_anchor: Option<&str>,
) -> Result<&'a SceneNodeRecord, BacklotSceneError> {
    select_camera_anchor_with_report(manifest, intent, subject, previous_anchor)
        .map(|selection| selection.anchor)
}

pub fn camera_fov_radians(anchor: &SceneNodeRecord) -> f32 {
    let lens = anchor.lens_mm.unwrap_or(42.0).max(1.0);
    2.0 * (18.0 / lens).atan()
}

pub fn camera_look_at(anchor: &SceneNodeRecord, subject: &CameraSubject) -> [f32; 3] {
    let target = match subject.kind {
        CameraSubjectKind::Character
        | CameraSubjectKind::CharacterPair
        | CameraSubjectKind::CharacterGroup => subject.center().or(anchor.look_at),
        _ => anchor.look_at.or_else(|| subject.center()),
    };
    target.unwrap_or([
        anchor.position[0],
        anchor.position[1],
        anchor.position[2] - 1.0,
    ])
}

pub fn cutaways_for_anchor(anchor: &str) -> &'static [&'static str] {
    match anchor {
        "CAM_Hallway_Wide" | "CAM_Hallway_Depth" | "CAM_TwoShot_A" | "CAM_OTS_Left"
        | "CAM_OTS_Right" | "CAM_Reaction_Left" | "CAM_Reaction_Right" => {
            &["CUTAWAY_Hallway_South"]
        }
        "CAM_Side_Corridor" => &["CUTAWAY_Ceiling_Side"],
        "CAM_Elevator_Interior" => &["CUTAWAY_Elevator_Back"],
        _ => &[],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BacklotLoadStatus {
    Loading,
    Ready,
    Failed(String),
}

#[derive(Resource, Debug, Clone)]
pub struct BacklotSceneRuntime {
    pub status: BacklotLoadStatus,
    pub node_entities: HashMap<String, Entity>,
    pub prop_entities: HashMap<String, Entity>,
}

impl Default for BacklotSceneRuntime {
    fn default() -> Self {
        Self {
            status: BacklotLoadStatus::Loading,
            node_entities: HashMap::new(),
            prop_entities: HashMap::new(),
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct BacklotFrameState {
    pub elevator_open: f32,
    pub elevator_indicator_active: bool,
    pub panel_active: f32,
    pub impossible_reveal: f32,
    pub flicker: bool,
    pub time: f32,
    pub active_anchor: Option<String>,
}

impl Default for BacklotFrameState {
    fn default() -> Self {
        Self {
            elevator_open: 0.0,
            elevator_indicator_active: false,
            panel_active: 0.0,
            impossible_reveal: 0.0,
            flicker: false,
            time: 0.0,
            active_anchor: None,
        }
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum BacklotSceneSet {
    ApplyFrame,
}

#[derive(Component)]
struct BacklotSetRoot;

#[derive(Component)]
pub struct BacklotNode {
    pub name: String,
}

#[derive(Component)]
struct ImportedElevatorDoor {
    record: SceneNodeRecord,
}

#[derive(Component)]
struct ImportedIndicator;

#[derive(Component)]
struct ImportedPanelButton;

#[derive(Component)]
struct ImportedAnimatedNode {
    base_translation: Vec3,
    base_rotation: Quat,
    base_scale: Vec3,
}

#[derive(Component)]
struct ImportedCutaway {
    node: String,
    default_visible: bool,
}

#[derive(Component)]
struct BacklotPracticalLight {
    base_intensity: f32,
    elevator: bool,
}

#[derive(Clone)]
pub struct BacklotScenePlugin {
    mode: BacklotSetMode,
    manifest: Option<BacklotSceneManifest>,
}

impl BacklotScenePlugin {
    pub fn load(project_root: &Path) -> Result<Self, BacklotSceneError> {
        let mode = select_set_mode(std::env::var("BACKLOT_SET_FALLBACK").ok().as_deref())?;
        if mode == BacklotSetMode::Greybox {
            return Ok(Self {
                mode,
                manifest: None,
            });
        }
        let sidecar_path = project_root.join(DEFAULT_MANIFEST_PATH);
        let text = std::fs::read_to_string(&sidecar_path).map_err(|error| {
            BacklotSceneError::new(format!(
                "failed to read backlot sidecar {}: {error}",
                sidecar_path.display()
            ))
        })?;
        let manifest = parse_manifest_str(&text)?;
        let glb_path = project_root.join(PathBuf::from(&manifest.runtime_glb));
        if !glb_path.is_file() {
            return Err(BacklotSceneError::new(format!(
                "backlot GLB does not exist: {}",
                glb_path.display()
            )));
        }
        Ok(Self {
            mode,
            manifest: Some(manifest),
        })
    }

    pub fn mode(&self) -> BacklotSetMode {
        self.mode
    }

    pub fn manifest(&self) -> Option<&BacklotSceneManifest> {
        self.manifest.as_ref()
    }
}

impl Plugin for BacklotScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.mode)
            .insert_resource(BacklotFrameState::default())
            .configure_sets(Update, BacklotSceneSet::ApplyFrame);
        if let Some(manifest) = &self.manifest {
            app.insert_resource(manifest.clone())
                .insert_resource(BacklotSceneRuntime::default())
                .add_systems(Startup, spawn_imported_backlot)
                .add_systems(
                    Update,
                    apply_imported_backlot_state.in_set(BacklotSceneSet::ApplyFrame),
                )
                .add_observer(index_imported_backlot);
        } else {
            app.insert_resource(BacklotSceneRuntime {
                status: BacklotLoadStatus::Ready,
                ..default()
            });
        }
    }
}

fn asset_relative_path(runtime_glb: &str) -> String {
    runtime_glb
        .replace('\\', "/")
        .strip_prefix("assets/")
        .unwrap_or(runtime_glb)
        .to_string()
}

fn spawn_imported_backlot(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    manifest: Res<BacklotSceneManifest>,
) {
    let scene: Handle<WorldAsset> = asset_server
        .load(GltfAssetLabel::Scene(0).from_asset(asset_relative_path(&manifest.runtime_glb)));
    commands.spawn((WorldAssetRoot(scene), BacklotSetRoot));

    for reference in &manifest.lighting_references {
        if !reference.node.starts_with("LIGHT_") {
            continue;
        }
        let (color, intensity, range, elevator) = practical_light_spec(&reference.node);
        commands.spawn((
            PointLight {
                color,
                intensity,
                range,
                radius: 0.35,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(Vec3::from_array(reference.position)),
            BacklotPracticalLight {
                base_intensity: intensity,
                elevator,
            },
        ));
    }
    commands.spawn(AmbientLight {
        color: Color::srgb(0.48, 0.46, 0.52),
        brightness: 220.0,
        affects_lightmapped_meshes: false,
    });
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.92, 0.82),
            illuminance: 3_500.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 5.0).looking_at(Vec3::new(0.0, 1.2, -1.5), Vec3::Y),
    ));
}

fn practical_light_spec(node: &str) -> (Color, f32, f32, bool) {
    if node.contains("Elevator") {
        (Color::srgb(0.48, 0.72, 1.0), 18_000.0, 9.0, true)
    } else if node.contains("Side") {
        (Color::srgb(0.48, 0.64, 1.0), 12_500.0, 11.0, false)
    } else if node.contains("Lobby") {
        (Color::srgb(1.0, 0.76, 0.54), 21_000.0, 12.0, false)
    } else {
        (Color::srgb(1.0, 0.72, 0.48), 16_500.0, 10.0, false)
    }
}

fn index_imported_backlot(
    trigger: On<WorldInstanceReady>,
    mut commands: Commands,
    roots: Query<(), With<BacklotSetRoot>>,
    children: Query<&Children>,
    names: Query<&Name>,
    transforms: Query<&Transform>,
    manifest: Res<BacklotSceneManifest>,
    mut runtime: ResMut<BacklotSceneRuntime>,
) {
    if roots.get(trigger.entity).is_err() {
        return;
    }
    let mut entities_by_name = HashMap::new();
    for entity in std::iter::once(trigger.entity).chain(children.iter_descendants(trigger.entity)) {
        if let Ok(name) = names.get(entity) {
            entities_by_name.insert(name.as_str().to_string(), entity);
        }
    }
    let instantiated_names = entities_by_name.keys().cloned().collect::<HashSet<_>>();
    if let Err(error) = validate_required_nodes(&manifest, &instantiated_names) {
        runtime.status = BacklotLoadStatus::Failed(error.to_string());
        tracing::error!("{error}");
        return;
    }

    for (name, entity) in &entities_by_name {
        commands
            .entity(*entity)
            .insert(BacklotNode { name: name.clone() });
    }
    for collider in &manifest.colliders {
        if let Some(entity) = entities_by_name.get(&collider.node) {
            commands.entity(*entity).insert(Visibility::Hidden);
        }
    }
    // Blender cameras are semantic anchor records, not active Bevy renderers.
    // The interactive and production paths each own exactly one real camera.
    for anchor in &manifest.camera_anchors {
        if let Some(entity) = entities_by_name.get(&anchor.node) {
            commands
                .entity(*entity)
                .remove::<Camera>()
                .remove::<Camera3d>();
        }
    }
    for cutaway in &manifest.cutaways {
        if let Some(entity) = entities_by_name.get(&cutaway.node) {
            let visible = cutaway.default_visible.unwrap_or(true);
            commands.entity(*entity).insert((
                ImportedCutaway {
                    node: cutaway.node.clone(),
                    default_visible: visible,
                },
                if visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ));
        }
    }
    for dynamic in &manifest.dynamic_objects {
        let Some(entity) = entities_by_name.get(&dynamic.node) else {
            continue;
        };
        let base = transforms.get(*entity).copied().unwrap_or_default();
        let animated = ImportedAnimatedNode {
            base_translation: base.translation,
            base_rotation: base.rotation,
            base_scale: base.scale,
        };
        if dynamic.kind == "dynamic_door" {
            commands.entity(*entity).insert((
                animated,
                ImportedElevatorDoor {
                    record: dynamic.clone(),
                },
            ));
        } else if dynamic.node == "PROP_Floor_Indicator_Glyph" {
            commands
                .entity(*entity)
                .insert((animated, ImportedIndicator));
        }
    }
    for interactable in &manifest.interactables {
        if let Some(entity) = entities_by_name.get(&interactable.node) {
            let base = transforms.get(*entity).copied().unwrap_or_default();
            commands.entity(*entity).insert((
                ImportedAnimatedNode {
                    base_translation: base.translation,
                    base_rotation: base.rotation,
                    base_scale: base.scale,
                },
                ImportedPanelButton,
            ));
        }
    }
    for (node, ids) in imported_prop_bindings() {
        let Some(entity) = entities_by_name.get(*node) else {
            continue;
        };
        let ids = ids.iter().map(|id| (*id).to_string()).collect::<Vec<_>>();
        commands
            .entity(*entity)
            .insert(PropMarker { ids: ids.clone() });
        for id in ids {
            runtime.prop_entities.insert(id, *entity);
        }
    }
    runtime.node_entities = entities_by_name;
    runtime.status = BacklotLoadStatus::Ready;
    tracing::info!(
        "backlot set '{}' ready with {} named nodes",
        manifest.set_id,
        runtime.node_entities.len()
    );
}

fn imported_prop_bindings() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("SET_Elevator_Frame_Left", &["elevator", "elevator_frame"]),
        ("DOOR_Elevator_Left", &["elevator_doors"]),
        ("PROP_Floor_Indicator_Glyph", &["elevator_indicator"]),
        (
            "PROP_Elevator_Panel",
            &[
                "elevator_panel",
                "elevator_control_panel",
                "control_panel",
                "maintenance_panel",
            ],
        ),
        (
            "PROP_Hall_Practical_2",
            &["hallway_light", "flickering_light"],
        ),
        ("PROP_Plant_Pot", &["strange_plant"]),
    ]
}

fn apply_imported_backlot_state(
    state: Res<BacklotFrameState>,
    mut animated: Query<(
        &ImportedAnimatedNode,
        Option<&ImportedElevatorDoor>,
        Option<&ImportedIndicator>,
        Option<&ImportedPanelButton>,
        &mut Transform,
    )>,
    mut cutaways: Query<(&ImportedCutaway, &mut Visibility)>,
    mut lights: Query<(&BacklotPracticalLight, &mut PointLight)>,
) {
    for (base, door, indicator, panel, mut transform) in &mut animated {
        transform.rotation = base.base_rotation;
        if let Some(door) = door {
            if let Ok(translation) = dynamic_door_translation(&door.record, state.elevator_open) {
                transform.translation = Vec3::from_array(translation);
            }
            transform.scale = base.base_scale;
        } else if indicator.is_some() {
            transform.translation = base.base_translation;
            let pulse = if state.elevator_indicator_active {
                1.0 + 0.16 * (state.time * 8.0).sin().abs()
            } else {
                1.0
            };
            transform.scale = base.base_scale * pulse;
        } else if panel.is_some() {
            let active = state.panel_active.clamp(0.0, 1.0);
            transform.translation = base.base_translation + Vec3::new(0.0, 0.0, -0.025 * active);
            transform.scale = base.base_scale * (1.0 + 0.25 * active);
        }
    }
    let corridor_cutaways = state
        .active_anchor
        .as_deref()
        .map(cutaways_for_anchor)
        .unwrap_or_default();
    for (cutaway, mut visibility) in &mut cutaways {
        let hidden_for_camera = corridor_cutaways.contains(&cutaway.node.as_str());
        let hidden_for_reveal =
            state.impossible_reveal > 0.001 && cutaway.node == "CUTAWAY_Elevator_Back";
        *visibility = if cutaway.default_visible && !hidden_for_camera && !hidden_for_reveal {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (tag, mut light) in &mut lights {
        let reveal_boost = if tag.elevator {
            state.impossible_reveal.clamp(0.0, 1.0) * 2_600.0
        } else {
            0.0
        };
        light.intensity = tag.base_intensity + reveal_boost;
        if state.flicker && (state.time * 18.0).sin() > 0.0 {
            light.intensity *= 0.25;
        }
    }
}

pub fn populate_scene_index(
    scene: &mut SceneIndex,
    manifest: &BacklotSceneManifest,
    runtime: &BacklotSceneRuntime,
) {
    scene.marks.clear();
    scene.anchors.clear();
    scene.props = runtime.prop_entities.clone();
    for mark in &manifest.staging_marks {
        scene
            .marks
            .insert(mark.node.clone(), Vec3::from_array(mark.position));
    }
    for (alias, node) in staging_mark_aliases() {
        if let Some(mark) = manifest.staging_mark(node) {
            scene
                .marks
                .insert((*alias).to_string(), Vec3::from_array(mark.position));
        }
    }
    for prop_id in runtime.prop_entities.keys() {
        if let Some(position) = manifest.prop_position(prop_id) {
            scene
                .marks
                .insert(prop_id.clone(), Vec3::from_array(position));
        }
    }
    scene
        .anchors
        .extend(manifest.camera_anchors.iter().map(|anchor| {
            (
                Vec3::from_array(anchor.position),
                Vec3::from_array(anchor.look_at.unwrap_or([0.0, 1.2, 0.0])),
            )
        }));
}

fn staging_mark_aliases() -> &'static [(&'static str, &'static str)] {
    &[
        ("hall_center", "MARK_Hallway_Group_C"),
        ("elevator_door", "MARK_Elevator_Threshold"),
        ("apt_3b_door", "MARK_Apartment_3B"),
        ("apt_4a_door", "MARK_Apartment_4A"),
        ("maintenance_panel", "MARK_Panel_Interaction"),
        ("panel_stand", "MARK_Panel_Interaction"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const MANIFEST: &str = include_str!("../../../assets/scenes/apartment_floor_03.scene.json");

    fn manifest() -> BacklotSceneManifest {
        parse_manifest_str(MANIFEST).expect("ground-truth sidecar should parse")
    }

    #[test]
    fn manifest_success_and_required_nodes() {
        let manifest = manifest();
        let names = manifest
            .required_nodes
            .iter()
            .cloned()
            .collect::<HashSet<_>>();

        validate_required_nodes(&manifest, &names).expect("all required nodes are present");
        assert_eq!(manifest.set_id, "apartment_floor_03");
        assert_eq!(manifest.camera_anchors.len(), 13);
        assert_eq!(manifest.staging_marks.len(), 12);
    }

    #[test]
    fn manifest_missing_nodes_fail_clearly() {
        let manifest = manifest();
        let mut names = manifest
            .required_nodes
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        names.remove("DOOR_Elevator_Left");

        let error = validate_required_nodes(&manifest, &names).unwrap_err();
        assert!(error.to_string().contains("DOOR_Elevator_Left"));
        assert!(error.to_string().contains("missing required"));
    }

    #[test]
    fn camera_intents_map_to_distinct_authored_anchors() {
        let manifest = manifest();
        let subject = CameraSubject::character("mara", [-0.85, 1.25, 0.35]);

        let establish = select_camera_anchor(&manifest, "establish", &subject, None).unwrap();
        let full_body = select_camera_anchor(&manifest, "full_body", &subject, None).unwrap();
        let speaker = select_camera_anchor(&manifest, "speaker_closeup", &subject, None).unwrap();
        let ots = select_camera_anchor(&manifest, "over_the_shoulder", &subject, None).unwrap();
        let elevator = CameraSubject::feature(
            "elevator",
            [-5.5, 1.45, -4.4],
            CameraSubjectKind::EnvironmentFeature,
        );
        let elevator_insert =
            select_camera_anchor(&manifest, "insert_object", &elevator, None).unwrap();

        assert_eq!(establish.node, "CAM_Hallway_Wide");
        assert_eq!(full_body.node, "CAM_Hallway_Depth");
        assert_eq!(speaker.node, "CAM_OTS_Left");
        assert_eq!(ots.node, "CAM_OTS_Right");
        assert_eq!(elevator_insert.node, "CAM_Elevator_Reveal");
        assert_eq!(
            [
                establish.node.as_str(),
                full_body.node.as_str(),
                speaker.node.as_str(),
                ots.node.as_str(),
            ]
            .into_iter()
            .collect::<HashSet<_>>()
            .len(),
            4
        );
    }

    #[test]
    fn elevator_blocking_wide_selects_unoccluded_full_body_anchor() {
        let manifest = manifest();
        let group = CameraSubject::group(
            "proof_group",
            vec![[-6.35, 1.2, -2.45], [-4.1, 1.2, -3.8], [-4.35, 1.2, -2.45]],
        );
        let selected =
            select_camera_anchor(&manifest, "elevator_blocking_wide", &group, None).unwrap();
        assert_eq!(selected.node, "CAM_Elevator_Blocking_Wide");
        validate_camera_candidate(&manifest, selected, &group, None).unwrap();
    }

    #[test]
    fn camera_validation_rejects_bad_and_repeated_compositions() {
        let manifest = manifest();
        let subject = CameraSubject::character("mara", [0.0, 1.2, 0.0]);
        let valid = manifest.camera_anchor("CAM_Hallway_Wide").unwrap();
        validate_camera_candidate(&manifest, valid, &subject, None).unwrap();

        let mut non_finite = valid.clone();
        non_finite.position[0] = f32::NAN;
        assert!(matches!(
            validate_camera_candidate(&manifest, &non_finite, &subject, None),
            Err(CameraRejection::NonFiniteTransform)
        ));
        assert!(matches!(
            validate_camera_candidate(&manifest, valid, &subject, Some("CAM_Hallway_Wide")),
            Err(CameraRejection::RepeatedComposition(_))
        ));

        let missing = CameraSubject::missing("not_in_world");
        assert!(matches!(
            validate_camera_candidate(&manifest, valid, &missing, None),
            Err(CameraRejection::MissingSubject(_))
        ));

        let mut body_collision = valid.clone();
        body_collision.position = [0.0, 1.2, 0.5];
        assert!(matches!(
            validate_camera_candidate(&manifest, &body_collision, &subject, None),
            Err(CameraRejection::TooClose(_))
        ));
    }

    #[test]
    fn camera_validation_rejects_geometry_between_camera_and_subject() {
        let mut manifest = manifest();
        manifest.static_geometry.push(SceneNodeRecord {
            node: "TEST_OCCLUDING_WALL".into(),
            kind: "static".into(),
            position: [0.0, 1.4, 5.0],
            dimensions: Some([8.0, 3.0, 0.2]),
            look_at: None,
            lens_mm: None,
            collision_role: Some("wall_boundary".into()),
            default_visible: Some(true),
            closed_position_bevy: None,
            open_axis_bevy: None,
            travel_m: None,
        });
        let subject = CameraSubject::character("mara", [0.0, 1.2, 0.0]);
        let anchor = manifest.camera_anchor("CAM_Hallway_Wide").unwrap();
        assert!(matches!(
            validate_camera_candidate(&manifest, anchor, &subject, None),
            Err(CameraRejection::Occluded(node)) if node == "TEST_OCCLUDING_WALL"
        ));
    }

    #[test]
    fn imported_dynamic_door_transform_uses_manifest_pivot() {
        let manifest = manifest();
        let left = manifest.dynamic_object("DOOR_Elevator_Left").unwrap();
        let halfway = dynamic_door_translation(left, 0.5).unwrap();

        assert_eq!(halfway, [-6.45, 1.38, -4.66]);
        assert_eq!(
            dynamic_door_translation(left, 0.0).unwrap(),
            [-5.99, 1.38, -4.66]
        );
    }

    #[test]
    fn greybox_requires_the_explicit_environment_value() {
        assert_eq!(select_set_mode(None).unwrap(), BacklotSetMode::Imported);
        assert_eq!(
            select_set_mode(Some("greybox")).unwrap(),
            BacklotSetMode::Greybox
        );
        assert!(select_set_mode(Some("1")).is_err());
        assert!(select_set_mode(Some("auto")).is_err());
    }
}
