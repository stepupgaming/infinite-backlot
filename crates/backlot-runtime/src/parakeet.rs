use crate::process::ProcessSpec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParakeetRequest {
    pub id: String,
    pub audio: PathBuf,
    pub output: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ParakeetConfig {
    pub runtime_root: PathBuf,
    pub python_executable: PathBuf,
    pub model_id: String,
    pub cache_root: PathBuf,
    pub request_file: PathBuf,
    pub response_file: PathBuf,
}

impl ParakeetConfig {
    pub fn project_default(
        project_root: &Path,
        request_file: PathBuf,
        response_file: PathBuf,
    ) -> Self {
        Self {
            runtime_root: project_root.join("runtimes/parakeet-asr"),
            python_executable: PathBuf::from(
                r"C:\Projects\gemmy\runtimes\parakeet-asr\.venv\Scripts\python.exe",
            ),
            model_id: "nvidia/parakeet-tdt-0.6b-v2".into(),
            cache_root: PathBuf::from(r"F:\Models\huggingface"),
            request_file,
            response_file,
        }
    }

    pub fn process_spec(&self) -> ProcessSpec {
        let mut env = BTreeMap::new();
        env.insert("HF_HOME".into(), self.cache_root.display().to_string());
        env.insert(
            "HUGGINGFACE_HUB_CACHE".into(),
            self.cache_root.join("hub").display().to_string(),
        );
        // Never inherit Hermes' control-plane Python packages into the runtime's
        // pinned uv environment.
        env.insert("PYTHONPATH".into(), String::new());
        env.insert("PYTHONNOUSERSITE".into(), "1".into());
        ProcessSpec {
            program: self.python_executable.clone(),
            args: vec![
                "backlot_parakeet_worker.py".into(),
                "--model-id".into(),
                self.model_id.clone(),
                "--requests".into(),
                self.request_file.display().to_string(),
                "--responses".into(),
                self.response_file.display().to_string(),
            ],
            cwd: self.runtime_root.clone(),
            env,
            stdout_path: None,
            stderr_path: None,
        }
    }
}
