//! Text-to-speech abstraction.
//!
//! The product needs spoken dialogue. TTS is plug-able: a real local engine
//! (here `espeak-ng`, invoked as a process) can be dropped in behind this
//! trait, or the duration-only `EstimatingTts` stub can be used when no audio
//! engine is configured. Real synthesis produces a WAV file and *measures* its
//! duration, which is what drives accurate dialogue timing (the estimating
//! stub only predicts).

use crate::config::{GepardTtsConfig, HttpTtsConfig, TtsConfig, VoiceProfileConfig};
use crate::protocol::{DeliveryEmotion, DeliveryPace, DeliverySpec};
use backlot_runtime::gepard::{GepardConfig, GepardLineRequest, GepardPreset};
use backlot_runtime::{ModelRuntimeManager, RuntimeKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsResult {
    /// Path to generated audio, if any.
    pub audio_path: Option<String>,
    /// Measured/estimated duration in seconds.
    pub duration: f32,
    /// Whether this came from cache.
    pub cached: bool,
    /// Whether synthesis succeeded (false => no real audio was produced).
    pub ok: bool,
    /// Which provider produced this result.
    pub provider: String,
}

#[derive(Debug, Clone)]
pub struct TtsRequest {
    pub id: String,
    pub actor_id: String,
    pub text: String,
    pub voice_id: String,
    pub delivery: Option<DeliverySpec>,
}

pub trait Tts: Send + Sync {
    /// Estimate how long `text` will take to speak (seconds).
    fn estimate_duration(&self, text: &str) -> f32 {
        let words = text.split_whitespace().count().max(1) as f32;
        (words * 0.34 + 0.35).clamp(0.6, 14.0)
    }

    /// Synthesize `text` for `voice_id`. Implementations should cache by hash.
    fn synthesize(&self, text: &str, voice_id: &str) -> TtsResult;

    /// Batch providers override this so a model loads once for all cache misses.
    fn synthesize_batch(&self, requests: &[TtsRequest]) -> Vec<TtsResult> {
        requests
            .iter()
            .map(|request| self.synthesize(&request.text, &request.voice_id))
            .collect()
    }

    /// Human-readable provider id, used in diagnostics.
    fn provider_name(&self) -> &'static str;
}

/// Duration-only provider used when no audio engine is configured.
#[derive(Debug, Clone, Default)]
pub struct EstimatingTts;

impl Tts for EstimatingTts {
    fn synthesize(&self, text: &str, _voice_id: &str) -> TtsResult {
        TtsResult {
            audio_path: None,
            duration: self.estimate_duration(text),
            cached: false,
            ok: false,
            provider: "estimating".into(),
        }
    }
    fn provider_name(&self) -> &'static str {
        "estimating"
    }
}

/// Real local TTS via the `espeak-ng` CLI. Each line is synthesized to a WAV
/// file cached by content hash, and the *measured* duration is read back from
/// the WAV header so dialogue timing is exact rather than estimated.
pub struct EspeakTts {
    exe: String,
    sample_rate: u32,
    cache_dir: String,
    amplitude: i32,
    speed: f32,
    default_voice: String,
    voice_map: std::collections::HashMap<String, String>,
}

impl EspeakTts {
    pub fn new(cfg: &TtsConfig) -> Self {
        Self {
            exe: cfg.executable.clone(),
            sample_rate: cfg.sample_rate,
            cache_dir: Path::new(&cfg.cache_dir)
                .join("espeak")
                .to_string_lossy()
                .into_owned(),
            amplitude: cfg.amplitude,
            speed: cfg.speed,
            default_voice: cfg.default_voice.clone(),
            voice_map: cfg.voice_map.clone(),
        }
    }

    /// Resolve the espeak voice name for a character/voice id.
    pub fn voice_for(&self, voice_id: &str) -> String {
        self.voice_map
            .get(voice_id)
            .cloned()
            .unwrap_or_else(|| self.default_voice.clone())
    }

    /// Best-effort reachability probe for the configured binary.
    pub fn available(&self) -> bool {
        std::process::Command::new(&self.exe)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl Tts for EspeakTts {
    fn synthesize(&self, text: &str, voice_id: &str) -> TtsResult {
        let key = line_key(text, voice_id);
        let dir = Path::new(&self.cache_dir);
        let _ = std::fs::create_dir_all(dir);
        let wav = dir.join(format!("{key}.wav"));

        if wav.exists() {
            if let Some(d) = wav_duration_secs(wav.to_string_lossy().as_ref()) {
                return TtsResult {
                    audio_path: Some(wav.to_string_lossy().into_owned()),
                    duration: d,
                    cached: true,
                    ok: true,
                    provider: "espeak".into(),
                };
            }
        }

        let voice = self.voice_for(voice_id);
        let wpm = (175.0 * self.speed).round().clamp(40.0, 800.0) as i32;
        let out = std::process::Command::new(&self.exe)
            .arg("-v")
            .arg(&voice)
            .arg("-s")
            .arg(wpm.to_string())
            .arg("-a")
            .arg(self.amplitude.to_string())
            .arg("-w")
            .arg(wav.to_string_lossy().as_ref())
            .arg(text)
            .output();

        match out {
            Ok(status) if status.status.success() && wav.exists() => {
                match wav_duration_secs(wav.to_string_lossy().as_ref()) {
                    Some(d) => TtsResult {
                        audio_path: Some(wav.to_string_lossy().into_owned()),
                        duration: d,
                        cached: false,
                        ok: true,
                        provider: "espeak".into(),
                    },
                    None => self.fallback(text),
                }
            }
            _ => self.fallback(text),
        }
    }

    fn provider_name(&self) -> &'static str {
        "espeak"
    }
}

impl EspeakTts {
    fn fallback(&self, text: &str) -> TtsResult {
        TtsResult {
            audio_path: None,
            duration: self.estimate_duration(text),
            cached: false,
            ok: false,
            provider: "espeak-failed".into(),
        }
    }
}

/// Local OpenAI-compatible HTTP TTS provider. Calls a configurable
/// `/v1/audio/speech` endpoint (Kokoro, XTTS, AllTalk, Piper-HTTP, etc.) via a
/// `curl` subprocess, so backlot-core stays free of async HTTP dependencies.
/// Output WAV/PCM is cached by content hash and measured like espeak.
pub struct HttpTts {
    cfg: HttpTtsConfig,
    cache_dir: String,
}

impl HttpTts {
    pub fn new(cfg: HttpTtsConfig, cache_dir: String) -> Self {
        Self {
            cfg,
            cache_dir: Path::new(&cache_dir)
                .join("http")
                .to_string_lossy()
                .into_owned(),
        }
    }

    pub fn voice_for(&self, voice_id: &str) -> String {
        self.cfg
            .voice_map
            .get(voice_id)
            .cloned()
            .unwrap_or_else(|| self.cfg.default_voice.clone())
    }
}

/// Project-owned Gepard provider. It never leaves a server resident: cache
/// misses are serialized to one JSON batch, a uv-locked worker loads the model
/// once, produces every line, exits, and only measured WAV durations are used.
pub struct GepardTts {
    cfg: GepardTtsConfig,
    cache_dir: PathBuf,
    episode_audio_dir: Option<PathBuf>,
}

impl GepardTts {
    pub fn new(cfg: GepardTtsConfig, cache_dir: String) -> Self {
        Self {
            cfg,
            cache_dir: PathBuf::from(cache_dir).join("gepard_batch"),
            episode_audio_dir: None,
        }
    }

    pub fn with_episode_audio_dir(mut self, path: PathBuf) -> Self {
        self.episode_audio_dir = Some(absolute_path(&path));
        self
    }

    fn profile(&self, voice_id: &str) -> Option<&VoiceProfileConfig> {
        self.cfg.profiles.get(voice_id).or_else(|| {
            self.cfg
                .profiles
                .values()
                .find(|p| p.character_id == voice_id)
        })
    }

    fn reference_hash(profile: &VoiceProfileConfig) -> Option<String> {
        let bytes = std::fs::read(&profile.reference_wav).ok()?;
        let actual = blake3::hash(&bytes).to_hex().to_string();
        if !profile.reference_hash.is_empty() && profile.reference_hash != actual {
            return None;
        }
        Some(actual)
    }

    pub fn resolve_voice(&self, voice_id: &str) -> Option<ResolvedVoice> {
        let profile = self.profile(voice_id)?;
        Some(ResolvedVoice {
            character_id: profile.character_id.clone(),
            voice_id: if profile.voice_id.is_empty() {
                profile.character_id.clone()
            } else {
                profile.voice_id.clone()
            },
            reference_audio: absolute_path(Path::new(&profile.reference_wav)),
            reference_hash: Self::reference_hash(profile)?,
            seed_base: profile.seed,
            status: profile.status,
        })
    }

    fn preset_for(&self, delivery: Option<&DeliverySpec>) -> GepardPreset {
        let configured = &self.cfg.default_preset;
        let mut preset = GepardPreset {
            temperature: configured.temperature,
            top_k: configured.top_k,
            cfg_scale: configured.cfg_scale,
            cfg_frames: configured.cfg_frames,
            stop_threshold: configured.stop_threshold,
            max_frames: configured.max_frames,
            repetition_penalty: configured.repetition_penalty,
            repetition_window: configured.repetition_window,
        };
        if let Some(delivery) = delivery {
            preset.temperature = match delivery.emotion {
                DeliveryEmotion::Hushed => preset.temperature.min(0.25),
                DeliveryEmotion::Warm | DeliveryEmotion::Amused => preset.temperature.max(0.35),
                DeliveryEmotion::Urgent | DeliveryEmotion::Stunned => preset.temperature.max(0.40),
                _ => preset.temperature,
            };
        }
        preset
    }

    fn resolved_seed(&self, request: &TtsRequest, voice: &ResolvedVoice) -> u64 {
        let hash = blake3::hash(request.id.as_bytes());
        let mut suffix = [0u8; 8];
        suffix.copy_from_slice(&hash.as_bytes()[..8]);
        voice.seed_base.wrapping_add(u64::from_le_bytes(suffix))
    }

    pub fn cache_identity(&self, request: &TtsRequest) -> Option<GepardCacheIdentity> {
        let voice = self.resolve_voice(&request.voice_id)?;
        let preset = self.preset_for(request.delivery.as_ref());
        let seed = self.resolved_seed(request, &voice);
        let payload = serde_json::json!({
            "provider": "gepard_batch",
            "model_revision": self.cfg.model_revision,
            "codec_revision": self.cfg.codec_revision,
            "runtime_version": self.cfg.runtime_version,
            "character_voice_id": voice.voice_id,
            "reference_hash": voice.reference_hash,
            "text": normalize_spoken_text(&request.text),
            "seed": seed,
            "preset": preset,
        });
        Some(GepardCacheIdentity {
            key: blake3::hash(payload.to_string().as_bytes())
                .to_hex()
                .to_string(),
            provider: "gepard_batch".into(),
            normalized_text: normalize_spoken_text(&request.text),
            model_revision: self.cfg.model_revision.clone(),
            codec_revision: self.cfg.codec_revision.clone(),
            runtime_version: self.cfg.runtime_version.clone(),
            character_id: voice.character_id,
            voice_id: voice.voice_id,
            reference_audio: voice.reference_audio,
            reference_hash: voice.reference_hash,
            seed,
            preset,
        })
    }

    fn failed(&self, request: &TtsRequest) -> TtsResult {
        TtsResult {
            audio_path: None,
            duration: self.estimate_duration(&request.text),
            cached: false,
            ok: false,
            provider: "gepard_batch-failed".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedVoice {
    pub character_id: String,
    pub voice_id: String,
    pub reference_audio: PathBuf,
    pub reference_hash: String,
    pub seed_base: u64,
    pub status: crate::config::VoiceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GepardCacheIdentity {
    pub key: String,
    pub provider: String,
    pub normalized_text: String,
    pub model_revision: String,
    pub codec_revision: String,
    pub runtime_version: String,
    pub character_id: String,
    pub voice_id: String,
    pub reference_audio: PathBuf,
    pub reference_hash: String,
    pub seed: u64,
    pub preset: GepardPreset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GepardWorkerResponse {
    id: String,
    output: String,
    #[serde(default)]
    sample_rate: u32,
    duration: f32,
    #[serde(default)]
    elapsed_ms: u64,
    success: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GepardManifestLine {
    pub id: String,
    pub actor_id: String,
    pub text: String,
    pub voice: ResolvedVoice,
    pub cache: GepardCacheIdentity,
    pub cached: bool,
    pub output: String,
    pub sample_rate: u32,
    pub duration: f32,
    pub elapsed_ms: u64,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GepardBatchManifest {
    pub schema_version: u32,
    pub provider: String,
    pub runtime_root: String,
    pub worker_script: String,
    pub model_root: String,
    pub model_revision: String,
    pub codec_revision: String,
    pub runtime_version: String,
    pub device: String,
    pub line_count: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub worker_invocations: u32,
    pub request_preparation_secs: f32,
    pub model_load_secs: f32,
    pub dialogue_generation_secs: f32,
    pub audio_verification_secs: f32,
    pub gpu_memory: Vec<serde_json::Value>,
    pub errors: Vec<String>,
    pub lines: Vec<GepardManifestLine>,
}

fn validate_worker_responses(
    expected: &[TtsRequest],
    responses: &[GepardWorkerResponse],
) -> Vec<String> {
    let mut errors = Vec::new();
    let mut by_id: HashMap<&str, Vec<&GepardWorkerResponse>> = HashMap::new();
    for response in responses {
        by_id.entry(&response.id).or_default().push(response);
    }
    for request in expected {
        match by_id.get(request.id.as_str()) {
            None => errors.push(format!("missing response for {}", request.id)),
            Some(values) if values.len() != 1 => errors.push(format!(
                "expected exactly one response for {}, got {}",
                request.id,
                values.len()
            )),
            Some(values) => {
                let response = values[0];
                if !response.success {
                    errors.push(format!(
                        "{} failed: {}",
                        request.id,
                        response
                            .error
                            .as_deref()
                            .unwrap_or("worker reported failure")
                    ));
                }
                if !Path::new(&response.output).is_file() {
                    errors.push(format!("{} missing WAV: {}", request.id, response.output));
                } else if wav_duration_secs(&response.output).unwrap_or(0.0) <= 0.0 {
                    errors.push(format!("{} WAV is unreadable or empty", request.id));
                }
            }
        }
    }
    for id in by_id.keys() {
        if !expected.iter().any(|request| request.id == *id) {
            errors.push(format!("unexpected response id {id}"));
        }
    }
    errors
}

impl Tts for GepardTts {
    fn synthesize(&self, text: &str, voice_id: &str) -> TtsResult {
        let request = TtsRequest {
            id: stable_line_id(0, voice_id, text),
            actor_id: voice_id.into(),
            text: text.into(),
            voice_id: voice_id.into(),
            delivery: None,
        };
        self.synthesize_batch(std::slice::from_ref(&request))
            .into_iter()
            .next()
            .unwrap_or_else(|| self.failed(&request))
    }

    fn synthesize_batch(&self, requests: &[TtsRequest]) -> Vec<TtsResult> {
        let request_prep_started = std::time::Instant::now();
        let _ = std::fs::create_dir_all(&self.cache_dir);
        let persistent = self.episode_audio_dir.is_some();
        let batch_root = self.episode_audio_dir.clone().unwrap_or_else(|| {
            self.cache_dir
                .join("batches")
                .join(uuid::Uuid::new_v4().simple().to_string())
        });
        let dialogue_dir = batch_root.join("dialogue");
        let _ = std::fs::create_dir_all(&dialogue_dir);
        let request_file = batch_root.join("gepard_requests.json");
        let response_file = batch_root.join("gepard_responses.json");
        let trace_file = batch_root.join("gepard_trace.jsonl");
        let manifest_file = batch_root.join("gepard_manifest.json");
        let stderr_file = trace_file.with_extension("stderr.log");
        // A repeated episode build must never reuse stale response or trace
        // records. Cache hits are represented explicitly in the new manifest.
        for path in [
            &request_file,
            &response_file,
            &trace_file,
            &stderr_file,
            &manifest_file,
        ] {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::write(&response_file, b"[]");
        let _ = std::fs::write(&trace_file, b"");

        let mut results: Vec<Option<TtsResult>> = vec![None; requests.len()];
        let mut pending: Vec<GepardLineRequest> = Vec::new();
        let mut pending_requests: Vec<TtsRequest> = Vec::new();
        let mut ids: HashMap<String, usize> = HashMap::new();
        let mut identities: Vec<Option<(ResolvedVoice, GepardCacheIdentity, PathBuf)>> =
            vec![None; requests.len()];
        let mut cache_hits = 0usize;
        let mut errors = Vec::new();

        for (index, request) in requests.iter().enumerate() {
            let Some(voice) = self.resolve_voice(&request.voice_id) else {
                errors.push(format!(
                    "{} has no valid voice profile or reference hash",
                    request.id
                ));
                results[index] = Some(self.failed(request));
                continue;
            };
            let Some(identity) = self.cache_identity(request) else {
                errors.push(format!(
                    "{} cache identity could not be resolved",
                    request.id
                ));
                results[index] = Some(self.failed(request));
                continue;
            };
            let cache_wav = self.cache_dir.join(format!("{}.wav", identity.key));
            let output_wav = if persistent {
                dialogue_dir.join(format!("{}.wav", request.id))
            } else {
                cache_wav.clone()
            };
            identities[index] = Some((voice.clone(), identity.clone(), output_wav.clone()));
            // Gepard/NanoCodec emits 22.05 kHz. Older Backlot builds could
            // corrupt the cache by relabelling those samples as 44.1 kHz while
            // trimming silence. Reject that cache entry so the real worker
            // regenerates it instead of replaying pitch-doubled audio.
            if !self.cfg.cache_bypass
                && wav_sample_rate(&cache_wav.to_string_lossy()) == Some(22_050)
            {
                if let Some(duration) = wav_duration_secs(&cache_wav.to_string_lossy()) {
                    if persistent && std::fs::copy(&cache_wav, &output_wav).is_err() {
                        errors.push(format!("{} cached WAV could not be staged", request.id));
                        results[index] = Some(self.failed(request));
                        continue;
                    }
                    cache_hits += 1;
                    results[index] = Some(TtsResult {
                        audio_path: Some(output_wav.to_string_lossy().into_owned()),
                        duration,
                        cached: true,
                        ok: true,
                        provider: "gepard_batch".into(),
                    });
                    continue;
                }
            }
            ids.insert(request.id.clone(), index);
            pending_requests.push(request.clone());
            pending.push(GepardLineRequest {
                id: request.id.clone(),
                text: shape_prosody_text(&request.text, request.delivery.as_ref()),
                output: output_wav,
                reference_audio: voice.reference_audio,
                seed: identity.seed,
                preset: identity.preset,
            });
        }

        let _ = std::fs::write(
            &request_file,
            serde_json::to_vec_pretty(&pending).unwrap_or_default(),
        );
        let request_preparation_secs = request_prep_started.elapsed().as_secs_f32();
        let mut worker_invocations = 0u32;
        if !pending.is_empty() {
            worker_invocations = 1;
            let worker = GepardConfig {
                runtime_root: absolute_path(Path::new(&self.cfg.runtime_root)),
                worker_script: PathBuf::from(&self.cfg.worker_script),
                model_root: absolute_path(Path::new(&self.cfg.model_root)),
                request_file: absolute_path(&request_file),
                response_file: absolute_path(&response_file),
                trace_file: Some(absolute_path(&trace_file)),
                device: Some(self.cfg.device.clone()),
            };
            let mut manager = ModelRuntimeManager::default();
            match manager.start(
                RuntimeKind::Gepard,
                worker.process_spec(),
                Some(self.cfg.model_revision.clone()),
            ) {
                Ok(_) => {
                    let completed = manager
                        .wait_for_exit(Duration::from_secs_f32(self.cfg.timeout_secs.max(30.0)))
                        .unwrap_or(false);
                    let _ = manager.mark_work_complete(cache_hits as u32, pending.len() as u32);
                    let _ = manager.stop();
                    if !completed {
                        errors.push("Gepard worker exited unsuccessfully or timed out".into());
                    }
                }
                Err(error) => errors.push(format!("Gepard worker process failed: {error}")),
            }

            match std::fs::read(&response_file)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Vec<GepardWorkerResponse>>(&bytes).ok())
            {
                Some(responses) => {
                    errors.extend(validate_worker_responses(&pending_requests, &responses));
                    for response in responses {
                        let Some(&index) = ids.get(&response.id) else {
                            continue;
                        };
                        let measured = wav_duration_secs(&response.output).unwrap_or(0.0);
                        let ok = response.success && measured > 0.0;
                        if ok {
                            if let Some((_, identity, _)) = &identities[index] {
                                let cache_wav =
                                    self.cache_dir.join(format!("{}.wav", identity.key));
                                if Path::new(&response.output) != cache_wav
                                    && std::fs::copy(&response.output, &cache_wav).is_err()
                                {
                                    errors.push(format!(
                                        "{} could not be stored in the Gepard cache",
                                        response.id
                                    ));
                                }
                                let metadata =
                                    self.cache_dir.join(format!("{}.json", identity.key));
                                let _ = std::fs::write(
                                    metadata,
                                    serde_json::to_vec_pretty(&serde_json::json!({
                                        "identity": identity,
                                        "sample_rate": response.sample_rate,
                                        "duration": measured,
                                    }))
                                    .unwrap_or_default(),
                                );
                            }
                        }
                        results[index] = Some(TtsResult {
                            audio_path: ok.then_some(response.output),
                            duration: if ok {
                                measured
                            } else {
                                self.estimate_duration(&requests[index].text)
                            },
                            cached: false,
                            ok,
                            provider: if ok {
                                "gepard_batch".into()
                            } else {
                                "gepard_batch-failed".into()
                            },
                        });
                    }
                }
                None => errors.push("Gepard response JSON is missing or malformed".into()),
            }
        }

        let verification_started = std::time::Instant::now();
        let trace_values = read_jsonl_values(&trace_file);
        let model_load_secs = trace_values
            .iter()
            .find(|value| value["phase"] == "tts.load.completed")
            .and_then(|value| value["elapsed_ms"].as_u64())
            .map(|millis| millis as f32 / 1000.0)
            .unwrap_or(0.0);
        let dialogue_generation_secs = trace_values
            .iter()
            .filter(|value| {
                value["phase"] == "tts.line.completed" || value["phase"] == "tts.line.failed"
            })
            .filter_map(|value| value["elapsed_ms"].as_u64())
            .sum::<u64>() as f32
            / 1000.0;
        let gpu_memory = trace_values
            .iter()
            .filter(|value| value["phase"] == "tts.gpu_memory")
            .cloned()
            .collect();

        let finalized: Vec<TtsResult> = results
            .into_iter()
            .zip(requests)
            .map(|(result, request)| result.unwrap_or_else(|| self.failed(request)))
            .collect();
        let response_by_id: HashMap<String, GepardWorkerResponse> = std::fs::read(&response_file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<GepardWorkerResponse>>(&bytes).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|response: GepardWorkerResponse| (response.id.clone(), response))
            .collect();
        let lines = requests
            .iter()
            .zip(&finalized)
            .enumerate()
            .filter_map(|(index, (request, result))| {
                let (voice, cache, output) = identities[index].clone()?;
                let response = response_by_id.get(&request.id);
                Some(GepardManifestLine {
                    id: request.id.clone(),
                    actor_id: request.actor_id.clone(),
                    text: normalize_spoken_text(&request.text),
                    voice,
                    cache,
                    cached: result.cached,
                    output: result
                        .audio_path
                        .clone()
                        .unwrap_or_else(|| output.to_string_lossy().into_owned()),
                    sample_rate: result
                        .audio_path
                        .as_deref()
                        .and_then(wav_sample_rate)
                        .unwrap_or(0),
                    duration: result.duration,
                    elapsed_ms: response.map(|value| value.elapsed_ms).unwrap_or(0),
                    success: result.ok,
                })
            })
            .collect();
        let manifest = GepardBatchManifest {
            schema_version: 1,
            provider: "gepard_batch".into(),
            runtime_root: self.cfg.runtime_root.clone(),
            worker_script: self.cfg.worker_script.clone(),
            model_root: self.cfg.model_root.clone(),
            model_revision: self.cfg.model_revision.clone(),
            codec_revision: self.cfg.codec_revision.clone(),
            runtime_version: self.cfg.runtime_version.clone(),
            device: self.cfg.device.clone(),
            line_count: requests.len(),
            cache_hits,
            cache_misses: pending.len(),
            worker_invocations,
            request_preparation_secs,
            model_load_secs,
            dialogue_generation_secs,
            audio_verification_secs: verification_started.elapsed().as_secs_f32(),
            gpu_memory,
            errors,
            lines,
        };
        let _ = std::fs::write(
            &manifest_file,
            serde_json::to_vec_pretty(&manifest).unwrap_or_default(),
        );
        if !persistent {
            let _ = std::fs::remove_dir_all(&batch_root);
        }
        finalized
    }

    fn provider_name(&self) -> &'static str {
        "gepard_batch"
    }
}

fn normalize_spoken_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn stable_line_id(occurrence: usize, actor_id: &str, text: &str) -> String {
    let payload = format!(
        "{}\0{}",
        actor_id.trim().to_ascii_lowercase(),
        normalize_spoken_text(text)
    );
    let hash = blake3::hash(payload.as_bytes()).to_hex().to_string();
    format!("line_{:04}_{}", occurrence + 1, &hash[..12])
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn read_jsonl_values(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn shape_prosody_text(text: &str, delivery: Option<&DeliverySpec>) -> String {
    let mut shaped = normalize_spoken_text(text);
    let Some(delivery) = delivery else {
        return shaped;
    };
    if matches!(delivery.pace, DeliveryPace::Slow | DeliveryPace::Measured) {
        shaped = shaped.replace(", ", ",  ");
    }
    if matches!(delivery.pace, DeliveryPace::Fast) {
        shaped = shaped.replace(", ", " ");
    }
    shaped
}

impl Tts for HttpTts {
    fn synthesize(&self, text: &str, voice_id: &str) -> TtsResult {
        let key = line_key(text, voice_id);
        let dir = Path::new(&self.cache_dir);
        let _ = std::fs::create_dir_all(dir);
        let ext = if self.cfg.format.eq_ignore_ascii_case("pcm") {
            "pcm"
        } else {
            "wav"
        };
        let out = dir.join(format!("http_{key}.{ext}"));

        if out.exists() {
            if let Some(d) = wav_duration_secs(out.to_string_lossy().as_ref()) {
                return TtsResult {
                    audio_path: Some(out.to_string_lossy().into_owned()),
                    duration: d,
                    cached: true,
                    ok: true,
                    provider: "http".into(),
                };
            }
        }

        let voice = self.voice_for(voice_id);
        let url = format!("{}/audio/speech", self.cfg.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.cfg.model,
            "input": text,
            "voice": voice,
            "response_format": self.cfg.format,
        })
        .to_string();
        let body_file = dir.join(format!("http_{key}.req.json"));
        if std::fs::write(&body_file, &body).is_err() {
            return self.fallback(text);
        }
        let timeout = format!("{:.0}", self.cfg.timeout_secs.max(5.0) as u64);
        let mut cmd = std::process::Command::new("curl");
        cmd.arg("-sS")
            .arg("-X")
            .arg("POST")
            .arg(&url)
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("--max-time")
            .arg(&timeout)
            .arg("--data-binary")
            .arg(format!("@{}", body_file.to_string_lossy()))
            .arg("-o")
            .arg(out.to_string_lossy().as_ref());
        if !self.cfg.api_key.is_empty() {
            cmd.arg("-H")
                .arg(format!("Authorization: Bearer {}", self.cfg.api_key));
        }
        let res = cmd.output();
        let _ = std::fs::remove_file(&body_file);
        match res {
            Ok(o) if o.status.success() && out.exists() => {
                match wav_duration_secs(out.to_string_lossy().as_ref()) {
                    Some(d) => TtsResult {
                        audio_path: Some(out.to_string_lossy().into_owned()),
                        duration: d,
                        cached: false,
                        ok: true,
                        provider: "http".into(),
                    },
                    None => {
                        tracing::warn!("http TTS returned non-WAV or unreadable output");
                        self.fallback(text)
                    }
                }
            }
            Ok(o) => {
                tracing::warn!(
                    "http TTS request failed: status={} stderr={}",
                    o.status,
                    String::from_utf8_lossy(&o.stderr)
                        .chars()
                        .take(200)
                        .collect::<String>()
                );
                self.fallback(text)
            }
            Err(e) => {
                tracing::warn!("http TTS curl invocation failed: {e}");
                self.fallback(text)
            }
        }
    }

    fn provider_name(&self) -> &'static str {
        "http"
    }
}

impl HttpTts {
    fn fallback(&self, text: &str) -> TtsResult {
        TtsResult {
            audio_path: None,
            duration: Tts::estimate_duration(self, text),
            cached: false,
            ok: false,
            provider: "http-failed".into(),
        }
    }
}

struct UnavailableTts {
    provider: &'static str,
}

impl Tts for UnavailableTts {
    fn synthesize(&self, text: &str, _voice_id: &str) -> TtsResult {
        TtsResult {
            audio_path: None,
            duration: self.estimate_duration(text),
            cached: false,
            ok: false,
            provider: self.provider.into(),
        }
    }

    fn provider_name(&self) -> &'static str {
        self.provider
    }
}

/// Build the configured TTS provider. Gepard production failures remain Gepard
/// failures; they never substitute espeak or duration estimation.
pub fn build_tts(cfg: &TtsConfig) -> Box<dyn Tts> {
    build_tts_internal(cfg, None)
}

pub fn build_tts_for_episode(cfg: &TtsConfig, audio_dir: &Path) -> Box<dyn Tts> {
    build_tts_internal(cfg, Some(audio_dir))
}

fn build_tts_internal(cfg: &TtsConfig, audio_dir: Option<&Path>) -> Box<dyn Tts> {
    if cfg.provider == "gepard_batch" || cfg.provider == "gepard" {
        if let Some(gepard) = cfg.gepard.clone() {
            let provider = GepardTts::new(gepard, cfg.cache_dir.clone());
            return Box::new(match audio_dir {
                Some(path) => provider.with_episode_audio_dir(path.to_path_buf()),
                None => provider,
            });
        }
        tracing::error!("gepard_batch selected without [tts.gepard] configuration");
        return Box::new(UnavailableTts {
            provider: "gepard_batch-unavailable",
        });
    }
    if cfg.provider == "espeak" {
        let e = EspeakTts::new(cfg);
        if e.available() {
            return Box::new(e);
        }
        tracing::warn!(
            "espeak TTS provider '{}' not reachable; using estimating stub",
            e.exe
        );
    }
    if cfg.provider == "http" {
        if let Some(http) = cfg.http.clone() {
            if http.base_url.trim().is_empty() {
                tracing::warn!(
                    "http TTS provider configured with empty base_url; using estimating stub"
                );
            } else {
                return Box::new(HttpTts::new(http, cfg.cache_dir.clone()));
            }
        } else {
            tracing::warn!(
                "http TTS provider selected but no [tts.http] config; using estimating stub"
            );
        }
    }
    Box::new(EstimatingTts)
}

/// Parse a PCM WAV file header to recover its duration in seconds.
pub fn wav_duration_secs(path: &str) -> Option<f32> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    // fmt chunk: sample rate at offset 24 (little-endian u32).
    let sample_rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]) as f32;
    // Walk chunks to find "data" size, and read block-align (bytes per frame)
    // from the fmt chunk so duration is correct for any bit depth.
    let mut i = 12;
    let mut data_size: Option<u32> = None;
    let mut block_align = 2u32;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let size = u32::from_le_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]);
        if id == b"data" {
            data_size = Some(size);
            break;
        }
        // fmt data layout (from chunk start): audioformat[0..2], channels[2..4],
        // samplerate[4..8], byterate[8..12], blockalign[12..14], bits[14..16].
        if id == b"fmt " && i + 22 <= bytes.len() {
            block_align = u16::from_le_bytes([bytes[i + 20], bytes[i + 21]]) as u32;
        }
        i += 8 + size as usize;
        if size % 2 == 1 {
            i += 1; // padding byte
        }
    }
    let data_size = data_size? as f32;
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]) as f32;
    let bytes_per_sample = (block_align as f32 / channels).max(1.0);
    let total_samples = data_size / (channels * bytes_per_sample);
    if sample_rate <= 0.0 || total_samples <= 0.0 {
        None
    } else {
        Some(total_samples / sample_rate)
    }
}

pub fn wav_sample_rate(path: &str) -> Option<u32> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 28 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    Some(u32::from_le_bytes([
        bytes[24], bytes[25], bytes[26], bytes[27],
    ]))
}

/// Hash a line for caching keys.
pub fn line_key(text: &str, voice_id: &str) -> String {
    let payload = format!("{}\0{}", normalize_spoken_text(text), voice_id);
    blake3::hash(payload.as_bytes()).to_hex().to_string()
}

/// Convenience: a shareable, lazily-built TTS handle.
pub type SharedTts = Mutex<Box<dyn Tts>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GepardPresetConfig, VoiceStatus};
    use std::collections::HashMap;

    fn test_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "backlot-tts-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_test_wav(path: &Path) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&38u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&22_050u32.to_le_bytes());
        bytes.extend_from_slice(&44_100u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0i16.to_le_bytes());
        std::fs::write(path, bytes).unwrap();
    }

    fn test_gepard_config(reference_wav: &Path) -> GepardTtsConfig {
        let profile = VoiceProfileConfig {
            character_id: "mara".into(),
            voice_id: "mara_gepard".into(),
            reference_wav: reference_wav.to_string_lossy().into_owned(),
            reference_hash: blake3::hash(&std::fs::read(reference_wav).unwrap())
                .to_hex()
                .to_string(),
            language: Some("en".into()),
            accent: Some("American".into()),
            seed: 42_001,
            status: VoiceStatus::Temporary,
        };
        GepardTtsConfig {
            runtime_root: "runtimes/gepard".into(),
            worker_script: "backlot_gepard_worker.py".into(),
            model_root: "F:/Models/InfiniteBacklot/gepard-1.0".into(),
            device: "cuda".into(),
            model_revision: "gepard-test".into(),
            codec_revision: "nanocodec-test".into(),
            runtime_version: "worker-test".into(),
            timeout_secs: 30.0,
            cache_bypass: false,
            default_preset: GepardPresetConfig::default(),
            profiles: HashMap::from([("mara".into(), profile)]),
        }
    }

    fn request(id: &str, text: &str) -> TtsRequest {
        TtsRequest {
            id: id.into(),
            actor_id: "mara".into(),
            text: text.into(),
            voice_id: "mara".into(),
            delivery: None,
        }
    }

    #[test]
    fn stable_line_ids_include_occurrence_and_normalized_content() {
        let a = stable_line_id(0, "mara", "  Hello   elevator. ");
        let b = stable_line_id(0, "mara", "Hello elevator.");
        let c = stable_line_id(1, "mara", "Hello elevator.");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("line_0001_"));
    }

    #[test]
    fn character_voice_resolution_is_stable() {
        let dir = test_dir("voice");
        let reference = dir.join("voice.wav");
        write_test_wav(&reference);
        let gepard = GepardTts::new(test_gepard_config(&reference), dir.display().to_string());
        let first = gepard.resolve_voice("mara").unwrap();
        let second = gepard.resolve_voice("mara").unwrap();
        assert_eq!(first.character_id, "mara");
        assert_eq!(first.voice_id, "mara_gepard");
        assert_eq!(first.reference_hash, second.reference_hash);
        assert_eq!(first.seed_base, 42_001);
    }

    #[test]
    fn cache_identity_changes_for_every_waveform_setting() {
        let dir = test_dir("cache");
        let reference = dir.join("voice.wav");
        write_test_wav(&reference);
        let base = test_gepard_config(&reference);
        let a = GepardTts::new(base.clone(), dir.display().to_string());
        let key_a = a.cache_identity(&request("line_0001_a", "Hello.")).unwrap();

        let mut changed = base;
        changed.default_preset.temperature += 0.1;
        let b = GepardTts::new(changed, dir.display().to_string());
        let key_b = b.cache_identity(&request("line_0001_a", "Hello.")).unwrap();
        assert_ne!(key_a.key, key_b.key);
        assert_eq!(key_a.provider, "gepard_batch");
    }

    #[test]
    fn response_validation_rejects_missing_failed_and_missing_wav() {
        let dir = test_dir("responses");
        let expected = vec![request("line_0001_a", "One"), request("line_0002_b", "Two")];
        let responses = vec![GepardWorkerResponse {
            id: "line_0001_a".into(),
            output: dir.join("missing.wav").display().to_string(),
            sample_rate: 22_050,
            duration: 1.0,
            elapsed_ms: 10,
            success: false,
            error: Some("fixture failure".into()),
        }];
        let errors = validate_worker_responses(&expected, &responses);
        assert!(errors.iter().any(|error| error.contains("failed")));
        assert!(errors.iter().any(|error| error.contains("missing WAV")));
        assert!(errors.iter().any(|error| error.contains("line_0002_b")));
    }

    #[test]
    fn gepard_production_mode_never_falls_back_to_espeak_or_estimating() {
        let cfg = TtsConfig {
            provider: "gepard_batch".into(),
            gepard: None,
            ..Default::default()
        };
        let tts = build_tts(&cfg);
        assert_eq!(tts.provider_name(), "gepard_batch-unavailable");
        let result = tts.synthesize("This must fail.", "mara");
        assert!(!result.ok);
        assert_eq!(result.provider, "gepard_batch-unavailable");
    }
}
