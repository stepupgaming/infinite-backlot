//! Authoritative coordinates for the one production hallway/elevator set.
//!
//! World blocking, typed camera subjects, interaction anchors, and Bevy set
//! construction all consume these values. Keeping them here prevents the old
//! failure where actors staged at `(3, 0, -1)` while the rendered elevator was
//! actually at `(-5.5, 0, -5)`.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const HALL_CENTER: [f32; 3] = [0.0, 0.0, -0.7];
pub const ELEVATOR_STAND: [f32; 3] = [-5.5, 0.0, -3.75];
// Offset from the call panel so the performer can face and reach it without
// covering the buttons with their torso in the interaction insert.
pub const PANEL_STAND: [f32; 3] = [-3.6, 0.0, -3.8];
pub const APARTMENT_3B_STAND: [f32; 3] = [1.62, 0.0, 5.0];
pub const APARTMENT_4A_STAND: [f32; 3] = [1.62, 0.0, -3.0];

pub const ELEVATOR_CENTER: [f32; 3] = [-5.5, 1.4, -5.0];
pub const ELEVATOR_DOORS: [f32; 3] = [-5.5, 1.4, -4.19];
pub const ELEVATOR_INDICATOR: [f32; 3] = [-5.5, 2.65, -4.16];
// Hall-call panel on the corridor side of the elevator return. It must remain
// reachable from PANEL_STAND and visible without shooting through the cabin.
pub const ELEVATOR_CONTROL_PANEL: [f32; 3] = [-4.1, 1.35, -4.12];
pub const IMPOSSIBLE_FLOOR: [f32; 3] = [-5.5, 1.35, -5.62];

pub fn feature_position(id: &str) -> Option<[f32; 3]> {
    match id {
        "elevator" | "elevator_interior" => Some(ELEVATOR_CENTER),
        "elevator_door" | "elevator_doors" | "elevator_frame" => Some(ELEVATOR_DOORS),
        "elevator_indicator" => Some(ELEVATOR_INDICATOR),
        "maintenance_panel" | "elevator_control_panel" | "elevator_panel" | "control_panel" => {
            Some(ELEVATOR_CONTROL_PANEL)
        }
        "impossible_floor" | "impossible_floor_reveal" => Some(IMPOSSIBLE_FLOOR),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementKind {
    Move,
    Approach,
    Enter,
    Exit,
    Interact,
}

#[derive(Debug, Clone, Copy)]
pub struct StageSlot {
    pub id: &'static str,
    pub position: [f32; 3],
    pub radius: f32,
    pub reveal_excluded: bool,
}

pub const STAGE_SLOTS: &[StageSlot] = &[
    StageSlot {
        id: "apt_3b_entry",
        position: APARTMENT_3B_STAND,
        radius: 0.45,
        reveal_excluded: false,
    },
    StageSlot {
        id: "hallway_entry",
        position: [0.0, 0.0, 7.15],
        radius: 0.45,
        reveal_excluded: false,
    },
    StageSlot {
        id: "service_area",
        position: [-1.55, 0.0, 2.35],
        radius: 0.45,
        reveal_excluded: false,
    },
    StageSlot {
        id: "hallway_two_shot_a",
        position: [-0.85, 0.0, 0.35],
        radius: 0.45,
        reveal_excluded: false,
    },
    StageSlot {
        id: "hallway_two_shot_b",
        position: [0.85, 0.0, 0.35],
        radius: 0.45,
        reveal_excluded: false,
    },
    StageSlot {
        id: "hallway_group_c",
        position: HALL_CENTER,
        radius: 0.45,
        reveal_excluded: false,
    },
    StageSlot {
        id: "elevator_observer",
        position: [-2.45, 0.0, -1.35],
        radius: 0.45,
        reveal_excluded: false,
    },
    StageSlot {
        id: "elevator_right",
        position: [-4.35, 0.0, -2.45],
        radius: 0.45,
        reveal_excluded: false,
    },
    StageSlot {
        id: "elevator_left",
        position: [-6.35, 0.0, -2.45],
        radius: 0.45,
        reveal_excluded: false,
    },
    StageSlot {
        id: "elevator_blocker",
        position: [-4.65, 0.0, -3.35],
        radius: 0.45,
        reveal_excluded: true,
    },
    StageSlot {
        id: "panel_operator",
        position: PANEL_STAND,
        radius: 0.45,
        reveal_excluded: false,
    },
    StageSlot {
        id: "elevator_threshold",
        position: ELEVATOR_STAND,
        radius: 0.45,
        reveal_excluded: true,
    },
    StageSlot {
        id: "side_corridor",
        position: [5.55, 0.0, 0.3],
        radius: 0.45,
        reveal_excluded: false,
    },
];

pub fn stage_slot(id: &str) -> Option<&'static StageSlot> {
    STAGE_SLOTS.iter().find(|slot| slot.id == id)
}

pub fn slot_position(id: &str) -> Option<[f32; 3]> {
    stage_slot(id).map(|slot| slot.position)
}

pub fn horizontal_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StageReservation {
    pub actor: String,
    pub requested_destination: String,
    pub slot: String,
    pub position: [f32; 3],
    pub personal_radius: f32,
    pub preferred_conversational_distance: f32,
    pub fallback_used: bool,
}

#[derive(Debug, Clone, Default)]
pub struct StageOccupancy {
    by_actor: BTreeMap<String, StageReservation>,
}

impl StageOccupancy {
    pub fn for_episode<'a>(actors: impl IntoIterator<Item = &'a str>) -> Self {
        let mut occupancy = Self::default();
        for actor in actors {
            let slot = match actor {
                "voss" => "elevator_left",
                "mara" => "panel_operator",
                "ellis" => "apt_3b_entry",
                _ => "side_corridor",
            };
            if let Some(stage) = stage_slot(slot) {
                occupancy.by_actor.insert(
                    actor.to_string(),
                    StageReservation {
                        actor: actor.to_string(),
                        requested_destination: slot.to_string(),
                        slot: slot.to_string(),
                        position: stage.position,
                        personal_radius: stage.radius,
                        preferred_conversational_distance: 1.15,
                        fallback_used: false,
                    },
                );
            }
        }
        occupancy
    }

    pub fn current(&self, actor: &str) -> Option<&StageReservation> {
        self.by_actor.get(actor)
    }

    pub fn reserve(
        &mut self,
        actor: &str,
        requested: &str,
        kind: MovementKind,
    ) -> Result<StageReservation, String> {
        if let Some(target) = self.by_actor.get(requested).cloned() {
            let offsets = [[-1.15, 0.0], [1.15, 0.0], [0.0, 1.15], [0.0, -1.15]];
            let occupied = self
                .by_actor
                .iter()
                .filter(|(other, _)| other.as_str() != actor)
                .map(|(_, reservation)| reservation)
                .collect::<Vec<_>>();
            for (index, offset) in offsets.into_iter().enumerate() {
                let position = [
                    target.position[0] + offset[0],
                    0.0,
                    target.position[2] + offset[1],
                ];
                if occupied.iter().any(|other| {
                    horizontal_distance(position, other.position) < 0.45 + other.personal_radius
                }) || self.path_blocked(actor, position, &occupied)
                {
                    continue;
                }
                let reservation = StageReservation {
                    actor: actor.to_string(),
                    requested_destination: requested.to_string(),
                    slot: format!("near_{requested}_{}", index + 1),
                    position,
                    personal_radius: 0.45,
                    preferred_conversational_distance: 1.15,
                    fallback_used: true,
                };
                self.by_actor.insert(actor.to_string(), reservation.clone());
                return Ok(reservation);
            }
        }
        let candidates = self.candidates(actor, requested, kind);
        let occupied = self
            .by_actor
            .iter()
            .filter(|(other, _)| other.as_str() != actor)
            .map(|(_, reservation)| reservation)
            .collect::<Vec<_>>();
        for (index, candidate) in candidates.iter().enumerate() {
            let Some(slot) = stage_slot(candidate) else {
                continue;
            };
            let clear = occupied.iter().all(|other| {
                horizontal_distance(slot.position, other.position) + 0.001
                    >= slot.radius + other.personal_radius
            });
            if !clear || self.path_blocked(actor, slot.position, &occupied) {
                continue;
            }
            let reservation = StageReservation {
                actor: actor.to_string(),
                requested_destination: requested.to_string(),
                slot: slot.id.to_string(),
                position: slot.position,
                personal_radius: slot.radius,
                preferred_conversational_distance: 1.15,
                fallback_used: index > 0 || slot.id != requested,
            };
            self.by_actor.insert(actor.to_string(), reservation.clone());
            return Ok(reservation);
        }
        Err(format!(
            "no collision-safe stage slot for {actor} requesting {requested}"
        ))
    }

    fn candidates(&self, actor: &str, requested: &str, kind: MovementKind) -> Vec<&'static str> {
        if self.by_actor.contains_key(requested) {
            let target = self.by_actor.get(requested).unwrap();
            return STAGE_SLOTS
                .iter()
                .filter(|slot| !slot.reveal_excluded)
                .filter(|slot| {
                    (0.8..=1.5).contains(&horizontal_distance(slot.position, target.position))
                })
                .map(|slot| slot.id)
                .collect();
        }
        // Explicit authored slot ids are stronger than broad semantic tokens.
        // `elevator_observer`, for example, must not be remapped through the
        // generic "elevator" preference list back onto the actor's current
        // `elevator_right` slot.
        if let Some(slot) = stage_slot(requested) {
            return vec![slot.id];
        }
        let token = requested.to_ascii_lowercase();
        if token.contains("panel") || matches!(kind, MovementKind::Interact) {
            return vec!["panel_operator", "elevator_right", "elevator_observer"];
        }
        if token.contains("elevator") {
            return match actor {
                "voss" => vec!["elevator_left", "elevator_observer", "hallway_two_shot_a"],
                "ellis" => vec!["elevator_right", "elevator_observer", "hallway_two_shot_b"],
                "mara" => vec!["elevator_blocker", "panel_operator", "elevator_observer"],
                _ => vec!["elevator_observer", "elevator_right", "elevator_left"],
            };
        }
        if token.contains("3b") {
            return vec!["apt_3b_entry", "hallway_entry", "service_area"];
        }
        if token.contains("entry") || matches!(kind, MovementKind::Enter) {
            return vec!["hallway_entry", "service_area", "hallway_two_shot_b"];
        }
        vec![
            "hallway_two_shot_a",
            "hallway_two_shot_b",
            "hallway_group_c",
            "service_area",
        ]
    }

    fn path_blocked(
        &self,
        actor: &str,
        destination: [f32; 3],
        occupied: &[&StageReservation],
    ) -> bool {
        let Some(start) = self.current(actor).map(|reservation| reservation.position) else {
            return false;
        };
        occupied.iter().any(|other| {
            point_segment_distance_xz(other.position, start, destination)
                < other.personal_radius + 0.25
                && horizontal_distance(other.position, destination) > other.personal_radius + 0.45
        })
    }
}

fn point_segment_distance_xz(point: [f32; 3], start: [f32; 3], end: [f32; 3]) -> f32 {
    let ab = [end[0] - start[0], end[2] - start[2]];
    let ap = [point[0] - start[0], point[2] - start[2]];
    let denominator = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if denominator <= 1e-6 {
        0.0
    } else {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / denominator).clamp(0.0, 1.0)
    };
    let nearest = [start[0] + ab[0] * t, start[2] + ab[1] * t];
    ((point[0] - nearest[0]).powi(2) + (point[2] - nearest[1]).powi(2)).sqrt()
}

#[derive(Debug, Clone)]
pub struct ActorRootSample {
    pub timestamp: f32,
    pub actor: String,
    pub position: [f32; 3],
    pub radius: f32,
}

impl ActorRootSample {
    pub fn new(timestamp: f32, actor: impl Into<String>, position: [f32; 3], radius: f32) -> Self {
        Self {
            timestamp,
            actor: actor.into(),
            position,
            radius,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CharacterOverlapReport {
    pub minimum_actor_distance: f32,
    pub character_overlap_frames: usize,
    pub character_interpenetration_failures: usize,
    pub maximum_overlap_depth: f32,
    pub actor_pairs: Vec<String>,
    pub first_overlap_timestamp: Option<f32>,
    pub last_overlap_timestamp: Option<f32>,
}

pub fn analyze_actor_overlap(
    samples: &[ActorRootSample],
    tolerance: f32,
) -> CharacterOverlapReport {
    let mut by_time = BTreeMap::<u32, Vec<&ActorRootSample>>::new();
    for sample in samples {
        by_time
            .entry((sample.timestamp * 1000.0).round() as u32)
            .or_default()
            .push(sample);
    }
    let mut report = CharacterOverlapReport {
        minimum_actor_distance: f32::INFINITY,
        ..Default::default()
    };
    let mut pairs = BTreeSet::new();
    for samples in by_time.values() {
        let mut frame_overlapped = false;
        for left in 0..samples.len() {
            for right in left + 1..samples.len() {
                let a = samples[left];
                let b = samples[right];
                let distance = horizontal_distance(a.position, b.position);
                report.minimum_actor_distance = report.minimum_actor_distance.min(distance);
                let depth = a.radius + b.radius - distance;
                if depth > tolerance {
                    frame_overlapped = true;
                    report.maximum_overlap_depth = report.maximum_overlap_depth.max(depth);
                    let (first, second) = if a.actor <= b.actor {
                        (&a.actor, &b.actor)
                    } else {
                        (&b.actor, &a.actor)
                    };
                    pairs.insert(format!("{first}:{second}"));
                    report.first_overlap_timestamp.get_or_insert(a.timestamp);
                    report.last_overlap_timestamp = Some(a.timestamp);
                }
            }
        }
        if frame_overlapped {
            report.character_overlap_frames += 1;
        }
    }
    if !report.minimum_actor_distance.is_finite() {
        report.minimum_actor_distance = 0.0;
    }
    report.character_interpenetration_failures = report.character_overlap_frames;
    report.actor_pairs = pairs.into_iter().collect();
    report
}

#[cfg(test)]
mod staging_tests {
    use super::*;

    #[test]
    fn semantic_elevator_requests_reserve_distinct_conversational_slots() {
        let mut occupancy = StageOccupancy::for_episode(["voss", "mara", "ellis"]);
        let voss = occupancy
            .reserve("voss", "elevator", MovementKind::Approach)
            .unwrap();
        let mara = occupancy
            .reserve("mara", "elevator", MovementKind::Approach)
            .unwrap();
        let ellis = occupancy
            .reserve("ellis", "elevator", MovementKind::Approach)
            .unwrap();

        assert_ne!(voss.slot, mara.slot);
        assert_ne!(voss.slot, ellis.slot);
        assert_ne!(mara.slot, ellis.slot);
        for pair in [(&voss, &mara), (&voss, &ellis), (&mara, &ellis)] {
            assert!(horizontal_distance(pair.0.position, pair.1.position) >= 0.9);
        }
    }

    #[test]
    fn character_target_resolves_to_offset_instead_of_the_other_root() {
        let mut occupancy = StageOccupancy::for_episode(["voss", "mara", "ellis"]);
        let ellis = occupancy.current("ellis").unwrap().clone();
        let mara = occupancy
            .reserve("mara", "ellis", MovementKind::Approach)
            .unwrap();

        assert_ne!(mara.position, ellis.position);
        let distance = horizontal_distance(mara.position, ellis.position);
        assert!((0.8..=1.5).contains(&distance), "distance={distance}");
    }

    #[test]
    fn overlap_sampling_reports_colliding_actor_roots() {
        let frames = vec![
            ActorRootSample::new(0.0, "mara", [0.0, 0.0, 0.0], 0.45),
            ActorRootSample::new(0.0, "ellis", [0.2, 0.0, 0.0], 0.45),
            ActorRootSample::new(1.0, "mara", [0.0, 0.0, 0.0], 0.45),
            ActorRootSample::new(1.0, "ellis", [1.2, 0.0, 0.0], 0.45),
        ];
        let report = analyze_actor_overlap(&frames, 0.05);
        assert_eq!(report.character_overlap_frames, 1);
        assert!(report.maximum_overlap_depth > 0.6);
        assert_eq!(report.actor_pairs, vec!["ellis:mara"]);
        assert_eq!(report.first_overlap_timestamp, Some(0.0));
        assert_eq!(report.last_overlap_timestamp, Some(0.0));
    }
}
