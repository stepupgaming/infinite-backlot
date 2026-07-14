//! OpenAI-compatible chat-completions client.
//!
//! Speaks the `/v1/chat/completions` protocol used by llama.cpp, Ollama, vLLM,
//! LM Studio, and OpenAI itself — via the canonical `async-openai` SDK, which
//! constructs well-formed requests, handles auth/headers/versioning, and maps
//! errors correctly. Structured output is requested with the `json_object`
//! format first and the strict `json_schema` format as a fallback.

use async_openai::{
    config::OpenAIConfig,
    error::OpenAIError,
    types::chat::{
        ChatCompletionRequestMessage, CreateChatCompletionRequestArgs, ResponseFormat,
        ResponseFormatJsonSchema,
    },
    Client,
};
use backlot_core::config::LlmConfig;
use backlot_core::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
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

/// Full captured model response for a single logical (structured) call.
/// Unlike the production path, this preserves `reasoning_content`, the raw
/// wire text, finish reason, usage, and model id — needed for diagnostics.
#[derive(Clone, Debug, Serialize, Default)]
pub struct CapturedResponse {
    pub model: Option<String>,
    pub id: Option<String>,
    pub created: Option<u64>,
    pub finish_reason: Option<String>,
    pub content: String,
    pub reasoning_content: Option<String>,
    pub usage: Option<serde_json::Value>,
    pub raw_text: String,
    pub extracted_json: Option<String>,
}

/// One physical HTTP request to the server (a "wire call"). The authoring path
/// can issue several of these per logical call (json_object attempt + json_schema
/// retries + schema-repair re-requests), so counting these exposes the true
/// server occupancy and call-graph depth.
#[derive(Clone, Debug, Serialize)]
pub struct WireCall {
    pub seq: u64,
    pub purpose: String,
    pub format: String,
    pub start_utc: String,
    pub start_unix_ms: u128,
    pub wall_ms: u128,
    pub model: Option<String>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub finish_reason: Option<String>,
    pub ok: bool,
    pub err: Option<String>,
    pub content_len: usize,
    pub reasoning_len: usize,
}

/// Local message shape used by `chat_structured`; converted to the SDK type
/// right before the call.
#[derive(Clone, Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

/// Local response-format enum kept for the `chat_structured` interface. The
/// SDK `ResponseFormat` is used at the wire boundary.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
enum ResponseFormatLocal {
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

/// Minimal response shape expected by `chat_structured`.
#[derive(Clone, Debug)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[allow(dead_code)]
    usage: Option<Usage>,
}

#[derive(Clone, Debug)]
struct Choice {
    message: ChatMessageOut,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Clone, Debug)]
struct ChatMessageOut {
    content: String,
    /// Present on reasoning models; the structured answer is frequently emitted
    /// here rather than in `content`.
    reasoning_content: Option<String>,
}

#[derive(Clone, Debug)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

pub struct LlmClient {
    config: LlmConfig,
    http: Client<OpenAIConfig>,
    /// Parallel plain-reqwest client used only by the diagnostic capture path so
    /// it can read the raw response body (including `reasoning_content`, which the
    /// async-openai typed response drops). Production stays on `http` (the SDK).
    raw: reqwest::Client,
    metrics: Arc<Mutex<LlmMetrics>>,
    /// Every physical HTTP request, in order, for the diagnostic trace.
    wire_log: Arc<Mutex<Vec<WireCall>>>,
    /// When set, each wire call is appended (and flushed) to this JSONL file.
    trace_path: Arc<Mutex<Option<PathBuf>>>,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Result<Self> {
        let oa_config = OpenAIConfig::new()
            .with_api_base(config.base_url.clone())
            .with_api_key(config.api_key.clone());
        // The SDK has no built-in timeout knob, so we configure the underlying
        // reqwest client with one and hand it to the SDK.
        let timeout = std::time::Duration::from_secs_f32(config.timeout_secs.max(1.0));
        let http_client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| CoreError::Llm(format!("client build: {e}")))?;
        let http = Client::with_config(oa_config).with_http_client(http_client);
        let raw = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| CoreError::Llm(format!("raw client build: {e}")))?;
        Ok(Self {
            config,
            http,
            raw,
            metrics: Arc::new(Mutex::new(LlmMetrics::default())),
            wire_log: Arc::new(Mutex::new(Vec::new())),
            trace_path: Arc::new(Mutex::new(None)),
        })
    }

    pub fn metrics(&self) -> LlmMetrics {
        self.metrics.lock().unwrap().clone()
    }

    /// The configured model identifier (recorded in authorship + diagnostics).
    pub fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Diagnostic accessors for the static server/model configuration.
    pub fn config_base_url(&self) -> &str {
        &self.config.base_url
    }
    pub fn config_model(&self) -> &str {
        &self.config.model
    }
    pub fn config_temperature(&self) -> f32 {
        self.config.temperature
    }
    pub fn config_max_tokens(&self) -> u32 {
        self.config.max_tokens
    }
    pub fn config_timeout(&self) -> f32 {
        self.config.timeout_secs
    }
    pub fn config_llm_max_retries(&self) -> u32 {
        self.config.max_retries
    }
    pub fn config_stream(&self) -> bool {
        self.config.stream
    }

    /// Shared handle to the live metrics, for cross-thread inspection.
    pub fn metrics_arc(&self) -> Arc<Mutex<LlmMetrics>> {
        self.metrics.clone()
    }

    /// Lightweight reachability probe. Errors indicate the model is unavailable.
    pub async fn health_check(&self) -> Result<bool> {
        match self.http.models().list().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        format: Option<ResponseFormatLocal>,
    ) -> Result<ChatResponse> {
        let mut oa_messages = Vec::with_capacity(messages.len());
        for m in &messages {
            let msg = match m.role.as_str() {
                "system" => ChatCompletionRequestMessage::System(
                    async_openai::types::chat::ChatCompletionRequestSystemMessage {
                        content: async_openai::types::chat::ChatCompletionRequestSystemMessageContent::Text(
                            m.content.clone(),
                        ),
                        name: None,
                    },
                ),
                _ => ChatCompletionRequestMessage::User(
                    async_openai::types::chat::ChatCompletionRequestUserMessage {
                        content: async_openai::types::chat::ChatCompletionRequestUserMessageContent::Text(
                            m.content.clone(),
                        ),
                        name: None,
                    },
                ),
            };
            oa_messages.push(msg);
        }

        let mut req_builder = CreateChatCompletionRequestArgs::default();
        req_builder
            .model(self.config.model.clone())
            .messages(oa_messages)
            .temperature(self.config.temperature)
            .max_tokens(self.config.max_tokens);
        if let Some(fmt) = format {
            req_builder.response_format(convert_format(fmt));
        }
        let req = req_builder
            .build()
            .map_err(|e| CoreError::Llm(format!("request build: {e}")))?;

        {
            let mut g = self.metrics.lock().unwrap();
            g.requests += 1;
        }

        let start = Instant::now();
        match self.http.chat().create(req).await {
            Ok(parsed) => {
                let elapsed = start.elapsed().as_secs_f32() * 1000.0;
                let mut g = self.metrics.lock().unwrap();
                g.last_latency_ms = elapsed;
                if let Some(u) = &parsed.usage {
                    g.last_prompt_tokens = u.prompt_tokens;
                    g.last_completion_tokens = u.completion_tokens;
                }
                g.last_error = None;
                let raw = parsed
                    .choices
                    .first()
                    .map(|c| c.message.content.clone().unwrap_or_default())
                    .unwrap_or_default();
                trace_llm("oa_ok", elapsed as u128, raw.len(), true, &raw);
                let choices = parsed
                    .choices
                    .into_iter()
                    .map(|c| Choice {
                        message: ChatMessageOut {
                            content: c.message.content.clone().unwrap_or_default(),
                            reasoning_content: None,
                        },
                        finish_reason: c.finish_reason.map(|_| "done".to_string()),
                    })
                    .collect();
                Ok(ChatResponse {
                    choices,
                    usage: parsed.usage.as_ref().map(|u| Usage {
                        prompt_tokens: u.prompt_tokens,
                        completion_tokens: u.completion_tokens,
                    }),
                })
            }
            Err(e) => {
                let elapsed = start.elapsed().as_millis();
                {
                    let mut g = self.metrics.lock().unwrap();
                    g.failures += 1;
                    let msg = e.to_string();
                    g.last_error = Some(format!("transport: {msg}"));
                    trace_llm_err("oa", elapsed, &msg);
                }
                Err(map_err(e))
            }
        }
    }

    /// Request structured JSON conforming to `schema_json`. Tries the lenient
    /// `json_object` format first, then falls back to the strict `json_schema`.
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

        // Reasoning models emit the structured answer inside `reasoning`, so
        // combine both sources before extracting the JSON.
        let combined = |m: &ChatMessageOut| -> String {
            let mut s = m.content.clone();
            if let Some(r) = &m.reasoning_content {
                s.push('\n');
                s.push_str(r);
            }
            s
        };

        // Attempt 1 mirrors Gemmy's proven local-MTP path: ordinary chat
        // completion plus prompt-level JSON discipline, followed by balanced
        // object extraction. llama.cpp response_format grammars constrain every
        // sampled token and substantially reduce draft-MTP acceptance/throughput.
        let instruction0 = format!(
            "Output ONLY a single valid JSON object conforming to the schema named \
             `{schema_name}`. Do not include commentary, markdown fences, or multiple objects."
        );
        match self.chat(messages(&instruction0), None).await {
            Ok(r) => {
                if let Some(m) = r.choices.into_iter().next().map(|c| c.message) {
                    let text = combined(&m);
                    if let Some(extracted) = extract_json(&text) {
                        return Ok(extracted.to_string());
                    }
                }
            }
            Err(e) => {
                let mut g = self.metrics.lock().unwrap();
                g.schema_repairs += 1;
                tracing::warn!("unconstrained MTP attempt failed ({e}); retrying with grammar");
            }
        }

        // Repair attempts may use a grammar only after the fast Gemmy-style
        // path failed to produce a balanced object.
        let strict_fmt = ResponseFormatLocal::JsonSchema {
            json_schema: JsonSchemaBody {
                name: schema_name.into(),
                schema: schema.clone(),
                strict: true,
            },
        };
        for attempt in 0..max_retries.max(1) {
            let instruction = format!(
                "Output ONLY a single valid JSON object conforming to the schema named \
                 `{schema_name}`. Do not include commentary, markdown fences, or multiple objects. \
                 (retry {attempt})"
            );
            match self
                .chat(messages(&instruction), Some(strict_fmt.clone()))
                .await
            {
                Ok(r) => {
                    if let Some(m) = r.choices.into_iter().next().map(|c| c.message) {
                        let text = combined(&m);
                        if let Some(extracted) = extract_json(&text) {
                            return Ok(extracted.to_string());
                        }
                    }
                }
                Err(e) => {
                    let mut g = self.metrics.lock().unwrap();
                    g.schema_repairs += 1;
                    tracing::warn!("json_schema attempt failed: {e}");
                }
            }
        }
        Err(CoreError::Llm("could not obtain structured output".into()))
    }
}

impl LlmClient {
    /// Enable per-event JSONL tracing of every physical HTTP request to `path`.
    pub fn set_trace_path(&self, path: PathBuf) {
        *self.trace_path.lock().unwrap() = Some(path);
    }

    /// Snapshot of every physical HTTP request recorded so far.
    pub fn wire_log_snapshot(&self) -> Vec<WireCall> {
        self.wire_log.lock().unwrap().clone()
    }

    /// Append a logical (per-structured-call) trace event to the trace file.
    /// Used by the authoring diagnostic to flush, per call: purpose, timing,
    /// validation outcome, measured duration, repair direction, and acceptance —
    /// so an interrupted run still leaves a complete, readable trace.
    pub fn append_trace_event(&self, event: serde_json::Value) {
        let path = self.trace_path.lock().unwrap();
        if let Some(p) = path.as_ref() {
            let mut line = event;
            if let Some(obj) = line.as_object_mut() {
                if !obj.contains_key("kind") {
                    obj.insert("kind".into(), serde_json::json!("logical"));
                }
            }
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
            {
                let _ = std::writeln!(f, "{}", serde_json::to_string(&line).unwrap_or_default());
            }
        }
    }

    /// Capture-aware structured request. Mirrors `chat_structured`'s json_object ->
    /// json_schema fallback sequence, but issues the requests via the raw reqwest
    /// client so the FULL response (including `reasoning_content`, finish reason,
    /// usage, model id) is preserved, and every wire call is logged for the
    /// diagnostic trace. Returns the extracted JSON string and the captured response.
    pub async fn chat_structured_capture(
        &self,
        system: &str,
        user: &str,
        schema_name: &str,
        schema_json: &str,
        max_retries: u32,
        purpose: &str,
    ) -> Result<(String, CapturedResponse)> {
        let schema: serde_json::Value = serde_json::from_str(schema_json)
            .unwrap_or_else(|_| serde_json::json!({"type": "object"}));
        let instruction0 = format!(
            "Output ONLY a single valid JSON object conforming to the schema named \
             `{schema_name}`. Do not include commentary, markdown fences, or multiple objects."
        );
        // Attempt 1: lenient json_object format.
        let (_w, cap, ext) = self
            .raw_post(system, user, &ResponseFormatLocal::JsonObject, purpose)
            .await?;
        if let Some(ex) = ext {
            return Ok((ex, cap));
        }
        // Attempts 2+: strict json_schema fallback (matches chat_structured).
        let strict = ResponseFormatLocal::JsonSchema {
            json_schema: JsonSchemaBody {
                name: schema_name.into(),
                schema: schema.clone(),
                strict: true,
            },
        };
        for attempt in 0..max_retries.max(1) {
            let instruction = format!(
                "Output ONLY a single valid JSON object conforming to the schema named \
                 `{schema_name}`. Do not include commentary, markdown fences, or multiple objects. \
                 (retry {attempt})"
            );
            let user_full = format!("{user}\n\n{instruction}");
            let (_w, cap, ext) = self.raw_post(system, &user_full, &strict, purpose).await?;
            if let Some(ex) = ext {
                return Ok((ex, cap));
            }
        }
        Err(CoreError::Llm("could not obtain structured output".into()))
    }

    async fn raw_post(
        &self,
        system: &str,
        user: &str,
        fmt: &ResponseFormatLocal,
        purpose: &str,
    ) -> Result<(WireCall, CapturedResponse, Option<String>)> {
        let fmt_label = match fmt {
            ResponseFormatLocal::JsonObject => "json_object",
            ResponseFormatLocal::JsonSchema { .. } => "json_schema",
        };
        let rf = match fmt {
            ResponseFormatLocal::JsonObject => json!({"type": "json_object"}),
            ResponseFormatLocal::JsonSchema { json_schema } => json!({
                "type": "json_schema",
                "json_schema": {
                    "name": json_schema.name,
                    "schema": json_schema.schema,
                    "strict": json_schema.strict
                }
            }),
        };
        let body = json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
            "response_format": rf
        });
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let start = Instant::now();
        let resp = match self.raw.post(&url).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                let wc = self.make_wire(
                    purpose,
                    fmt_label,
                    start,
                    false,
                    None,
                    0,
                    0,
                    None,
                    0,
                    0,
                    Some(format!("transport: {e}")),
                );
                self.record_wire(wc.clone());
                return Err(CoreError::Llm(format!("transport: {e}")));
            }
        };
        let status = resp.status();
        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => {
                let wc = self.make_wire(
                    purpose,
                    fmt_label,
                    start,
                    false,
                    None,
                    0,
                    0,
                    None,
                    0,
                    0,
                    Some(format!("read body: {e}")),
                );
                self.record_wire(wc.clone());
                return Err(CoreError::Llm(format!("read body: {e}")));
            }
        };
        let wall = start.elapsed().as_millis();
        let parsed: serde_json::Value = match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => v,
            Err(e) => {
                let wc = self.make_wire(
                    purpose,
                    fmt_label,
                    start,
                    false,
                    None,
                    0,
                    0,
                    None,
                    0,
                    0,
                    Some(format!("bad json response (status {status}): {e}")),
                );
                self.record_wire(wc.clone());
                return Err(CoreError::Llm(format!(
                    "bad json response (status {status}): {e}; head: {}",
                    text.chars().take(300).collect::<String>()
                )));
            }
        };
        let choices = parsed
            .get("choices")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        let msg = choices
            .first()
            .and_then(|c| c.get("message"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let content = msg
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let reasoning = msg
            .get("reasoning_content")
            .and_then(|v| v.as_str())
            .or_else(|| msg.get("reasoning").and_then(|v| v.as_str()))
            .map(|s| s.to_string());
        let finish = choices
            .first()
            .and_then(|c| c.get("finish_reason"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let usage = parsed.get("usage").cloned();
        let model = parsed
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let id = parsed
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let created = parsed.get("created").and_then(|v| v.as_u64());
        let prompt_tokens = usage
            .as_ref()
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let completion_tokens = usage
            .as_ref()
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let combined = match &reasoning {
            Some(r) => format!("{content}\n{r}"),
            None => content.clone(),
        };
        let extracted = extract_json_owned(&combined);

        let wc = self.make_wire(
            purpose,
            fmt_label,
            start,
            status.is_success(),
            model.clone(),
            prompt_tokens,
            completion_tokens,
            finish.clone(),
            content.len(),
            reasoning.as_ref().map(|r| r.len()).unwrap_or(0),
            None,
        );
        self.record_wire(wc.clone());

        {
            let mut g = self.metrics.lock().unwrap();
            g.requests += 1;
            g.last_latency_ms = wall as f32;
            g.last_prompt_tokens = prompt_tokens;
            g.last_completion_tokens = completion_tokens;
            g.last_error = None;
        }

        let cap = CapturedResponse {
            model,
            id,
            created,
            finish_reason: finish,
            content,
            reasoning_content: reasoning,
            usage,
            raw_text: text,
            extracted_json: extracted.clone(),
        };
        Ok((wc, cap, extracted))
    }

    fn make_wire(
        &self,
        purpose: &str,
        format: &str,
        start: Instant,
        ok: bool,
        model: Option<String>,
        prompt_tokens: u32,
        completion_tokens: u32,
        finish_reason: Option<String>,
        content_len: usize,
        reasoning_len: usize,
        err: Option<String>,
    ) -> WireCall {
        let now = std::time::SystemTime::now();
        let start_unix_ms = now
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
            .saturating_sub(start.elapsed().as_millis());
        WireCall {
            seq: self.wire_log.lock().unwrap().len() as u64,
            purpose: purpose.to_string(),
            format: format.to_string(),
            start_utc: chrono_timestamp(),
            start_unix_ms,
            wall_ms: start.elapsed().as_millis(),
            model,
            prompt_tokens,
            completion_tokens,
            finish_reason,
            ok,
            err,
            content_len,
            reasoning_len,
        }
    }

    fn record_wire(&self, wc: WireCall) {
        {
            let mut log = self.wire_log.lock().unwrap();
            log.push(wc.clone());
        }
        if let Some(p) = self.trace_path.lock().unwrap().clone() {
            let line = serde_json::to_string(&wc).unwrap_or_default();
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)
                .and_then(|mut f| {
                    use std::io::Write;
                    f.write_all(line.as_bytes())?;
                    f.write_all(b"\n")?;
                    f.flush()
                });
        }
    }
}

/// Convert the local response-format enum into the SDK's wire type.
fn convert_format(f: ResponseFormatLocal) -> ResponseFormat {
    match f {
        ResponseFormatLocal::JsonObject => ResponseFormat::JsonObject,
        ResponseFormatLocal::JsonSchema { json_schema } => ResponseFormat::JsonSchema {
            json_schema: ResponseFormatJsonSchema {
                description: None,
                name: json_schema.name,
                schema: json_schema.schema,
                strict: Some(json_schema.strict),
            },
        },
    }
}

/// Map an SDK error into the crate's `CoreError::Llm`, preserving enough
/// context to tell transport failures from API rejections.
fn map_err(e: OpenAIError) -> CoreError {
    match e {
        OpenAIError::Reqwest(re) => CoreError::Llm(format!("transport: {re}")),
        OpenAIError::ApiError(ae) => {
            let inner = &ae.api_error;
            let code = inner
                .code
                .as_ref()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".into());
            CoreError::Llm(format!("http {code}: {}", inner.message))
        }
        other => CoreError::Llm(other.to_string()),
    }
}

fn trace_llm(label: &str, elapsed_ms: u128, text_len: usize, ok: bool, text: &str) {
    if std::env::var("BACKLOT_LLM_TRACE").is_err() {
        return;
    }
    let stamp = chrono_timestamp();
    let head = format!(
        "[{stamp}] {label} {} elapsed={elapsed_ms}ms len={text_len}\n",
        if ok { "OK" } else { "EMPTY" }
    );
    let preview: String = text.chars().take(1200).collect();
    let line = format!("{head}{preview}\n{sep}\n", sep = "-".repeat(60));
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("llm_trace.log")
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

fn trace_llm_err(label: &str, elapsed_ms: u128, err: &str) {
    if std::env::var("BACKLOT_LLM_TRACE").is_err() {
        return;
    }
    let stamp = chrono_timestamp();
    let line = format!(
        "[{stamp}] {label} ERROR elapsed={elapsed_ms}ms err={err}\n{sep}\n",
        sep = "-".repeat(60)
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("llm_trace.log")
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

fn chrono_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{now}")
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

/// Owned variant of `extract_json` for capturing into diagnostics.
fn extract_json_owned(s: &str) -> Option<String> {
    extract_json(s).map(|s| s.to_string())
}
