//! Registry-driven connected Blender world contract and runtime proof.

use backlot_core::world_modules::{SemanticPoint, WorldModule, WorldModuleRegistry};
use bevy::prelude::Resource;
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

pub const CONNECTED_WORLD_MODULE_ID: &str = "infinite_backlot_block";
pub const WORLD_REGISTRY_PATH: &str = "assets/world/registry.json";

#[derive(Debug, Clone)]
pub struct ConnectedWorldError(String);

impl ConnectedWorldError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ConnectedWorldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ConnectedWorldError {}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeLightIntent {
    pub id: String,
    pub role: String,
    #[serde(default = "default_point_light")]
    pub light_type: String,
    pub position: [f32; 3],
    #[serde(default = "default_down_direction")]
    pub direction: [f32; 3],
    pub color_rgb: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    #[serde(default)]
    pub spot_angle_degrees: Option<f32>,
    #[serde(default)]
    pub runtime_controlled: bool,
    #[serde(default)]
    pub emissive_node: Option<String>,
}

fn default_point_light() -> String {
    "point".to_string()
}

fn default_down_direction() -> [f32; 3] {
    [0.0, -1.0, 0.0]
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeControlIntent {
    pub id: String,
    pub node: String,
    pub kind: String,
    #[serde(default)]
    pub default_state: String,
}

#[derive(Debug, Clone, Deserialize, Resource)]
pub struct ConnectedWorldManifest {
    pub schema_version: u32,
    pub module_id: String,
    #[serde(rename = "asset")]
    pub runtime_glb: PathBuf,
    pub source_blend: PathBuf,
    pub version: u32,
    pub quality_tier: String,
    pub bounds: backlot_core::world_modules::ModuleBounds,
    #[serde(default)]
    pub sockets: Vec<SemanticPoint>,
    #[serde(default)]
    pub staging_marks: Vec<SemanticPoint>,
    #[serde(default)]
    pub camera_anchors: Vec<SemanticPoint>,
    #[serde(default)]
    pub interactions: Vec<SemanticPoint>,
    #[serde(default)]
    pub cutaway_groups: Vec<SemanticPoint>,
    #[serde(default)]
    pub collision_groups: Vec<SemanticPoint>,
    #[serde(default)]
    pub lighting: Vec<RuntimeLightIntent>,
    #[serde(default)]
    pub runtime_controls: Vec<RuntimeControlIntent>,
    pub glb_sha256: String,
}

impl ConnectedWorldManifest {
    pub fn mark(&self, id: &str) -> Option<&SemanticPoint> {
        self.staging_marks.iter().find(|point| point.id == id)
    }

    pub fn camera(&self, id: &str) -> Option<&SemanticPoint> {
        self.camera_anchors.iter().find(|point| point.id == id)
    }
}

#[derive(Debug, Clone)]
pub struct ConnectedWorldContract {
    pub registry_version: u32,
    pub module: WorldModule,
    pub manifest: ConnectedWorldManifest,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProofPathPoint {
    pub mark_id: String,
    pub position: [f32; 3],
}

const LIGHT_ROLES: &[&str] = &[
    "LIGHT_INTERIOR_KEY",
    "LIGHT_INTERIOR_FILL",
    "LIGHT_PRACTICAL",
    "LIGHT_SIGN",
    "LIGHT_STREET",
    "LIGHT_ALLEY",
    "LIGHT_STORE",
    "LIGHT_EXTERIOR_AMBIENT",
];

pub fn parse_connected_world_str(
    text: &str,
) -> Result<ConnectedWorldManifest, ConnectedWorldError> {
    let manifest: ConnectedWorldManifest = serde_json::from_str(text).map_err(|error| {
        ConnectedWorldError::new(format!("invalid connected-world JSON: {error}"))
    })?;
    validate_connected_world_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_connected_world_manifest(
    manifest: &ConnectedWorldManifest,
) -> Result<(), ConnectedWorldError> {
    if manifest.schema_version != 1 {
        return Err(ConnectedWorldError::new(format!(
            "unsupported connected-world schema {}",
            manifest.schema_version
        )));
    }
    if manifest.module_id.trim().is_empty()
        || manifest.runtime_glb.extension().and_then(|v| v.to_str()) != Some("glb")
    {
        return Err(ConnectedWorldError::new(
            "connected world requires a module_id and .glb runtime asset",
        ));
    }
    if manifest.staging_marks.is_empty()
        || manifest.camera_anchors.is_empty()
        || manifest.interactions.is_empty()
        || manifest.collision_groups.is_empty()
    {
        return Err(ConnectedWorldError::new(
            "connected world lacks staging, camera, interaction, or collision semantics",
        ));
    }
    let mut semantic_ids = HashSet::new();
    for point in manifest
        .sockets
        .iter()
        .chain(&manifest.staging_marks)
        .chain(&manifest.camera_anchors)
        .chain(&manifest.interactions)
        .chain(&manifest.cutaway_groups)
        .chain(&manifest.collision_groups)
    {
        if !semantic_ids.insert(point.id.as_str()) {
            return Err(ConnectedWorldError::new(format!(
                "duplicate connected-world semantic id {}",
                point.id
            )));
        }
        if point.position.iter().any(|value| !value.is_finite()) {
            return Err(ConnectedWorldError::new(format!(
                "semantic {} has non-finite position",
                point.id
            )));
        }
    }
    let mut light_ids = HashSet::new();
    for light in &manifest.lighting {
        if !light_ids.insert(light.id.as_str()) {
            return Err(ConnectedWorldError::new(format!(
                "duplicate runtime light {}",
                light.id
            )));
        }
        if !LIGHT_ROLES.contains(&light.role.as_str()) {
            return Err(ConnectedWorldError::new(format!(
                "unsupported runtime light role {}",
                light.role
            )));
        }
        if !matches!(light.light_type.as_str(), "point" | "spot" | "directional") {
            return Err(ConnectedWorldError::new(format!(
                "unsupported light type {} for {}",
                light.light_type, light.id
            )));
        }
        if light
            .position
            .iter()
            .chain(light.direction.iter())
            .chain(light.color_rgb.iter())
            .chain([&light.intensity, &light.range])
            .any(|value| !value.is_finite())
            || light.intensity <= 0.0
            || light.range <= 0.0
        {
            return Err(ConnectedWorldError::new(format!(
                "runtime light {} has invalid numeric intent",
                light.id
            )));
        }
    }
    let mut control_ids = HashSet::new();
    for control in &manifest.runtime_controls {
        if !control_ids.insert(control.id.as_str()) || control.node.trim().is_empty() {
            return Err(ConnectedWorldError::new(format!(
                "duplicate or empty runtime control {}",
                control.id
            )));
        }
    }
    Ok(())
}

pub fn load_connected_world_contract(
    project_root: impl AsRef<Path>,
    module_id: &str,
) -> Result<ConnectedWorldContract, ConnectedWorldError> {
    let project_root = project_root.as_ref();
    let registry =
        WorldModuleRegistry::load(project_root.join(WORLD_REGISTRY_PATH)).map_err(|error| {
            ConnectedWorldError::new(format!("world registry load failed: {error}"))
        })?;
    registry
        .validate(project_root)
        .map_err(|error| ConnectedWorldError::new(format!("world registry invalid: {error}")))?;
    let module = registry
        .modules
        .iter()
        .find(|module| module.module_id == module_id)
        .cloned()
        .ok_or_else(|| {
            ConnectedWorldError::new(format!("world registry has no module {module_id}"))
        })?;
    let sidecar_path = project_root.join(module.asset.with_extension("scene.json"));
    let text = std::fs::read_to_string(&sidecar_path).map_err(|error| {
        ConnectedWorldError::new(format!(
            "failed to read connected-world sidecar {}: {error}",
            sidecar_path.display()
        ))
    })?;
    let manifest = parse_connected_world_str(&text)?;
    if manifest.module_id != module.module_id
        || manifest.runtime_glb != module.asset
        || manifest.glb_sha256 != module.glb_sha256
    {
        return Err(ConnectedWorldError::new(
            "registry, connected-world sidecar, and GLB hash contract disagree",
        ));
    }
    if !project_root.join(&manifest.runtime_glb).is_file()
        || !project_root.join(&manifest.source_blend).is_file()
    {
        return Err(ConnectedWorldError::new(
            "connected-world source or runtime GLB is missing",
        ));
    }
    Ok(ConnectedWorldContract {
        registry_version: registry.registry_version,
        module,
        manifest,
    })
}

pub fn lobby_to_odd_hours_path(
    manifest: &ConnectedWorldManifest,
) -> Result<Vec<ProofPathPoint>, ConnectedWorldError> {
    let ids = [
        "MARK_MASTER_LOBBY",
        "MARK_MASTER_ENTRANCE",
        "MARK_MASTER_STREET",
        "MARK_STREET_CORNER_TWO_SHOT_B",
        "MARK_STORE_WINDOW",
        "MARK_STORE_ENTRANCE",
        "MARK_MASTER_STORE",
    ];
    ids.into_iter()
        .map(|id| {
            let mark = manifest.mark(id).ok_or_else(|| {
                ConnectedWorldError::new(format!("proof path requires authored mark {id}"))
            })?;
            Ok(ProofPathPoint {
                mark_id: id.to_string(),
                position: mark.position,
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct ProofPathSample {
    pub position: [f32; 3],
    pub forward: bevy::prelude::Vec3,
}

pub fn sample_proof_path(
    path: &[ProofPathPoint],
    normalized: f32,
) -> Result<ProofPathSample, ConnectedWorldError> {
    if path.len() < 2 {
        return Err(ConnectedWorldError::new(
            "proof path requires at least two authored marks",
        ));
    }
    let lengths = path
        .windows(2)
        .map(|window| {
            (bevy::prelude::Vec3::from_array(window[1].position)
                - bevy::prelude::Vec3::from_array(window[0].position))
            .length()
        })
        .collect::<Vec<_>>();
    let total = lengths.iter().sum::<f32>();
    if !total.is_finite() || total <= 0.001 {
        return Err(ConnectedWorldError::new("proof path has zero length"));
    }
    let target = normalized.clamp(0.0, 1.0) * total;
    let mut traversed = 0.0;
    for (index, length) in lengths.iter().copied().enumerate() {
        if target <= traversed + length || index + 1 == lengths.len() {
            let start = bevy::prelude::Vec3::from_array(path[index].position);
            let end = bevy::prelude::Vec3::from_array(path[index + 1].position);
            let segment_t = ((target - traversed) / length.max(0.001)).clamp(0.0, 1.0);
            let direction = (end - start).normalize_or_zero();
            return Ok(ProofPathSample {
                position: start.lerp(end, segment_t).to_array(),
                forward: if direction.length_squared() > 0.5 {
                    direction
                } else {
                    bevy::prelude::Vec3::Z
                },
            });
        }
        traversed += length;
    }
    Err(ConnectedWorldError::new("proof path sampling failed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const MASTER_SIDECAR: &str =
        include_str!("../../../assets/world/neighborhood/infinite_backlot_block.scene.json");

    #[test]
    fn parses_the_real_connected_master_sidecar() {
        let manifest = parse_connected_world_str(MASTER_SIDECAR)
            .expect("the real connected master sidecar should parse");
        assert_eq!(manifest.module_id, "infinite_backlot_block");
        assert_eq!(
            manifest.runtime_glb.to_string_lossy(),
            "assets/world/neighborhood/infinite_backlot_block.glb"
        );
        assert!(manifest.staging_marks.len() >= 20);
        assert!(manifest.camera_anchors.len() >= 10);
        assert!(manifest
            .interactions
            .iter()
            .any(|point| { point.id == "TRANSITION_SIDEWALK_TO_STORE" }));
    }

    #[test]
    fn registry_resolves_master_without_a_rust_asset_duplicate() {
        let contract = load_connected_world_contract(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
            "infinite_backlot_block",
        )
        .expect("registry and sidecar should resolve the master GLB");
        assert_eq!(contract.module.module_id, contract.manifest.module_id);
        assert_eq!(contract.registry_version, 2);
        assert_eq!(contract.module.asset, contract.manifest.runtime_glb);
        assert_eq!(contract.module.glb_sha256, contract.manifest.glb_sha256);
    }

    #[test]
    fn real_master_declares_runtime_lights_and_openable_doors() {
        let manifest = parse_connected_world_str(MASTER_SIDECAR).unwrap();
        assert!(manifest.lighting.len() >= 8);
        let roles = manifest
            .lighting
            .iter()
            .map(|light| light.role.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert!(roles.contains("LIGHT_INTERIOR_KEY"));
        assert!(roles.contains("LIGHT_STREET"));
        assert!(roles.contains("LIGHT_STORE"));
        assert!(manifest
            .runtime_controls
            .iter()
            .any(|control| { control.id == "CONTROL_MAIN_ENTRY" && control.kind == "door" }));
        assert!(manifest
            .runtime_controls
            .iter()
            .any(|control| { control.id == "CONTROL_STORE_ENTRY" && control.kind == "door" }));
    }

    #[test]
    fn typed_light_policy_rejects_unknown_roles() {
        let mut value: serde_json::Value = serde_json::from_str(MASTER_SIDECAR).unwrap();
        value["lighting"] = serde_json::json!([{
            "id": "LIGHT_BAD",
            "role": "LIGHT_UNKNOWN",
            "light_type": "point",
            "position": [0.0, 2.0, 0.0],
            "direction": [0.0, -1.0, 0.0],
            "color_rgb": [1.0, 1.0, 1.0],
            "intensity": 1000.0,
            "range": 8.0,
            "runtime_controlled": false
        }]);
        let error = parse_connected_world_str(&serde_json::to_string(&value).unwrap()).unwrap_err();
        assert!(error.to_string().contains("LIGHT_UNKNOWN"));
    }

    #[test]
    fn path_sampler_is_deterministic_and_faces_forward() {
        let manifest = parse_connected_world_str(MASTER_SIDECAR).unwrap();
        let path = lobby_to_odd_hours_path(&manifest).unwrap();
        let first = sample_proof_path(&path, 0.0).unwrap();
        let middle = sample_proof_path(&path, 0.5).unwrap();
        let last = sample_proof_path(&path, 1.0).unwrap();
        assert_eq!(first.position, path.first().unwrap().position);
        assert_eq!(last.position, path.last().unwrap().position);
        assert!(middle.position.iter().all(|value| value.is_finite()));
        assert!(middle.forward.length() > 0.99);
    }

    #[test]
    fn proof_path_uses_authored_marks_and_crosses_both_doors() {
        let manifest = parse_connected_world_str(MASTER_SIDECAR).unwrap();
        let path = lobby_to_odd_hours_path(&manifest).unwrap();
        assert_eq!(path.first().unwrap().mark_id, "MARK_MASTER_LOBBY");
        assert_eq!(path.last().unwrap().mark_id, "MARK_MASTER_STORE");
        assert!(path
            .iter()
            .any(|point| point.mark_id == "MARK_MASTER_ENTRANCE"));
        assert!(path
            .iter()
            .any(|point| point.mark_id == "MARK_STORE_ENTRANCE"));
        assert!(path.windows(2).all(|window| {
            let delta = bevy::prelude::Vec3::from_array(window[1].position)
                - bevy::prelude::Vec3::from_array(window[0].position);
            delta.length() <= 8.5
        }));
    }
}
