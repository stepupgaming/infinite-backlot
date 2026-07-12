//! `backlot-llm`: OpenAI-compatible chat-completions client + LLM episode
//! author with deterministic fallback.

pub mod author;
pub mod client;

pub use author::{LlmAuthor, SharedMetrics};
pub use client::{LlmClient, LlmMetrics};
