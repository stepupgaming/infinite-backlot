use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartInteractionCatalog {
    pub schema_version: u32,
    pub interactions: Vec<SmartInteraction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartInteraction {
    pub semantic_id: String,
    pub compatible_object_types: Vec<String>,
    pub approach_regions: Vec<String>,
    pub staging_slots: Vec<InteractionSlot>,
    pub proxy_keyframes: Vec<ProxyKeyframe>,
    pub end_effector_constraints: Vec<InteractionConstraint>,
    pub contact_events: Vec<InteractionContact>,
    pub runtime_state_transitions: Vec<String>,
    pub exit_state: String,
    pub camera_safe_zones: Vec<String>,
    pub required_clearance: f32,
    pub supported_motion_backends: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionSlot {
    pub id: String,
    pub root_alignment: [f32; 3],
    pub facing: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyKeyframe {
    pub normalized_time: f32,
    pub pose_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionConstraint {
    pub joint: String,
    pub normalized_time: f32,
    pub target_slot: String,
    pub position_offset: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    pub strict: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionContact {
    pub id: String,
    pub normalized_start: f32,
    pub normalized_end: f32,
    pub joint: String,
    pub target_slot: String,
}

#[derive(Debug, Error)]
pub enum SmartInteractionError {
    #[error("could not read smart interaction catalog {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid smart interaction catalog {path}: {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },
}

impl SmartInteractionCatalog {
    pub fn from_path(path: &Path) -> Result<Self, SmartInteractionError> {
        let text = fs::read_to_string(path).map_err(|source| SmartInteractionError::Read {
            path: path.display().to_string(),
            source,
        })?;
        serde_json::from_str(&text).map_err(|source| SmartInteractionError::Parse {
            path: path.display().to_string(),
            source,
        })
    }

    pub fn get(&self, id: &str) -> Option<&SmartInteraction> {
        self.interactions.iter().find(|item| item.semantic_id == id)
    }

    pub fn compatible(&self, interaction_id: &str, object_type: &str) -> bool {
        self.get(interaction_id).is_some_and(|interaction| {
            interaction
                .compatible_object_types
                .iter()
                .any(|candidate| candidate == object_type)
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported smart interaction schema {}",
                self.schema_version
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for interaction in &self.interactions {
            if !ids.insert(&interaction.semantic_id) {
                return Err(format!(
                    "duplicate smart interaction {}",
                    interaction.semantic_id
                ));
            }
            if interaction.semantic_id.trim().is_empty()
                || interaction.compatible_object_types.is_empty()
                || interaction.approach_regions.is_empty()
                || interaction.staging_slots.is_empty()
                || interaction.supported_motion_backends.is_empty()
                || interaction.required_clearance <= 0.0
            {
                return Err(format!(
                    "incomplete smart interaction {}",
                    interaction.semantic_id
                ));
            }
            for keyframe in &interaction.proxy_keyframes {
                if !(0.0..=1.0).contains(&keyframe.normalized_time) {
                    return Err(format!("invalid keyframe in {}", interaction.semantic_id));
                }
            }
            for constraint in &interaction.end_effector_constraints {
                if !(0.0..=1.0).contains(&constraint.normalized_time) {
                    return Err(format!("invalid constraint in {}", interaction.semantic_id));
                }
                let norm = constraint
                    .rotation_xyzw
                    .iter()
                    .map(|v| v * v)
                    .sum::<f32>()
                    .sqrt();
                if (norm - 1.0).abs() > 0.05 {
                    return Err(format!("invalid rotation in {}", interaction.semantic_id));
                }
            }
            for contact in &interaction.contact_events {
                if contact.normalized_start < 0.0
                    || contact.normalized_end > 1.0
                    || contact.normalized_end <= contact.normalized_start
                {
                    return Err(format!("invalid contact in {}", interaction.semantic_id));
                }
            }
        }
        Ok(())
    }
}
