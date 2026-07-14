use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JointTrack {
    pub joint: String,
    pub rotations: Vec<[f32; 4]>,
    #[serde(default)]
    pub translations: Vec<[f32; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessedMotionClip {
    pub schema_version: u32,
    pub semantic: String,
    pub sample_rate: f32,
    pub duration: f32,
    pub tracks: Vec<JointTrack>,
    pub root_positions: Vec<[f32; 3]>,
    /// Kimodo contact channels are preserved verbatim. SOMA77 currently emits
    /// six channels; keeping this dynamic avoids silently discarding toe/heel
    /// information when NVIDIA revises the contact layout.
    #[serde(default)]
    pub foot_contacts: Vec<Vec<bool>>,
    /// Global SOMA contact-joint positions corresponding to `foot_contacts`.
    /// They are retained for rehearsal diagnostics and contact correction.
    #[serde(default)]
    pub foot_positions: Vec<Vec<[f32; 3]>>,
    /// Per-contact correction remaining after root stabilization. Bevy applies
    /// these after clip sampling as bounded foot-lock IK targets.
    #[serde(default)]
    pub foot_lock_offsets: Vec<Vec<[f32; 3]>>,
    #[serde(default)]
    pub contact_channels: Vec<String>,
    pub looping: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipApproval {
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotionManifest {
    pub schema_version: u32,
    pub semantic: String,
    pub cache_key: String,
    pub source_revision: String,
    pub checkpoint: String,
    pub prompt: String,
    pub seed: u64,
    pub approval: ClipApproval,
    pub clip: PathBuf,
    #[serde(default)]
    pub preview: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("motion I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("motion codec failed: {0}")]
    Codec(String),
    #[error("motion manifest failed: {0}")]
    Manifest(#[from] serde_json::Error),
}

#[derive(Debug, Default)]
pub struct MotionLibrary {
    approved: BTreeMap<String, Vec<MotionManifest>>,
    pending: BTreeMap<String, Vec<MotionManifest>>,
    rejected: BTreeMap<String, Vec<MotionManifest>>,
}

impl MotionLibrary {
    pub fn scan(root: &Path) -> Result<Self, LibraryError> {
        let mut library = Self::default();
        if !root.exists() {
            return Ok(library);
        }
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(dir)? {
                let path = entry?.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.file_name().and_then(|v| v.to_str()) == Some("manifest.json") {
                    let mut manifest: MotionManifest = serde_json::from_slice(&fs::read(&path)?)?;
                    let manifest_dir = path.parent().unwrap_or(root);
                    if manifest.clip.is_relative() {
                        manifest.clip = manifest_dir.join(&manifest.clip);
                    }
                    if let Some(preview) = manifest.preview.as_mut() {
                        if preview.is_relative() {
                            *preview = manifest_dir.join(&*preview);
                        }
                    }
                    let destination = match manifest.approval {
                        ClipApproval::Approved => &mut library.approved,
                        ClipApproval::Pending => &mut library.pending,
                        ClipApproval::Rejected => &mut library.rejected,
                    };
                    destination
                        .entry(manifest.semantic.clone())
                        .or_default()
                        .push(manifest);
                }
            }
        }
        Ok(library)
    }

    pub fn approved(&self, semantic: &str) -> &[MotionManifest] {
        self.approved
            .get(semantic)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn pending(&self, semantic: &str) -> &[MotionManifest] {
        self.pending.get(semantic).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn rejected(&self, semantic: &str) -> &[MotionManifest] {
        self.rejected
            .get(semantic)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

pub fn cache_key(parts: &[&str]) -> String {
    let mut hash = blake3::Hasher::new();
    for part in parts {
        hash.update(&(part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    hash.finalize().to_hex().to_string()
}

pub fn write_clip(path: &Path, clip: &ProcessedMotionClip) -> Result<(), LibraryError> {
    let bytes = bincode::serde::encode_to_vec(clip, bincode::config::standard())
        .map_err(|error| LibraryError::Codec(error.to_string()))?;
    fs::write(path, bytes)?;
    Ok(())
}

pub fn read_clip(path: &Path) -> Result<ProcessedMotionClip, LibraryError> {
    let bytes = fs::read(path)?;
    bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
        .map(|(clip, _)| clip)
        .map_err(|error| LibraryError::Codec(error.to_string()))
}
