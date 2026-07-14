use crate::process::ProcessSpec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GepardPreset {
    pub temperature: f32,
    pub top_k: u32,
    pub cfg_scale: f32,
    pub cfg_frames: u32,
    pub stop_threshold: f32,
    pub repetition_penalty: f32,
    pub repetition_window: u32,
}

impl Default for GepardPreset {
    fn default() -> Self {
        Self {
            temperature: 0.3,
            top_k: 0,
            cfg_scale: 1.0,
            cfg_frames: 0,
            stop_threshold: 0.5,
            repetition_penalty: 1.0,
            repetition_window: 32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GepardLineRequest {
    pub id: String,
    pub text: String,
    pub output: PathBuf,
    pub reference_audio: PathBuf,
    pub seed: u64,
    pub preset: GepardPreset,
}

#[derive(Debug, Clone)]
pub struct GepardConfig {
    pub runtime_root: PathBuf,
    pub model_root: PathBuf,
    pub request_file: PathBuf,
    pub response_file: PathBuf,
}

impl GepardConfig {
    pub fn project_default(
        project_root: &Path,
        request_file: PathBuf,
        response_file: PathBuf,
    ) -> Self {
        Self {
            runtime_root: project_root.join("runtimes/gepard"),
            model_root: PathBuf::from(r"F:\Models\InfiniteBacklot\gepard-1.0"),
            request_file,
            response_file,
        }
    }

    pub fn process_spec(&self) -> ProcessSpec {
        let mut env = BTreeMap::new();
        env.insert("HF_HOME".into(), r"F:\Models\huggingface".into());
        env.insert(
            "HUGGINGFACE_HUB_CACHE".into(),
            r"F:\Models\huggingface\hub".into(),
        );
        ProcessSpec {
            program: PathBuf::from("uv"),
            args: vec![
                "run".into(),
                "--frozen".into(),
                "--no-sync".into(),
                "python".into(),
                "backlot_gepard_worker.py".into(),
                "--model-root".into(),
                self.model_root.display().to_string(),
                "--requests".into(),
                self.request_file.display().to_string(),
                "--responses".into(),
                self.response_file.display().to_string(),
            ],
            cwd: self.runtime_root.clone(),
            env,
        }
    }
}
