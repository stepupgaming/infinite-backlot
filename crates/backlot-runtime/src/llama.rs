use crate::process::ProcessSpec;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Project-owned handle to Gemmy's proven persistent llama-server launcher.
/// Gemmy remains the single source of truth for model paths, MTP draft setup,
/// Q4 KV cache parameters, and llama.cpp performance flags.
#[derive(Debug, Clone)]
pub struct Gemma26Config {
    pub gemmy_exe: PathBuf,
    pub model_alias: String,
    pub port: u16,
}

impl Gemma26Config {
    pub fn project_default(_project_root: &Path) -> Self {
        Self {
            gemmy_exe: PathBuf::from(r"C:\Projects\gemmy\target\release\gemmy.exe"),
            model_alias: "gemma-4-26b-mtp".into(),
            port: 8123,
        }
    }

    pub fn process_spec(&self) -> ProcessSpec {
        ProcessSpec {
            program: self.gemmy_exe.clone(),
            args: vec![
                "server".into(),
                "--model".into(),
                self.model_alias.clone(),
                "--port".into(),
                self.port.to_string(),
                "--server-timeout-secs".into(),
                "180".into(),
            ],
            cwd: self
                .gemmy_exe
                .parent()
                .unwrap_or_else(|| Path::new(r"C:\Projects\gemmy"))
                .to_path_buf(),
            env: BTreeMap::new(),
        }
    }
}
