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
    pub max_frames: u32,
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
            max_frames: 2000,
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
    pub worker_script: PathBuf,
    pub model_root: PathBuf,
    pub request_file: PathBuf,
    pub response_file: PathBuf,
    pub trace_file: Option<PathBuf>,
    pub device: Option<String>,
}

impl GepardConfig {
    pub fn project_default(
        project_root: &Path,
        request_file: PathBuf,
        response_file: PathBuf,
    ) -> Self {
        Self {
            runtime_root: project_root.join("runtimes/gepard"),
            worker_script: PathBuf::from("backlot_gepard_worker.py"),
            model_root: PathBuf::from(r"F:\Models\InfiniteBacklot\gepard-1.0"),
            request_file,
            response_file,
            trace_file: None,
            device: Some("cuda".into()),
        }
    }

    pub fn process_spec(&self) -> ProcessSpec {
        let mut env = BTreeMap::new();
        env.insert("HF_HOME".into(), r"F:\Models\huggingface".into());
        env.insert(
            "HUGGINGFACE_HUB_CACHE".into(),
            r"F:\Models\huggingface\hub".into(),
        );
        // Hermes and other launchers may export a host PYTHONPATH. The pinned
        // uv environment must never import binary wheels from that host venv.
        env.insert("PYTHONPATH".into(), String::new());
        env.insert("PYTHONNOUSERSITE".into(), "1".into());
        let mut args = vec![
            "run".into(),
            "--frozen".into(),
            "--no-sync".into(),
            "python".into(),
            self.worker_script.display().to_string(),
            "--model-root".into(),
            self.model_root.display().to_string(),
            "--requests".into(),
            self.request_file.display().to_string(),
            "--responses".into(),
            self.response_file.display().to_string(),
        ];
        if let Some(device) = self.device.as_deref().filter(|value| !value.is_empty()) {
            args.push("--device".into());
            args.push(device.into());
        }
        let stderr_path = self
            .trace_file
            .as_ref()
            .map(|path| path.with_extension("stderr.log"));
        ProcessSpec {
            program: PathBuf::from("uv"),
            args,
            cwd: self.runtime_root.clone(),
            env,
            stdout_path: self.trace_file.clone(),
            stderr_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serialization_matches_worker_contract() {
        let request = GepardLineRequest {
            id: "line_0001_abc".into(),
            text: "The elevator knows.".into(),
            output: PathBuf::from("audio/dialogue/line_0001_abc.wav"),
            reference_audio: PathBuf::from("ref_audio/nurisa_en.wav"),
            seed: 42_123,
            preset: GepardPreset::default(),
        };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["id"], "line_0001_abc");
        assert_eq!(value["seed"], 42_123);
        assert_eq!(value["preset"]["max_frames"], 2000);
    }

    #[test]
    fn process_spec_uses_configured_worker_device_trace_and_clean_python_path() {
        let cfg = GepardConfig {
            runtime_root: PathBuf::from("runtimes/gepard"),
            worker_script: PathBuf::from("custom_worker.py"),
            model_root: PathBuf::from("F:/Models/gepard"),
            request_file: PathBuf::from("requests.json"),
            response_file: PathBuf::from("responses.json"),
            trace_file: Some(PathBuf::from("trace.jsonl")),
            device: Some("cuda".into()),
        };
        let spec = cfg.process_spec();
        assert!(spec
            .args
            .windows(2)
            .any(|pair| pair == ["--device", "cuda"]));
        assert!(spec.args.iter().any(|arg| arg == "custom_worker.py"));
        assert_eq!(spec.env.get("PYTHONPATH").map(String::as_str), Some(""));
        assert_eq!(spec.stdout_path, Some(PathBuf::from("trace.jsonl")));
    }
}
