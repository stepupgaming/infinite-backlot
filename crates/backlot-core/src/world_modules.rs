//! Data-driven reusable world-module registry and deterministic layout contract.
//!
//! This layer intentionally contains no Bevy scene-specific code. Blender-authored
//! GLBs and sidecars can enter the registry without adding a Rust scene variant.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldModuleRegistry {
    pub schema_version: u32,
    pub registry_version: u32,
    pub coordinate_system: String,
    pub module_count: usize,
    pub modules: Vec<WorldModule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldModule {
    pub module_id: String,
    pub asset: PathBuf,
    pub source_blend: PathBuf,
    pub category: String,
    pub version: u32,
    pub bounds: ModuleBounds,
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
    pub tags: Vec<String>,
    pub glb_sha256: String,
    pub preview: PathBuf,
    pub provenance: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticPoint {
    pub id: String,
    pub node: String,
    pub position: [f32; 3],
    #[serde(flatten)]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl WorldModuleRegistry {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        serde_json::from_slice(&std::fs::read(path.as_ref()).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())
    }

    pub fn validate(&self, project_root: &Path) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported world registry schema {}",
                self.schema_version
            ));
        }
        if self.module_count != self.modules.len() {
            return Err(format!(
                "module_count {} does not match {} entries",
                self.module_count,
                self.modules.len()
            ));
        }
        let mut module_ids = HashSet::new();
        for module in &self.modules {
            if !module_ids.insert(&module.module_id) {
                return Err(format!("duplicate module id {}", module.module_id));
            }
            for path in [&module.asset, &module.source_blend, &module.preview] {
                if !project_root.join(path).is_file() {
                    return Err(format!(
                        "module {} references missing file {}",
                        module.module_id,
                        path.display()
                    ));
                }
            }
            if module.glb_sha256.len() != 64 {
                return Err(format!("module {} has invalid GLB hash", module.module_id));
            }
            if module
                .bounds
                .min
                .iter()
                .chain(module.bounds.max.iter())
                .any(|value| !value.is_finite())
            {
                return Err(format!("module {} has non-finite bounds", module.module_id));
            }
            if (0..3).any(|axis| module.bounds.min[axis] >= module.bounds.max[axis]) {
                return Err(format!("module {} has inverted bounds", module.module_id));
            }
            validate_unique_points(&module.module_id, "socket", &module.sockets)?;
            validate_unique_points(&module.module_id, "staging mark", &module.staging_marks)?;
            validate_unique_points(&module.module_id, "camera anchor", &module.camera_anchors)?;
            if module.sockets.is_empty()
                || module.staging_marks.is_empty()
                || module.camera_anchors.is_empty()
            {
                return Err(format!(
                    "module {} lacks production semantics",
                    module.module_id
                ));
            }
            if module.collision_groups.is_empty() {
                return Err(format!(
                    "module {} lacks a collision proxy",
                    module.module_id
                ));
            }
            if module.provenance.is_null() {
                return Err(format!("module {} lacks provenance", module.module_id));
            }
        }
        Ok(())
    }

    pub fn by_id(&self) -> HashMap<&str, &WorldModule> {
        self.modules
            .iter()
            .map(|module| (module.module_id.as_str(), module))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedStagingSlot {
    pub id: String,
    pub position: [f32; 3],
    pub facing_target: String,
}

impl WorldModule {
    /// Expand an authored interaction point into exclusive actor marks. The
    /// generated positions stay inside authored module bounds and are evenly
    /// distributed around the target, preventing multiple actors from being
    /// assigned the interaction origin itself.
    pub fn generate_staging_slots(
        &self,
        interaction_id: &str,
        count: usize,
        radius: f32,
    ) -> Result<Vec<GeneratedStagingSlot>, String> {
        if count == 0 || !radius.is_finite() || radius < 0.4 {
            return Err("staging slot count/radius is invalid".into());
        }
        let target = self
            .interactions
            .iter()
            .find(|point| point.id == interaction_id)
            .ok_or_else(|| {
                format!(
                    "module {} has no interaction {interaction_id}",
                    self.module_id
                )
            })?;
        let names = [
            "operator",
            "observer_left",
            "observer_right",
            "reveal_clear",
        ];
        let margin = 0.4;
        let mut slots = Vec::with_capacity(count);
        for index in 0..count {
            let angle = std::f32::consts::TAU * index as f32 / count as f32;
            let x = (target.position[0] + angle.cos() * radius)
                .clamp(self.bounds.min[0] + margin, self.bounds.max[0] - margin);
            let z = (target.position[2] + angle.sin() * radius)
                .clamp(self.bounds.min[2] + margin, self.bounds.max[2] - margin);
            slots.push(GeneratedStagingSlot {
                id: format!(
                    "{}_{}",
                    interaction_id.to_ascii_lowercase(),
                    names.get(index).copied().unwrap_or("neighbor")
                ),
                position: [x, 0.0, z],
                facing_target: interaction_id.to_string(),
            });
        }
        Ok(slots)
    }
}

fn validate_unique_points(
    module_id: &str,
    kind: &str,
    points: &[SemanticPoint],
) -> Result<(), String> {
    let mut ids = HashSet::new();
    for point in points {
        if !ids.insert(&point.id) {
            return Err(format!(
                "module {module_id} has duplicate {kind} {}",
                point.id
            ));
        }
        if point.position.iter().any(|value| !value.is_finite()) {
            return Err(format!(
                "module {module_id} has non-finite {kind} {}",
                point.id
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemonstrationLayout {
    pub schema_version: u32,
    pub world_seed: u64,
    pub registry_version: u32,
    pub layout_algorithm: String,
    pub layout_fingerprint: String,
    pub instances: Vec<ModuleInstance>,
    pub connections: Vec<ModuleConnection>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInstance {
    pub instance_id: String,
    pub role: String,
    pub module_id: String,
    pub module_version: u32,
    pub category: String,
    pub transform: ModuleTransform,
    #[serde(default)]
    pub runtime_state_overrides: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleTransform {
    pub translation: [f32; 3],
    pub yaw_degrees: f32,
    pub scale: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleConnection {
    pub from_role: String,
    pub from_socket: String,
    pub to_role: String,
    pub to_socket: String,
}

impl DemonstrationLayout {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        serde_json::from_slice(&std::fs::read(path.as_ref()).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())
    }

    pub fn validate(&self, registry: &WorldModuleRegistry) -> Result<(), String> {
        if self.schema_version != 1 || self.registry_version != registry.registry_version {
            return Err("layout and registry versions are incompatible".into());
        }
        let modules = registry.by_id();
        let mut roles = HashMap::new();
        for instance in &self.instances {
            let module = modules.get(instance.module_id.as_str()).ok_or_else(|| {
                format!(
                    "instance {} references unknown module {}",
                    instance.role, instance.module_id
                )
            })?;
            if module.version != instance.module_version {
                return Err(format!(
                    "instance {} requests unavailable module version",
                    instance.role
                ));
            }
            if roles.insert(instance.role.as_str(), module).is_some() {
                return Err(format!("duplicate layout role {}", instance.role));
            }
            if instance
                .transform
                .translation
                .iter()
                .chain(instance.transform.scale.iter())
                .any(|value| !value.is_finite())
                || !instance.transform.yaw_degrees.is_finite()
            {
                return Err(format!("instance {} has invalid transform", instance.role));
            }
        }
        for connection in &self.connections {
            let from = roles.get(connection.from_role.as_str()).ok_or_else(|| {
                format!("connection has unknown from role {}", connection.from_role)
            })?;
            let to = roles
                .get(connection.to_role.as_str())
                .ok_or_else(|| format!("connection has unknown to role {}", connection.to_role))?;
            if !from
                .sockets
                .iter()
                .any(|socket| socket.id == connection.from_socket)
            {
                return Err(format!(
                    "{} lacks socket {}",
                    connection.from_role, connection.from_socket
                ));
            }
            if !to
                .sockets
                .iter()
                .any(|socket| socket.id == connection.to_socket)
            {
                return Err(format!(
                    "{} lacks socket {}",
                    connection.to_role, connection.to_socket
                ));
            }
        }
        Ok(())
    }
}
