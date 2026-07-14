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
            cache_dir: cfg.cache_dir.clone(),
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
        Self { cfg, cache_dir }
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
}

impl GepardTts {
    pub fn new(cfg: GepardTtsConfig, cache_dir: String) -> Self {
        Self {
            cfg,
            cache_dir: PathBuf::from(cache_dir),
        }
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

    fn cache_key(&self, request: &TtsRequest, profile: &VoiceProfileConfig) -> Option<String> {
        let reference_hash = Self::reference_hash(profile)?;
        let payload = serde_json::json!({
            "model_revision": self.cfg.model_revision,
            "codec_revision": self.cfg.codec_revision,
            "reference_hash": reference_hash,
            "character": profile.character_id,
            "text": normalize_spoken_text(&request.text),
            "delivery": request.delivery,
            "seed": profile.seed,
        });
        Some(
            blake3::hash(payload.to_string().as_bytes())
                .to_hex()
                .to_string(),
        )
    }

    fn failed(&self, request: &TtsRequest) -> TtsResult {
        TtsResult {
            audio_path: None,
            duration: self.estimate_duration(&request.text),
            cached: false,
            ok: false,
            provider: "gepard-failed".into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GepardWorkerResponse {
    id: String,
    output: String,
    duration: f32,
    success: bool,
}

impl Tts for GepardTts {
    fn synthesize(&self, text: &str, voice_id: &str) -> TtsResult {
        self.synthesize_batch(&[TtsRequest {
            text: text.into(),
            voice_id: voice_id.into(),
            delivery: None,
        }])
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            self.failed(&TtsRequest {
                text: text.into(),
                voice_id: voice_id.into(),
                delivery: None,
            })
        })
    }

    fn synthesize_batch(&self, requests: &[TtsRequest]) -> Vec<TtsResult> {
        let _ = std::fs::create_dir_all(&self.cache_dir);
        let mut results: Vec<Option<TtsResult>> = vec![None; requests.len()];
        let mut pending: Vec<GepardLineRequest> = Vec::new();
        let mut ids: HashMap<String, usize> = HashMap::new();

        for (index, request) in requests.iter().enumerate() {
            let Some(profile) = self.profile(&request.voice_id) else {
                results[index] = Some(self.failed(request));
                continue;
            };
            let Some(key) = self.cache_key(request, profile) else {
                results[index] = Some(self.failed(request));
                continue;
            };
            let wav = self.cache_dir.join(format!("gepard_{key}.wav"));
            // Gepard/NanoCodec emits 22.05 kHz. Older Backlot builds could
            // corrupt the cache by relabelling those samples as 44.1 kHz while
            // trimming silence. Reject that cache entry so the real worker
            // regenerates it instead of replaying pitch-doubled audio.
            if wav_sample_rate(&wav.to_string_lossy()) == Some(22_050) {
                if let Some(duration) = wav_duration_secs(&wav.to_string_lossy()) {
                    results[index] = Some(TtsResult {
                        audio_path: Some(wav.to_string_lossy().into_owned()),
                        duration,
                        cached: true,
                        ok: true,
                        provider: "gepard".into(),
                    });
                    continue;
                }
            }
            let id = format!("line_{index}_{key}");
            ids.insert(id.clone(), index);
            pending.push(GepardLineRequest {
                id,
                text: shape_prosody_text(&request.text, request.delivery.as_ref()),
                output: wav,
                reference_audio: PathBuf::from(&profile.reference_wav),
                seed: profile.seed,
                preset: gepard_preset(request.delivery.as_ref()),
            });
        }

        if !pending.is_empty() {
            let batch_id = uuid::Uuid::new_v4().simple().to_string();
            let request_file = self
                .cache_dir
                .join(format!("batch_{batch_id}.request.json"));
            let response_file = self
                .cache_dir
                .join(format!("batch_{batch_id}.response.json"));
            let write_ok = std::fs::write(
                &request_file,
                serde_json::to_vec_pretty(&pending).unwrap_or_default(),
            )
            .is_ok();
            if write_ok {
                let worker = GepardConfig {
                    runtime_root: PathBuf::from(&self.cfg.runtime_root),
                    model_root: PathBuf::from(&self.cfg.model_root),
                    request_file: request_file.clone(),
                    response_file: response_file.clone(),
                };
                let mut manager = ModelRuntimeManager::default();
                if manager
                    .start(
                        RuntimeKind::Gepard,
                        worker.process_spec(),
                        Some(self.cfg.model_revision.clone()),
                    )
                    .is_ok()
                {
                    let completed = manager
                        .wait_for_exit(Duration::from_secs_f32(self.cfg.timeout_secs.max(30.0)))
                        .unwrap_or(false);
                    let _ = manager.mark_work_complete(0, pending.len() as u32);
                    let _ = manager.stop();
                    if completed {
                        if let Ok(bytes) = std::fs::read(&response_file) {
                            if let Ok(responses) =
                                serde_json::from_slice::<Vec<GepardWorkerResponse>>(&bytes)
                            {
                                for response in responses {
                                    if let Some(&index) = ids.get(&response.id) {
                                        let measured = wav_duration_secs(&response.output)
                                            .unwrap_or(response.duration);
                                        results[index] = Some(TtsResult {
                                            audio_path: Some(response.output),
                                            duration: measured,
                                            cached: false,
                                            ok: response.success && measured > 0.0,
                                            provider: "gepard".into(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let _ = std::fs::remove_file(request_file);
            let _ = std::fs::remove_file(response_file);
        }

        results
            .into_iter()
            .zip(requests)
            .map(|(result, request)| result.unwrap_or_else(|| self.failed(request)))
            .collect()
    }

    fn provider_name(&self) -> &'static str {
        "gepard"
    }
}

fn normalize_spoken_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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

fn gepard_preset(delivery: Option<&DeliverySpec>) -> GepardPreset {
    let mut preset = GepardPreset::default();
    if let Some(delivery) = delivery {
        preset.temperature = match delivery.emotion {
            DeliveryEmotion::Hushed => 0.25,
            DeliveryEmotion::Warm | DeliveryEmotion::Amused => 0.35,
            DeliveryEmotion::Urgent | DeliveryEmotion::Stunned => 0.40,
            _ => 0.30,
        };
    }
    preset
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

/// Build the configured TTS provider. When `provider == "espeak"` we still
/// verify the binary is reachable; if not, we transparently downgrade to the
/// estimating stub and let the caller decide whether that is acceptable.
pub fn build_tts(cfg: &TtsConfig) -> Box<dyn Tts> {
    if cfg.provider == "gepard" {
        if let Some(gepard) = cfg.gepard.clone() {
            return Box::new(GepardTts::new(gepard, cfg.cache_dir.clone()));
        }
        tracing::error!("gepard TTS selected without [tts.gepard] configuration");
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
