use crate::config::AsrConfig;
use backlot_runtime::parakeet::{ParakeetConfig, ParakeetRequest};
use backlot_runtime::{ModelRuntimeManager, RuntimeKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTiming {
    pub text: String,
    pub start: f32,
    pub end: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordAlignment {
    pub text: String,
    pub words: Vec<WordTiming>,
    #[serde(rename = "durationSeconds")]
    pub duration_seconds: f32,
}

#[derive(Debug, Clone, Default)]
pub struct AlignmentBatch {
    pub alignments: HashMap<String, WordAlignment>,
    pub cache_hits: u32,
    pub cache_misses: u32,
    pub elapsed_secs: f32,
}

#[derive(Debug, Deserialize)]
struct WorkerResponse {
    id: String,
    output: String,
    success: bool,
}

pub fn align_wavs(
    cfg: &AsrConfig,
    wavs: &[(String, String)],
) -> crate::error::Result<AlignmentBatch> {
    let started = Instant::now();
    if cfg.provider != "parakeet" || wavs.is_empty() {
        return Ok(AlignmentBatch {
            elapsed_secs: started.elapsed().as_secs_f32(),
            ..Default::default()
        });
    }
    let cache_root = PathBuf::from(&cfg.cache_dir);
    std::fs::create_dir_all(&cache_root).map_err(|source| crate::error::CoreError::Io {
        path: cache_root.clone(),
        source,
    })?;
    let mut batch = AlignmentBatch::default();
    let mut requests = Vec::new();
    for (id, wav) in wavs {
        let wav_hash = std::fs::read(wav)
            .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
            .unwrap_or_else(|_| blake3::hash(wav.as_bytes()).to_hex().to_string());
        let output = cache_root.join(format!("parakeet_{wav_hash}.words.json"));
        if let Ok(bytes) = std::fs::read(&output) {
            if let Ok(alignment) = serde_json::from_slice::<WordAlignment>(&bytes) {
                batch.alignments.insert(id.clone(), alignment);
                batch.cache_hits += 1;
                continue;
            }
        }
        batch.cache_misses += 1;
        requests.push(ParakeetRequest {
            id: id.clone(),
            audio: PathBuf::from(wav),
            output,
        });
    }

    if !requests.is_empty() {
        let batch_id = uuid::Uuid::new_v4().simple().to_string();
        let request_file = cache_root.join(format!("batch_{batch_id}.request.json"));
        let response_file = cache_root.join(format!("batch_{batch_id}.response.json"));
        std::fs::write(
            &request_file,
            serde_json::to_vec_pretty(&requests).unwrap_or_default(),
        )
        .map_err(|source| crate::error::CoreError::Io {
            path: request_file.clone(),
            source,
        })?;
        let worker = ParakeetConfig {
            runtime_root: PathBuf::from(&cfg.runtime_root),
            python_executable: PathBuf::from(
                r"C:\Projects\gemmy\runtimes\parakeet-asr\.venv\Scripts\python.exe",
            ),
            model_id: cfg.model_id.clone(),
            cache_root: PathBuf::from(r"F:\Models\huggingface"),
            request_file: request_file.clone(),
            response_file: response_file.clone(),
        };
        let mut runtime = ModelRuntimeManager::default();
        runtime
            .start(
                RuntimeKind::Parakeet,
                worker.process_spec(),
                Some(cfg.model_id.clone()),
            )
            .map_err(|error| crate::error::CoreError::Msg(error.to_string()))?;
        let completed = runtime
            .wait_for_exit(Duration::from_secs_f32(cfg.timeout_secs.max(30.0)))
            .map_err(|error| crate::error::CoreError::Msg(error.to_string()))?;
        let _ = runtime.mark_work_complete(batch.cache_hits, batch.cache_misses);
        let _ = runtime.stop();
        if !completed {
            return Err(crate::error::CoreError::Msg(
                "Parakeet alignment worker failed or timed out".into(),
            ));
        }
        let responses: Vec<WorkerResponse> =
            serde_json::from_slice(&std::fs::read(&response_file).map_err(|source| {
                crate::error::CoreError::Io {
                    path: response_file.clone(),
                    source,
                }
            })?)?;
        for response in responses.into_iter().filter(|response| response.success) {
            if let Ok(bytes) = std::fs::read(&response.output) {
                if let Ok(alignment) = serde_json::from_slice::<WordAlignment>(&bytes) {
                    batch.alignments.insert(response.id, alignment);
                }
            }
        }
        let _ = std::fs::remove_file(request_file);
        let _ = std::fs::remove_file(response_file);
    }
    batch.elapsed_secs = started.elapsed().as_secs_f32();
    Ok(batch)
}
