//! JSON schemas for structured LLM output (OpenAI `response_format`).

use crate::protocol::{
    AuthoredEpisode, BeatCommand, EpisodePlan, KNOWN_ACTIONS, KNOWN_CAMERA_INTENTS,
    KNOWN_COMPLETION_TYPES,
};
use schemars::schema_for;
use serde_json::Value;

pub fn episode_plan_schema() -> String {
    serde_json::to_string_pretty(&schema_for!(EpisodePlan)).unwrap_or_else(|_| "{}".into())
}

pub fn beat_command_schema() -> String {
    serde_json::to_string_pretty(&schema_for!(BeatCommand)).unwrap_or_else(|_| "{}".into())
}

/// Schema for the single whole-episode structured call. The bounded vocabularies
/// (action type, camera-intent type, completion-condition type) are exposed as
/// real JSON Schema `enum`s so the model is constrained at the schema level and
/// the Rust validators stay the final authority.
pub fn authored_episode_schema() -> String {
    let mut schema = serde_json::to_value(schema_for!(AuthoredEpisode))
        .unwrap_or_else(|_| Value::Object(Default::default()));
    inject_vocab_enums(&mut schema);
    serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".into())
}

/// Drill into a schema node's `properties` object.
fn props_of(node: &mut Value) -> Option<&mut serde_json::Map<String, Value>> {
    node.as_object_mut()?.get_mut("properties")?.as_object_mut()
}

/// Add bounded-vocabulary `enum` constraints to the relevant string properties
/// of the `AuthoredEpisode` schema.
fn inject_vocab_enums(schema: &mut Value) {
    if let Some(root_props) = props_of(schema) {
        if let Some(beats) = root_props.get_mut("beats") {
            // beats is an array; the per-beat schema lives under `items`.
            if let Some(beat_props) = beats.get_mut("items").and_then(props_of) {
                // actions: array -> items -> action (string)
                if let Some(actions) = beat_props.get_mut("actions") {
                    if let Some(action_props) = actions.get_mut("items").and_then(props_of) {
                        if let Some(action) = action_props.get_mut("action") {
                            if let Some(o) = action.as_object_mut() {
                                o.insert("enum".into(), Value::from(KNOWN_ACTIONS));
                            }
                        }
                    }
                }
                // camera_intent.type
                if let Some(cam) = beat_props.get_mut("camera_intent") {
                    if let Some(camp) = props_of(cam) {
                        if let Some(t) = camp.get_mut("type") {
                            if let Some(o) = t.as_object_mut() {
                                o.insert("enum".into(), Value::from(KNOWN_CAMERA_INTENTS));
                            }
                        }
                    }
                }
                // completion_condition.type
                if let Some(cc) = beat_props.get_mut("completion_condition") {
                    if let Some(ccp) = props_of(cc) {
                        if let Some(t) = ccp.get_mut("type") {
                            if let Some(o) = t.as_object_mut() {
                                o.insert("enum".into(), Value::from(KNOWN_COMPLETION_TYPES));
                            }
                        }
                    }
                }
            }
        }
    }
}
