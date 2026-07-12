//! JSON schemas for structured LLM output (OpenAI `response_format`).

use crate::protocol::{BeatCommand, EpisodePlan};
use schemars::schema_for;

pub fn episode_plan_schema() -> String {
    serde_json::to_string_pretty(&schema_for!(EpisodePlan)).unwrap_or_else(|_| "{}".into())
}

pub fn beat_command_schema() -> String {
    serde_json::to_string_pretty(&schema_for!(BeatCommand)).unwrap_or_else(|_| "{}".into())
}
