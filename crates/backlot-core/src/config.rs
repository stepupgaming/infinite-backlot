//! Runtime + LLM configuration loaded from `data/config.toml`.
//!
//! The LLM section is intentionally an OpenAI-compatible shape: a base URL,
//! model name, and credential. Any local server that speaks the
//! `/v1/chat/completions` protocol works (llama.cpp, Ollama, vLLM, LM Studio, …).

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub llm: LlmConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub director: DirectorConfig,
    /// Text-to-speech configuration (real local engine or estimating stub).
    #[serde(default)]
    pub tts: TtsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// OpenAI-compatible chat-completions base URL, e.g.
    /// `http://localhost:1234/v1`.
    pub base_url: String,
    /// Model name passed as `model` in the request.
    pub model: String,
    /// Optional bearer token. Many local servers accept an empty string.
    #[serde(default)]
    pub api_key: String,
    /// Per-request timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: f32,
    /// Sampling temperature (0 = deterministic where the server allows).
    #[serde(default = "default_temp")]
    pub temperature: f32,
    /// Hard cap on completion tokens for a plan request.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Number of schema-repair / replacement attempts before fallback.
    #[serde(default = "default_retries")]
    pub max_retries: u32,
    /// When true, use the `stream` field and read the final aggregated choice.
    #[serde(default)]
    pub stream: bool,
    /// Optional organization header.
    #[serde(default)]
    pub organization: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Default episode length used when the director does not specify one.
    #[serde(default = "default_duration")]
    pub target_duration_secs: f32,
    /// Maximum continuous silence/dead-air before the governor intervenes.
    #[serde(default = "default_dead_air")]
    pub max_dead_air_secs: f32,
    /// How many episodes to produce before returning to idle (0 = unlimited).
    #[serde(default)]
    pub episodes_to_run: u32,
    /// Base random seed for authoring.
    #[serde(default)]
    pub base_seed: u64,
    /// Whether the render pass should attempt frame capture.
    #[serde(default)]
    pub capture_frames: bool,
    /// Root output directory.
    #[serde(default = "default_output")]
    pub output_dir: String,
    /// Caption style identifier passed to downstream export.
    #[serde(default = "default_caption_style")]
    pub caption_style: String,
    /// Master video resolution (width, height). Vertical proof uses 1080x1920.
    #[serde(default = "default_resolution")]
    pub resolution: (u32, u32),
    /// Fixed frame rate for deterministic capture (e.g. 30).
    #[serde(default = "default_fps")]
    pub frame_rate: u32,
    /// Path or command name for FFmpeg. Safe argument passing is used.
    #[serde(default = "default_ffmpeg")]
    pub ffmpeg_path: String,
    /// Path to a TrueType font for burned-in captions (ffmpeg drawtext).
    /// Empty => a best-effort system font is located at render time.
    #[serde(default)]
    pub font_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectorConfig {
    /// When no model is reachable (or disabled) the built-in deterministic
    /// director authors episodes instead. This keeps the product runnable
    /// with zero external dependencies.
    #[serde(default = "default_true")]
    pub allow_fallback_director: bool,
    /// Disable the real LLM entirely (always use the fallback).
    #[serde(default)]
    pub force_fallback: bool,
    /// REQUIRE-LLM production mode. When true the run must use the configured
    /// model and MUST fail clearly (never silently fall back) if the model is
    /// unreachable, invalid, or any beat cannot be authored/validated.
    #[serde(default)]
    pub require_llm: bool,
    /// How many times a single LLM piece may be repaired/re-requested before
    /// giving up (only relevant when `require_llm` is true).
    #[serde(default = "default_retries")]
    pub max_repairs: u32,
}

/// Text-to-speech configuration. Supports a real local engine (`espeak`) and
/// the duration-only estimating stub. Voices are mapped per character so two
/// performers get distinguishable, persistent voices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    /// Provider id: `estimating` | `espeak` | `http`.
    #[serde(default = "default_tts_provider")]
    pub provider: String,
    /// Path/command for the espeak-ng binary.
    #[serde(default = "default_espeak")]
    pub executable: String,
    /// Output sample rate for generated audio.
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    /// Directory for cached WAV clips (keyed by content hash).
    #[serde(default = "default_tts_cache")]
    pub cache_dir: String,
    /// Default voice id used when a character has no explicit mapping.
    #[serde(default = "default_voice")]
    pub default_voice: String,
    /// Amplitude passed to espeak-ng (0..200).
    #[serde(default = "default_amp")]
    pub amplitude: i32,
    /// Speed multiplier (espeak words/min scaled).
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// Character id -> espeak voice name. Two characters should map to two
    /// distinct voices for distinguishable dialogue.
    #[serde(default)]
    pub voice_map: HashMap<String, String>,
    /// HTTP TTS provider configuration (used when `provider == "http"`). This
    /// enables any OpenAI-compatible `/v1/audio/speech` local server (Kokoro,
    /// XTTS, AllTalk, Piper-HTTP, etc.) without coupling the app to one engine.
    #[serde(default)]
    pub http: Option<HttpTtsConfig>,
}

/// Configuration for a local OpenAI-compatible HTTP TTS provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpTtsConfig {
    /// Base URL, e.g. `http://localhost:8000/v1`.
    #[serde(default)]
    pub base_url: String,
    /// API key (optional for local servers).
    #[serde(default)]
    pub api_key: String,
    /// TTS model id passed in the request body (e.g. `tts-1`, `kokoro`).
    #[serde(default = "default_http_tts_model")]
    pub model: String,
    /// Output format: `wav` or `pcm`.
    #[serde(default = "default_http_tts_format")]
    pub format: String,
    /// Character id -> voice id used by the provider.
    #[serde(default)]
    pub voice_map: HashMap<String, String>,
    /// Default voice when no per-character mapping exists.
    #[serde(default = "default_http_tts_voice")]
    pub default_voice: String,
    /// Request timeout in seconds.
    #[serde(default = "default_http_tts_timeout")]
    pub timeout_secs: f32,
}

fn default_http_tts_model() -> String {
    "tts-1".into()
}
fn default_http_tts_format() -> String {
    "wav".into()
}
fn default_http_tts_voice() -> String {
    "alloy".into()
}
fn default_http_tts_timeout() -> f32 {
    30.0
}

impl Default for Config {
    fn default() -> Self {
        Self {
            llm: LlmConfig::default(),
            runtime: RuntimeConfig::default(),
            director: DirectorConfig::default(),
            tts: TtsConfig::default(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:1234/v1".into(),
            model: "gemma-4-26b-a3b".into(),
            api_key: String::new(),
            timeout_secs: 120.0,
            temperature: 0.4,
            max_tokens: 2048,
            max_retries: 2,
            stream: false,
            organization: None,
        }
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            target_duration_secs: 75.0,
            max_dead_air_secs: 4.0,
            episodes_to_run: 0,
            base_seed: 0xC0FFEE,
            capture_frames: false,
            output_dir: "output".into(),
            caption_style: "backlot-default".into(),
            resolution: (1080, 1920),
            frame_rate: 30,
            ffmpeg_path: "ffmpeg".into(),
            font_path: String::new(),
        }
    }
}

impl Default for DirectorConfig {
    fn default() -> Self {
        Self {
            allow_fallback_director: true,
            force_fallback: false,
            require_llm: false,
            max_repairs: 2,
        }
    }
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            provider: "estimating".into(),
            executable: "espeak-ng".into(),
            sample_rate: 44100,
            cache_dir: "output/cache/tts".into(),
            default_voice: "en-us".into(),
            amplitude: 100,
            speed: 1.0,
            voice_map: HashMap::new(),
        }
    }
}

impl Config {
    /// Load configuration from a TOML file, layering defaults for missing keys.
    pub fn load(path: &str) -> Result<Config> {
        let text = std::fs::read_to_string(path).map_err(|source| CoreError::Io {
            path: std::path::PathBuf::from(path),
            source,
        })?;
        let cfg: Config = toml::from_str(&text)?;
        Ok(cfg)
    }

    /// Load or fall back to defaults when the file is absent.
    pub fn load_or_default(path: &str) -> Config {
        match Self::load(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("config load failed ({e}); using defaults");
                Config::default()
            }
        }
    }
}

fn default_timeout() -> f32 {
    120.0
}
fn default_temp() -> f32 {
    0.4
}
fn default_max_tokens() -> u32 {
    2048
}
fn default_retries() -> u32 {
    2
}
fn default_duration() -> f32 {
    75.0
}
fn default_dead_air() -> f32 {
    4.0
}
fn default_output() -> String {
    "output".into()
}
fn default_caption_style() -> String {
    "backlot-default".into()
}
fn default_resolution() -> (u32, u32) {
    (1080, 1920)
}
fn default_fps() -> u32 {
    30
}
fn default_ffmpeg() -> String {
    "ffmpeg".into()
}
fn default_tts_provider() -> String {
    "estimating".into()
}
fn default_espeak() -> String {
    "espeak-ng".into()
}
fn default_sample_rate() -> u32 {
    44100
}
fn default_tts_cache() -> String {
    "output/cache/tts".into()
}
fn default_voice() -> String {
    "en-us".into()
}
fn default_amp() -> i32 {
    100
}
fn default_speed() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}
