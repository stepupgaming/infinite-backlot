//! Persistent world model.
//!
//! The world is data-driven: characters, locations, props and story threads are
//! plain serializable structures. The LLM never mutates source code or raw ECS
//! components directly — it requests *operations* (see `protocol`) that are
//! applied through validated transitions onto this model.

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    pub characters: HashMap<String, Character>,
    pub locations: HashMap<String, Location>,
    pub props: HashMap<String, Prop>,
    pub threads: HashMap<String, StoryThread>,
    /// Stable canonical truths. Mutation requires an explicit operation.
    pub canonical_facts: Vec<String>,
    /// How many episodes have been committed.
    pub episode_count: u64,
    /// Monotonic simulated time in seconds since world creation.
    pub simulated_time: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Character {
    pub id: String,
    pub display_name: String,
    pub role: String,
    /// Hex color used for the greybox avatar, e.g. "#ffcc00".
    pub color_hex: String,
    pub personality: Vec<String>,
    pub motivations: Vec<String>,
    pub fears: Vec<String>,
    pub voice_id: String,
    /// Current emotional state labels, e.g. ["strained"].
    pub emotion: Vec<String>,
    pub current_goal: Option<String>,
    /// Facts this character definitely knows.
    pub known_facts: Vec<String>,
    /// Beliefs, which may be incorrect (enables dramatic irony).
    pub believed_facts: Vec<String>,
    /// Multidimensional relationships keyed by other character id.
    pub relationships: HashMap<String, Relationship>,
    /// Action tokens this character is permitted to perform.
    pub allowed_actions: Vec<String>,
    pub preferred_speech: Option<String>,
    pub home_location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    /// Dimension -> value in roughly [-1, 1].
    pub dimensions: HashMap<String, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    /// Semantic navigation targets within the room.
    pub staging_marks: Vec<StagingMark>,
    pub camera_anchors: Vec<CameraAnchor>,
    pub available_interactions: Vec<String>,
    #[serde(default)]
    pub room_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagingMark {
    pub id: String,
    pub position: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraAnchor {
    pub id: String,
    pub position: [f32; 3],
    /// Point the camera roughly looks toward.
    pub look_at: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prop {
    pub id: String,
    pub name: String,
    pub location_id: String,
    pub capabilities: Vec<String>,
    pub owner: Option<String>,
    /// Staging mark id where the prop normally lives.
    pub home_mark: String,
    #[serde(default)]
    pub story_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryThread {
    pub id: String,
    pub summary: String,
    pub characters: Vec<String>,
    pub locations: Vec<String>,
    pub importance: f32,
    pub age: u32,
    pub last_episode: Option<u64>,
    pub resolutions: Vec<String>,
    #[serde(default)]
    pub may_ignore: bool,
    #[serde(default)]
    pub protected: bool,
}

impl WorldState {
    pub fn load_initial(path: &str) -> Result<WorldState> {
        let text = std::fs::read_to_string(path).map_err(|source| CoreError::Io {
            path: std::path::PathBuf::from(path),
            source,
        })?;
        let w: WorldState = serde_json::from_str(&text)?;
        Ok(w)
    }

    pub fn character(&self, id: &str) -> Option<&Character> {
        self.characters.get(id)
    }

    pub fn location(&self, id: &str) -> Option<&Location> {
        self.locations.get(id)
    }

    pub fn prop(&self, id: &str) -> Option<&Prop> {
        self.props.get(id)
    }

    /// Resolve an entity id to a display label, regardless of kind.
    pub fn label_for(&self, id: &str) -> String {
        if let Some(c) = self.characters.get(id) {
            return c.display_name.clone();
        }
        if let Some(p) = self.props.get(id) {
            return p.name.clone();
        }
        if let Some(l) = self.locations.get(id) {
            return l.name.clone();
        }
        id.to_string()
    }

    /// Apply a canonical fact addition, de-duplicating.
    pub fn add_fact(&mut self, fact: &str) {
        if !self.canonical_facts.iter().any(|f| f == fact) {
            self.canonical_facts.push(fact.to_string());
        }
    }
}

impl Default for WorldState {
    fn default() -> Self {
        build_default_world()
    }
}

/// Construct the initial surreal apartment-building setting described in the PRD.
pub fn build_default_world() -> WorldState {
    let mut characters = HashMap::new();
    let relationships = |a: &str, b: &str, trust: f32, aff: f32, susp: f32, fam: f32| {
        let mut dims = HashMap::new();
        dims.insert("trust".into(), trust);
        dims.insert("affection".into(), aff);
        dims.insert("suspicion".into(), susp);
        dims.insert("familiarity".into(), fam);
        (
            a.to_string(),
            b.to_string(),
            Relationship { dimensions: dims },
        )
    };

    let mut mara_rels = HashMap::new();
    let (_, _, r) = relationships("mara", "ellis", 0.3, 0.2, 0.1, 0.8);
    mara_rels.insert("ellis".into(), r);
    let (_, _, r) = relationships("mara", "voss", -0.2, -0.1, 0.6, 0.4);
    mara_rels.insert("voss".into(), r);
    let (_, _, r) = relationships("mara", "nox", 0.1, 0.0, 0.4, 0.6);
    mara_rels.insert("nox".into(), r);

    characters.insert(
        "mara".into(),
        Character {
            id: "mara".into(),
            display_name: "Mara".into(),
            role: "building superintendent".into(),
            color_hex: "#4fa3ff".into(),
            personality: vec!["exhausted".into(), "pragmatic".into(), "secretive".into()],
            motivations: vec![
                "keep the building running".into(),
                "hide the impossible floor".into(),
            ],
            fears: vec!["inspections".into(), "the elevator being traced".into()],
            voice_id: "mara".into(),
            emotion: vec!["strained".into()],
            current_goal: Some("complete the shift without revealing anomalies".into()),
            known_facts: vec!["elevator_reaches_unknown_floor".into()],
            believed_facts: vec!["ellis_is_ordinary".into()],
            relationships: mara_rels,
            allowed_actions: vec![
                "move_to".into(),
                "speak".into(),
                "inspect".into(),
                "conceal_object".into(),
                "flicker_lights".into(),
                "sigh".into(),
                "gesture".into(),
                "open".into(),
                "close".into(),
                "activate".into(),
                "deactivate".into(),
                "look_at".into(),
            ],
            preferred_speech: Some("clipped, tired, deflecting".into()),
            home_location: Some("maintenance_room".into()),
        },
    );

    characters.insert(
        "ellis".into(),
        Character {
            id: "ellis".into(),
            display_name: "Ellis".into(),
            role: "excessively curious tenant".into(),
            color_hex: "#ffd24f".into(),
            personality: vec!["curious".into(), "earnest".into(), "unlucky".into()],
            motivations: vec!["understand the building".into()],
            fears: vec!["missing something important".into()],
            voice_id: "ellis".into(),
            emotion: vec!["eager".into()],
            current_goal: Some("investigate the strange elevator".into()),
            known_facts: vec![],
            believed_facts: vec!["building_is_normal".into()],
            relationships: HashMap::new(),
            allowed_actions: vec![
                "move_to".into(),
                "speak".into(),
                "inspect".into(),
                "knock_on".into(),
                "pick_up".into(),
                "point_at".into(),
                "whisper".into(),
                "react".into(),
                "look_at".into(),
                "open".into(),
                "enter_room".into(),
            ],
            preferred_speech: Some("rapid, full of questions".into()),
            home_location: Some("apartment_3b".into()),
        },
    );

    characters.insert(
        "voss".into(),
        Character {
            id: "voss".into(),
            display_name: "Inspector Voss".into(),
            role: "bureaucratic inspector".into(),
            color_hex: "#ff6b6b".into(),
            personality: vec!["pedantic".into(), "rule-bound".into(), "unshakeable".into()],
            motivations: vec!["cite every violation".into()],
            fears: vec!["being wrong on record".into()],
            voice_id: "voss".into(),
            emotion: vec!["neutral".into()],
            current_goal: Some("find a code violation".into()),
            known_facts: vec![],
            believed_facts: vec!["all_floors_are_documented".into()],
            relationships: HashMap::new(),
            allowed_actions: vec![
                "move_to".into(),
                "speak".into(),
                "inspect".into(),
                "point_at".into(),
                "write_note".into(),
                "react".into(),
                "look_at".into(),
                "open".into(),
                "ring_alarm".into(),
            ],
            preferred_speech: Some("formal, citation-heavy".into()),
            home_location: None,
        },
    );

    characters.insert(
        "nox".into(),
        Character {
            id: "nox".into(),
            display_name: "Nox".into(),
            role: "tenant who may not be human".into(),
            color_hex: "#9b6bff".into(),
            personality: vec!["placid".into(), "uncanny".into(), "helpful".into()],
            motivations: vec!["observe".into()],
            fears: vec![],
            voice_id: "nox".into(),
            emotion: vec!["serene".into()],
            current_goal: Some("remain unnoticed".into()),
            known_facts: vec!["elevator_reaches_unknown_floor".into()],
            believed_facts: vec!["mara_knows".into()],
            relationships: HashMap::new(),
            allowed_actions: vec![
                "move_to".into(),
                "speak".into(),
                "react".into(),
                "look_at".into(),
                "flicker_lights".into(),
                "conceal_object".into(),
                "pause".into(),
                "gesture".into(),
            ],
            preferred_speech: Some("calm, slightly delayed".into()),
            home_location: Some("apartment_4a".into()),
        },
    );

    // --- Locations: one hallway, three apartments, maintenance room, lobby ---
    let mut locations = HashMap::new();

    locations.insert(
        "floor_3_hallway".into(),
        Location {
            id: "floor_3_hallway".into(),
            name: "Third-Floor Hallway".into(),
            description: "Narrow hallway with an elevator, three apartment doors, a \
                          maintenance panel and a light that flickers when it disagrees \
                          with you."
                .into(),
            tags: vec!["hallway".into(), "primary".into()],
            staging_marks: vec![
                StagingMark {
                    id: "hall_center".into(),
                    position: crate::stage::HALL_CENTER,
                },
                StagingMark {
                    id: "elevator_door".into(),
                    position: crate::stage::ELEVATOR_STAND,
                },
                StagingMark {
                    id: "apt_3b_door".into(),
                    position: crate::stage::APARTMENT_3B_STAND,
                },
                StagingMark {
                    id: "apt_4a_door".into(),
                    position: crate::stage::APARTMENT_4A_STAND,
                },
                StagingMark {
                    id: "maintenance_panel".into(),
                    position: crate::stage::PANEL_STAND,
                },
                StagingMark {
                    id: "panel_stand".into(),
                    position: crate::stage::PANEL_STAND,
                },
            ],
            camera_anchors: vec![
                CameraAnchor {
                    id: "hall_wide".into(),
                    position: [0.0, 3.0, 7.0],
                    look_at: [0.0, 1.0, 0.0],
                },
                CameraAnchor {
                    id: "hall_elevator".into(),
                    position: [3.0, 1.8, 4.0],
                    look_at: [3.0, 1.0, -1.0],
                },
                CameraAnchor {
                    id: "hall_panel".into(),
                    position: [1.5, 1.8, 4.0],
                    look_at: [1.5, 1.0, -1.0],
                },
            ],
            available_interactions: vec![
                "use_elevator".into(),
                "open_maintenance_panel".into(),
                "knock_on_doors".into(),
                "control_hallway_lights".into(),
            ],
            room_state: "normal".into(),
        },
    );

    for (id, name, pos) in [
        ("apartment_3b", "Apartment 3B", [-3.0f32, 0.0, -4.0]),
        ("apartment_4a", "Apartment 4A", [-5.0, 0.0, -4.0]),
        ("maintenance_room", "Maintenance Room", [4.0, 0.0, -4.0]),
    ] {
        locations.insert(
            id.into(),
            Location {
                id: id.into(),
                name: name.into(),
                description: format!("{name}, a small but opinionated room."),
                tags: vec!["apartment".into()],
                staging_marks: vec![
                    StagingMark {
                        id: format!("{id}_door"),
                        position: [pos[0], 0.0, -1.0],
                    },
                    StagingMark {
                        id: format!("{id}_center"),
                        position: [pos[0], 0.0, pos[2] + 1.5],
                    },
                ],
                camera_anchors: vec![CameraAnchor {
                    id: format!("{id}_wide"),
                    position: [pos[0], 2.6, pos[2] + 5.0],
                    look_at: [pos[0], 1.0, pos[2]],
                }],
                available_interactions: vec!["enter_room".into(), "inspect".into()],
                room_state: "normal".into(),
            },
        );
    }

    // --- Props ---
    let mut props = HashMap::new();
    let prop = |id: &str, name: &str, loc: &str, mark: &str, caps: Vec<&str>| Prop {
        id: id.into(),
        name: name.into(),
        location_id: loc.into(),
        capabilities: caps.into_iter().map(str::to_string).collect(),
        owner: None,
        home_mark: mark.into(),
        story_state: "idle".into(),
    };
    props.insert(
        "elevator".into(),
        prop(
            "elevator",
            "Elevator",
            "floor_3_hallway",
            "elevator_door",
            vec!["ride", "open", "reach_unknown_floor"],
        ),
    );
    props.insert(
        "elevator_indicator".into(),
        prop(
            "elevator_indicator",
            "Elevator Indicator",
            "floor_3_hallway",
            "elevator_door",
            vec!["display_symbol"],
        ),
    );
    // Interaction surface props (control panel, door, frame, button) so that
    // the LLM can reference concrete targets like elevator_doors, elevator_panel,
    // maintenance_panel, etc. All are backed by real staging marks so they
    // resolve to actual world positions.
    props.insert(
        "elevator_doors".into(),
        prop(
            "elevator_doors",
            "Elevator Doors",
            "floor_3_hallway",
            "elevator_door",
            vec!["open", "close"],
        ),
    );
    props.insert(
        "elevator_panel".into(),
        prop(
            "elevator_panel",
            "Elevator Control Panel",
            "floor_3_hallway",
            "elevator_door",
            vec!["activate", "inspect"],
        ),
    );
    props.insert(
        "elevator_control_panel".into(),
        prop(
            "elevator_control_panel",
            "Elevator Control Panel",
            "floor_3_hallway",
            "panel_stand",
            vec!["activate", "inspect"],
        ),
    );
    props.insert(
        "elevator_frame".into(),
        prop(
            "elevator_frame",
            "Elevator Frame",
            "floor_3_hallway",
            "elevator_door",
            vec!["inspect"],
        ),
    );
    props.insert(
        "control_panel".into(),
        prop(
            "control_panel",
            "Elevator Control Panel (alias)",
            "floor_3_hallway",
            "maintenance_panel",
            vec!["activate", "inspect"],
        ),
    );
    props.insert(
        "maintenance_panel".into(),
        prop(
            "maintenance_panel",
            "Maintenance Panel",
            "floor_3_hallway",
            "maintenance_panel",
            vec!["open", "activate", "inspect"],
        ),
    );
    props.insert(
        "hallway_light".into(),
        prop(
            "hallway_light",
            "Hallway Light",
            "floor_3_hallway",
            "hall_center",
            vec!["flicker"],
        ),
    );
    props.insert(
        "maintenance_override_key".into(),
        prop(
            "maintenance_override_key",
            "Maintenance Override Key",
            "maintenance_room",
            "maintenance_room_center",
            vec!["conceal", "reveal"],
        ),
    );
    props.insert(
        "inspection_clipboard".into(),
        prop(
            "inspection_clipboard",
            "Inspection Clipboard",
            "floor_3_hallway",
            "hall_center",
            vec!["write", "cite"],
        ),
    );
    props.insert(
        "flickering_light".into(),
        prop(
            "flickering_light",
            "Flickering Light",
            "floor_3_hallway",
            "hall_center",
            vec!["flicker"],
        ),
    );
    props.insert(
        "strange_plant".into(),
        prop(
            "strange_plant",
            "Strange Plant",
            "apartment_4a",
            "apartment_4a_center",
            vec!["observer", "uncanny"],
        ),
    );

    // --- Story threads ---
    let mut threads = HashMap::new();
    threads.insert(
        "unknown_floor".into(),
        StoryThread {
            id: "unknown_floor".into(),
            summary: "The elevator intermittently reaches a floor absent from building records."
                .into(),
            characters: vec!["mara".into(), "ellis".into(), "nox".into()],
            locations: vec!["floor_3_hallway".into()],
            importance: 0.9,
            age: 0,
            last_episode: None,
            resolutions: vec!["seal the elevator".into(), "document the floor".into()],
            may_ignore: false,
            protected: false,
        },
    );
    threads.insert(
        "missing_tenant".into(),
        StoryThread {
            id: "missing_tenant".into(),
            summary: "Apartment 4A is occupied by someone who is never seen leaving.".into(),
            characters: vec!["nox".into(), "ellis".into()],
            locations: vec!["apartment_4a".into()],
            importance: 0.6,
            age: 0,
            last_episode: None,
            resolutions: vec!["confront nox".into()],
            may_ignore: true,
            protected: false,
        },
    );
    threads.insert(
        "inspection".into(),
        StoryThread {
            id: "inspection".into(),
            summary:
                "Inspector Voss is conducting a building inspection that could expose anomalies."
                    .into(),
            characters: vec!["voss".into(), "mara".into()],
            locations: vec!["floor_3_hallway".into()],
            importance: 0.7,
            age: 0,
            last_episode: None,
            resolutions: vec!["pass inspection".into(), "delay inspection".into()],
            may_ignore: false,
            protected: false,
        },
    );

    WorldState {
        characters,
        locations,
        props,
        threads,
        canonical_facts: vec![
            "mara_is_superintendent".into(),
            "ellis_lives_in_3b".into(),
            "elevator_reaches_unknown_floor".into(),
        ],
        episode_count: 0,
        simulated_time: 0.0,
    }
}
