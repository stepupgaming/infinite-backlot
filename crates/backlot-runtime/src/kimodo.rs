use crate::process::ProcessSpec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootWaypoint {
    pub frame: u32,
    pub x: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimodoRequest {
    pub semantic: String,
    pub prompt: String,
    pub duration: f32,
    pub output_stem: PathBuf,
    pub seed: u64,
    #[serde(default)]
    pub root_waypoints: Vec<RootWaypoint>,
    #[serde(default)]
    pub constraints: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct KimodoConfig {
    pub runtime_root: PathBuf,
    pub python_executable: PathBuf,
    pub checkpoint: PathBuf,
    pub request_file: PathBuf,
    pub response_file: PathBuf,
}

impl KimodoConfig {
    pub fn project_default(
        project_root: &Path,
        request_file: PathBuf,
        response_file: PathBuf,
    ) -> Self {
        Self {
            runtime_root: project_root.join("runtimes/kimodo"),
            python_executable: PathBuf::from(
                r"C:\Projects\gemmy\runtimes\kimodo\.venv\Scripts\python.exe",
            ),
            checkpoint: PathBuf::from(r"F:\Models\Kimodo\Kimodo-SOMA-RP-v1.1"),
            request_file,
            response_file,
        }
    }

    pub fn process_spec(&self) -> ProcessSpec {
        let mut env = BTreeMap::new();
        env.insert("HF_HOME".into(), r"F:\Models\huggingface".into());
        env.insert("TEXT_ENCODER_DEVICE".into(), "cpu".into());
        env.insert("HF_HUB_OFFLINE".into(), "1".into());
        env.insert("TRANSFORMERS_OFFLINE".into(), "1".into());
        ProcessSpec {
            program: self.python_executable.clone(),
            args: vec![
                "backlot_run_kimodo.py".into(),
                "--checkpoint".into(),
                self.checkpoint.display().to_string(),
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
