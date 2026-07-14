//! Authoritative coordinates for the one production hallway/elevator set.
//!
//! World blocking, typed camera subjects, interaction anchors, and Bevy set
//! construction all consume these values. Keeping them here prevents the old
//! failure where actors staged at `(3, 0, -1)` while the rendered elevator was
//! actually at `(-5.5, 0, -5)`.

pub const HALL_CENTER: [f32; 3] = [0.0, 0.0, 0.0];
pub const ELEVATOR_STAND: [f32; 3] = [-5.5, 0.0, -3.85];
pub const PANEL_STAND: [f32; 3] = [-4.1, 0.0, -3.85];
pub const APARTMENT_3B_STAND: [f32; 3] = [2.5, 0.0, -4.45];
pub const APARTMENT_4A_STAND: [f32; 3] = [4.5, 0.0, -4.45];

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
        "elevator_control_panel" | "elevator_panel" | "control_panel" => {
            Some(ELEVATOR_CONTROL_PANEL)
        }
        "impossible_floor" | "impossible_floor_reveal" => Some(IMPOSSIBLE_FLOOR),
        _ => None,
    }
}
