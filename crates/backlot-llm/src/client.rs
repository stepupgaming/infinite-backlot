//! OpenAI-compatible chat-completions client.
//!
//! Speaks the `/v1/chat/completions` protocol used by llama.cpp, Ollama, vLLM,
//! LM Studio, and OpenAI itself. The base URL, model name, timeout, sampling
//! and credentials are all configurable. Structured output is requested via the
//! `json_schema` response format, with a `json_object` fallback for servers
//! that do not implement strict schemas.

use backlot_core::config::LlmConfig;
use backlot_core::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LlmMetrics {
    pub requests: u32,
    pub failures: u32,
    pub last_latency_ms: f32,
    pub last_prompt_tokens: u32,
    pub last_completion_tokens: u32,
    pub last_error: Option<String>,
    pub schema_repairs: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
enum ResponseFormat {
    #[serde(rename = "json_schema")]
    JsonSchema { json_schema: JsonSchemaBody },
    #[serde(rename = "json_object")]
    JsonObject,
}

#[derive(Clone, Debug, Serialize)]
struct JsonSchemaBody {
    name: String,
    schema: serde_json::Value,
    strict: bool,
}

#[derive(Clone, Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Clone, Debug, Deserialize)]
struct Choice {
    message: ChatMessageOut,
    finish_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ChatMessageOut {
    content: String,
}

#[derive(Clone, Debug, Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
    metrics: Arc<Mutex<LlmMetrics>>,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs_f32(config.timeout_secs.max(1.0)))
            .build()
            .map_err(|e| CoreError::Llm(format!("client build: {e}")))?;
        Ok(Self {
            config,
            http,
            metrics: Arc::new(Mutex::new(LlmMetrics::default())),
        })
    }

    pub fn metrics(&self) -> LlmMetrics {
        self.metrics.lock().unwrap().clone()
    }

    /// The configured model identifier (recorded in authorship + diagnostics).
    pub fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Shared handle to the live metrics, for cross-thread inspection.
    pub fn metrics_arc(&self) -> Arc<Mutex<LlmMetrics>> {
        self.metrics.clone()
    }

    fn endpoint(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        format!("{base}/chat/completions")
    }

    /// Lightweight reachability probe. Errors indicate the model is unavailable.
    pub async fn health_check(&self) -> Result<bool> {
        let models_url = {
            let base = self.config.base_url.trim_end_matches('/');
            // base is typically `.../v1`; models live alongside completions.
            let trimmed = base.strip_suffix("/v1").unwrap_or(base);
            format!("{trimmed}/v1/models")
        };
        match self.http.get(&models_url).send().await {
            Ok(r) => Ok(r.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    async fn chat(&self, messages: Vec<ChatMessage>, format: Option<ResponseFormat>) -> Result<ChatResponse> {
        let req = ChatRequest {
            model: self.config.model.clone(),
            messages,
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            response_format: format,
            stream: if self.config.stream { Some(true) } else { None },
        };
        let mut guard = self.metrics.lock().unwrap();
        guard.requests += 1;
        drop(guard);

        let start = Instant::now();
        let result = self
            .http
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&req)
            .send()
            .await;
        let resp = match result {
            Ok(r) => r,
            Err(e) => {
                let mut g = self.metrics.lock().unwrap();
                g.failures += 1;
                g.last_error = Some(format!("transport: {e}"));
                return Err(CoreError::Llm(format!("transport: {e}")));
            }
        };
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let mut g = self.metrics.lock().unwrap();
            g.failures += 1;
            g.last_error = Some(format!("http {status}: {body}"));
            return Err(CoreError::Llm(format!("http {status}: {body}")));
        }
        let parsed: ChatResponse = match resp.json().await {
            Ok(p) => p,
            Err(e) => {
                let mut g = self.metrics.lock().unwrap();
                g.failures += 1;
                g.last_error = Some(format!("decode: {e}"));
                return Err(CoreError::Llm(format!("decode: {e}")));
            }
        };
        let elapsed = start.elapsed().as_secs_f32() * 1000.0;
        let mut g = self.metrics.lock().unwrap();
        g.last_latency_ms = elapsed;
        if let Some(u) = &parsed.usage {
            g.last_prompt_tokens = u.prompt_tokens;
            g.last_completion_tokens = u.completion_tokens;
        }
        g.last_error = None;
        Ok(parsed)
    }

    /// Request structured JSON conforming to `schema_json`. Retries with a
    /// looser `json_object` format once if the strict path fails.
    pub async fn chat_structured(
        &self,
        system: &str,
        user: &str,
        schema_name: &str,
        schema_json: &str,
        max_retries: u32,
    ) -> Result<String> {
        let schema: serde_json::Value = serde_json::from_str(schema_json)
            .unwrap_or_else(|_| serde_json::json!({"type": "object"}));

        let messages = |extra: &str| {
            let mut v = vec![ChatMessage {
                role: "system".into(),
                content: system.into(),
            }];
            if !extra.is_empty() {
                v.push(ChatMessage {
                    role: "user".into(),
                    content: format!("{user}\n\n{extra}"),
                });
            } else {
                v.push(ChatMessage {
                    role: "user".into(),
                    content: user.into(),
                });
            }
            v
        };

        // Attempt 1: strict json_schema.
        let strict_fmt = ResponseFormat::JsonSchema {
            json_schema: JsonSchemaBody {
                name: schema_name.into(),
                schema: schema.clone(),
                strict: true,
            },
        };
        match self.chat(messages(""), Some(strict_fmt)).await {
            Ok(r) => {
                if let Some(content) = r.choices.into_iter().next().and_then(|c| Some(c.message.content)) {
                    if content.trim().starts_with('{') {
                        return Ok(content);
                    }
                }
            }
            Err(e) => {
                let mut g = self.metrics.lock().unwrap();
                g.schema_repairs += 1;
                tracing::warn!("structured attempt failed ({e}); retrying as json_object");
            }
        }

        // Attempt 2+: json_object with an explicit instruction.
        for attempt in 0..max_retries.max(1) {
            let instruction = format!(
                "Output ONLY a single valid JSON object conforming to the schema named \
                 `{schema_name}`. Do not include commentary, markdown fences, or multiple objects. \
                 (retry {attempt})"
            );
            match self
                .chat(messages(&instruction), Some(ResponseFormat::JsonObject))
                .await
            {
                Ok(r) => {
                    if let Some(content) = r.choices.into_iter().next().map(|c| c.message.content) {
                        if let Some(extracted) = extract_json(&content) {
                            return Ok(extracted.to_string());
                        }
                    }
                }
                Err(e) => {
                    let mut g = self.metrics.lock().unwrap();
                    g.schema_repairs += 1;
                    tracing::warn!("json_object attempt failed: {e}");
                }
            }
        }
        Err(CoreError::Llm("could not obtain structured output".into()))
    }
}

/// Best-effort extraction of the first balanced JSON object from a string that
/// may contain markdown fences or surrounding prose.
fn extract_json(s: &str) -> Option<&str> {
    let s = s.trim();
    let start = s.find('{')?;
    let mut depth = 0;
    let mut in_str = false;
    let mut esc = false;
    for (i, c) in s[start..].char_indices() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' if in_str => esc = true,
            '"' => in_str = !in_str,
            '{' if !in_str => depth += 1,
            '}' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}
