//! Story-state mutation.
//!
//! Persistent changes requested by the director are applied *only* through this
//! validated pathway. The delta is recorded so it can be committed (on approve)
//! or discarded (on reject) without mutating the live world prematurely.

use crate::protocol::PersistentChange;
use crate::world::WorldState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldDelta {
    pub added_canonical_facts: Vec<String>,
    pub added_beliefs: Vec<String>, // "<char>:<value>"
    pub relationship_changes: Vec<String>, // "<a>-><b>:<dim><sign><amt>"
    pub resolved_threads: Vec<String>,
    pub new_threads: Vec<String>,
    pub location_changes: Vec<String>, // "<loc>:<state>"
    pub notes: Vec<String>,
}

/// Apply persistent changes to the world and return a record of what changed.
pub fn apply_persistent_changes(world: &mut WorldState, changes: &[PersistentChange]) -> WorldDelta {
    let mut delta = WorldDelta::default();
    for pc in changes {
        match pc.operation.as_str() {
            "add_fact" => {
                if let Some(ch) = world.characters.get_mut(&pc.target) {
                    if !ch.believed_facts.iter().any(|f| f == &pc.value) {
                        ch.believed_facts.push(pc.value.clone());
                    }
                    delta.added_beliefs.push(format!("{}:{}", pc.target, pc.value));
                } else {
                    world.add_fact(&pc.value);
                    delta.added_canonical_facts.push(pc.value.clone());
                }
            }
            "add_belief" => {
                if let Some(ch) = world.characters.get_mut(&pc.target) {
                    if !ch.believed_facts.iter().any(|f| f == &pc.value) {
                        ch.believed_facts.push(pc.value.clone());
                    }
                    delta.added_beliefs.push(format!("{}:{}", pc.target, pc.value));
                }
            }
            "remove_fact" => {
                world.canonical_facts.retain(|f| f != &pc.value);
            }
            "remove_false_belief" => {
                if let Some(ch) = world.characters.get_mut(&pc.target) {
                    ch.believed_facts.retain(|f| f != &pc.value);
                }
            }
            "change_relationship" => {
                let amt = pc.amount.unwrap_or(0.0);
                let dim = pc.field.clone().unwrap_or_else(|| "trust".into());
                let b = &pc.value; // value holds the other character id
                let entry = world
                    .characters
                    .entry(pc.target.clone())
                    .or_insert_with(|| crate::world::Character {
                        id: pc.target.clone(),
                        display_name: pc.target.clone(),
                        role: String::new(),
                        color_hex: "#888888".into(),
                        personality: vec![],
                        motivations: vec![],
                        fears: vec![],
                        voice_id: pc.target.clone(),
                        emotion: vec!["neutral".into()],
                        current_goal: None,
                        known_facts: vec![],
                        believed_facts: vec![],
                        relationships: Default::default(),
                        allowed_actions: vec![],
                        preferred_speech: None,
                        home_location: None,
                    });
                let rel = entry.relationships.entry(b.clone()).or_insert_with(|| {
                    use std::collections::HashMap;
                    crate::world::Relationship { dimensions: HashMap::new() }
                });
                let v = rel.dimensions.entry(dim.clone()).or_insert(0.0);
                *v = (*v + amt).clamp(-1.0, 1.0);
                delta.relationship_changes.push(format!(
                    "{}->{}:{}{}{:.2}",
                    pc.target, b, dim, if amt >= 0.0 { '+' } else { '-' }, amt.abs()
                ));
            }
            "resolve_thread" => {
                if world.threads.remove(&pc.target).is_some() {
                    delta.resolved_threads.push(pc.target.clone());
                }
            }
            "create_thread" => {
                use crate::world::StoryThread;
                let t = StoryThread {
                    id: pc.target.clone(),
                    summary: pc.value.clone(),
                    characters: vec![],
                    locations: vec![],
                    importance: 0.5,
                    age: 0,
                    last_episode: None,
                    resolutions: vec![],
                    may_ignore: true,
                    protected: false,
                };
                world.threads.insert(t.id.clone(), t);
                delta.new_threads.push(pc.target.clone());
            }
            "change_location_condition" => {
                if let Some(l) = world.locations.get_mut(&pc.target) {
                    l.room_state = pc.value.clone();
                    delta.location_changes.push(format!("{}:{}", pc.target, pc.value));
                }
            }
            other => {
                delta.notes.push(format!("unhandled operation '{other}' (target={})", pc.target));
            }
        }
    }
    delta
}

/// Produce a short repetition report for the model prompt.
pub fn repetition_report(recent: &[String]) -> String {
    if recent.is_empty() {
        return "No recent episodes.".into();
    }
    let mut out = String::from("Recent episodes:\n");
    for (i, s) in recent.iter().rev().take(5).enumerate() {
        out.push_str(&format!("{}. {}\n", i + 1, s));
    }
    out
}
