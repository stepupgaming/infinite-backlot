//! Text-to-speech abstraction.
//!
//! The product needs spoken dialogue. TTS is plug-able: a real local engine
//! (here `espeak-ng`, invoked as a process) can be dropped in behind this
//! trait, or the duration-only `EstimatingTts` stub can be used when no audio
//! engine is configured. Real synthesis produces a WAV file and *measures* its
//! duration, which is what drives accurate dialogue timing (the estimating
//! stub only predicts).

use crate::config::TtsConfig;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

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

pub trait Tts: Send + Sync {
    /// Estimate how long `text` will take to speak (seconds).
    fn estimate_duration(&self, text: &str) -> f32 {
        let words = text.split_whitespace().count().max(1) as f32;
        (words * 0.34 + 0.35).clamp(0.6, 14.0)
    }

    /// Synthesize `text` for `voice_id`. Implementations should cache by hash.
    fn synthesize(&self, text: &str, voice_id: &str) -> TtsResult;

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

/// Build the configured TTS provider. When `provider == "espeak"` we still
/// verify the binary is reachable; if not, we transparently downgrade to the
/// estimating stub and let the caller decide whether that is acceptable.
pub fn build_tts(cfg: &TtsConfig) -> Box<dyn Tts> {
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

/// Hash a line for caching keys.
pub fn line_key(text: &str, voice_id: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    voice_id.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Convenience: a shareable, lazily-built TTS handle.
pub type SharedTts = Mutex<Box<dyn Tts>>;
