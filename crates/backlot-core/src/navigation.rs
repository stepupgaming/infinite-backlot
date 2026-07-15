use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::fs;
use std::path::Path;
use thiserror::Error;

pub type Point3 = [f32; 3];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorDefaults {
    pub capsule_radius: f32,
    pub capsule_half_height: f32,
    pub floor_sample_step: f32,
    pub path_sample_step: f32,
    pub turn_radius: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavRegion {
    pub id: String,
    pub surface_type: String,
    pub access: String,
    pub height: f32,
    pub max_slope_deg: f32,
    pub actor_clearance: f32,
    pub priority: i32,
    pub polygon: Vec<[f32; 2]>,
    pub connected_portals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavPortal {
    pub id: String,
    pub regions: [String; 2],
    pub position: Point3,
    pub facing: Point3,
    pub width: f32,
    pub clearance: f32,
    pub traversal_type: String,
    pub runtime_open: bool,
    pub control_entity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavCollider {
    pub id: String,
    pub shape: String,
    pub center: Point3,
    pub half_extents: Point3,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FloorSupport {
    pub id: String,
    pub region_id: String,
    pub height: f32,
    pub polygon: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuideNode {
    pub id: String,
    pub region_id: String,
    pub position: Point3,
    #[serde(default)]
    pub portal_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionVolume {
    pub id: String,
    pub interaction_id: String,
    pub center: Point3,
    pub half_extents: Point3,
    pub required_clearance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationWorld {
    pub schema_version: u32,
    pub world_id: String,
    pub coordinate_system: String,
    pub actor_defaults: ActorDefaults,
    pub regions: Vec<NavRegion>,
    pub portals: Vec<NavPortal>,
    pub colliders: Vec<NavCollider>,
    pub floor_supports: Vec<FloorSupport>,
    pub guide_nodes: Vec<GuideNode>,
    pub guide_edges: Vec<[String; 2]>,
    pub interaction_volumes: Vec<InteractionVolume>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortalState {
    Open,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRequest {
    pub route_id: String,
    pub start: Point3,
    pub destinations: Vec<Point3>,
    pub actor_radius: f32,
    #[serde(default)]
    pub portal_states: BTreeMap<String, PortalState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteStatus {
    Resolved,
    Failed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouteValidation {
    pub static_collision_intersections: u32,
    pub closed_portal_violations: u32,
    pub unsupported_floor_samples: u32,
    pub clearance_failures: u32,
    pub interaction_exclusion_violations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRoute {
    pub route_id: String,
    pub status: RouteStatus,
    pub raw_path: Vec<Point3>,
    pub smoothed_path: Vec<Point3>,
    pub root_waypoints: Vec<Point3>,
    pub dense_root_path: Vec<Point3>,
    pub region_sequence: Vec<String>,
    pub portal_sequence: Vec<String>,
    pub arrival_heading: Point3,
    pub stop_distance: f32,
    pub validation: RouteValidation,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct RouteFailure {
    pub route_id: String,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum NavigationError {
    #[error("could not read navigation contract {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid navigation contract {path}: {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },
}

#[derive(Debug, Clone)]
struct QueueItem {
    cost: f32,
    id: String,
}

impl PartialEq for QueueItem {
    fn eq(&self, other: &Self) -> bool {
        self.cost.to_bits() == other.cost.to_bits() && self.id == other.id
    }
}
impl Eq for QueueItem {}
impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueueItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| other.id.cmp(&self.id))
    }
}

impl NavigationWorld {
    pub fn from_path(path: &Path) -> Result<Self, NavigationError> {
        let text = fs::read_to_string(path).map_err(|source| NavigationError::Read {
            path: path.display().to_string(),
            source,
        })?;
        serde_json::from_str(&text).map_err(|source| NavigationError::Parse {
            path: path.display().to_string(),
            source,
        })
    }

    pub fn validate_path_samples(&self, samples: &[Point3], actor_radius: f32) -> RouteValidation {
        let request = RouteRequest {
            route_id: "runtime_root_validation".into(),
            start: samples.first().copied().unwrap_or([0.0, 0.0, 0.0]),
            destinations: samples.last().copied().into_iter().collect(),
            actor_radius,
            portal_states: BTreeMap::new(),
        };
        self.validate_path(samples, &request)
    }

    pub fn resolve_route(&self, request: &RouteRequest) -> Result<ResolvedRoute, RouteFailure> {
        if request.destinations.is_empty() {
            return Err(self.failure(request, "route has no destination"));
        }
        if request.actor_radius <= 0.0 {
            return Err(self.failure(request, "actor radius must be positive"));
        }

        let mut raw_path = Vec::new();
        let mut smoothed_path = Vec::new();
        let mut region_sequence = Vec::new();
        let mut portal_sequence = Vec::new();
        let mut current = request.start;

        for destination in &request.destinations {
            let start_id = self.nearest_guide(current).ok_or_else(|| {
                self.failure(request, "start has no reachable authored navigation guide")
            })?;
            let goal_id = self.nearest_guide(*destination).ok_or_else(|| {
                self.failure(
                    request,
                    "destination has no reachable authored navigation guide",
                )
            })?;
            let ids = self.astar(&start_id, &goal_id, request).ok_or_else(|| {
                let closed = request
                    .portal_states
                    .iter()
                    .find(|(_, state)| **state == PortalState::Closed)
                    .map(|(id, _)| id.as_str())
                    .unwrap_or("none");
                self.failure(
                    request,
                    &format!("no collision-free guide route; closed portal: {closed}"),
                )
            })?;
            let mut leg = Vec::new();
            if distance(current, self.node(&ids[0]).position) > 0.02 {
                leg.push(current);
            }
            for id in &ids {
                let node = self.node(id);
                leg.push(node.position);
                push_unique(&mut region_sequence, &node.region_id);
                if let Some(portal_id) = &node.portal_id {
                    push_unique(&mut portal_sequence, portal_id);
                }
            }
            if distance(*destination, *leg.last().unwrap()) > 0.02 {
                leg.push(*destination);
            }
            let leg_dense = self.smooth_leg(&leg, request.actor_radius);
            let leg_validation = self.validate_path(&leg_dense, request);
            if leg_validation.static_collision_intersections > 0
                || leg_validation.closed_portal_violations > 0
                || leg_validation.unsupported_floor_samples > 0
                || leg_validation.clearance_failures > 0
            {
                return Err(self.failure(
                    request,
                    &format!("final path safety validation failed: {leg_validation:?}"),
                ));
            }
            append_leg(&mut raw_path, &leg);
            append_leg(&mut smoothed_path, &leg_dense);
            current = *destination;
        }

        let validation = self.validate_path(&smoothed_path, request);
        let arrival_heading = if smoothed_path.len() >= 2 {
            normalized_sub(
                smoothed_path[smoothed_path.len() - 1],
                smoothed_path[smoothed_path.len() - 2],
            )
        } else {
            [0.0, 0.0, 1.0]
        };
        let root_waypoints = simplify_waypoints(&smoothed_path, self.actor_defaults.turn_radius);
        Ok(ResolvedRoute {
            route_id: request.route_id.clone(),
            status: RouteStatus::Resolved,
            raw_path,
            smoothed_path: smoothed_path.clone(),
            root_waypoints,
            dense_root_path: smoothed_path,
            region_sequence,
            portal_sequence,
            arrival_heading,
            stop_distance: 0.42,
            validation,
        })
    }

    fn astar(&self, start: &str, goal: &str, request: &RouteRequest) -> Option<Vec<String>> {
        let mut adjacency: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for [a, b] in &self.guide_edges {
            if self.edge_enabled(a, b, request) {
                adjacency.entry(a.clone()).or_default().push(b.clone());
                adjacency.entry(b.clone()).or_default().push(a.clone());
            }
        }
        for values in adjacency.values_mut() {
            values.sort();
        }
        let mut open = BinaryHeap::new();
        let mut cost = BTreeMap::from([(start.to_string(), 0.0_f32)]);
        let mut parent = BTreeMap::<String, String>::new();
        open.push(QueueItem {
            cost: 0.0,
            id: start.to_string(),
        });
        while let Some(item) = open.pop() {
            if item.id == goal {
                let mut out = vec![item.id.clone()];
                let mut cursor = item.id;
                while let Some(previous) = parent.get(&cursor) {
                    out.push(previous.clone());
                    cursor = previous.clone();
                }
                out.reverse();
                return Some(out);
            }
            let current_cost = *cost.get(&item.id).unwrap_or(&f32::INFINITY);
            for next in adjacency.get(&item.id).into_iter().flatten() {
                let candidate =
                    current_cost + distance(self.node(&item.id).position, self.node(next).position);
                let improve = candidate + 1e-5 < *cost.get(next).unwrap_or(&f32::INFINITY);
                if improve {
                    cost.insert(next.clone(), candidate);
                    parent.insert(next.clone(), item.id.clone());
                    let heuristic = distance(self.node(next).position, self.node(goal).position);
                    open.push(QueueItem {
                        cost: candidate + heuristic,
                        id: next.clone(),
                    });
                }
            }
        }
        None
    }

    fn edge_enabled(&self, a: &str, b: &str, request: &RouteRequest) -> bool {
        let na = self.node(a);
        let nb = self.node(b);
        for portal_id in [na.portal_id.as_ref(), nb.portal_id.as_ref()]
            .into_iter()
            .flatten()
        {
            if self.portal_state(portal_id, request) == PortalState::Closed {
                return false;
            }
        }
        let step = self.actor_defaults.path_sample_step.max(0.05);
        for p in sample_segment(na.position, nb.position, step) {
            if !self.has_floor_support(p) || self.intersects_collider(p, request.actor_radius) {
                return false;
            }
        }
        true
    }

    fn portal_state(&self, id: &str, request: &RouteRequest) -> PortalState {
        request.portal_states.get(id).copied().unwrap_or_else(|| {
            self.portals
                .iter()
                .find(|p| p.id == id)
                .map(|p| {
                    if p.runtime_open {
                        PortalState::Open
                    } else {
                        PortalState::Closed
                    }
                })
                .unwrap_or(PortalState::Closed)
        })
    }

    fn nearest_guide(&self, point: Point3) -> Option<String> {
        self.guide_nodes
            .iter()
            .filter(|node| distance(node.position, point) <= 2.5)
            .min_by(|a, b| {
                distance(a.position, point)
                    .total_cmp(&distance(b.position, point))
                    .then_with(|| a.id.cmp(&b.id))
            })
            .map(|node| node.id.clone())
    }

    fn node(&self, id: &str) -> &GuideNode {
        self.guide_nodes
            .iter()
            .find(|node| node.id == id)
            .unwrap_or_else(|| panic!("unknown authored guide node {id}"))
    }

    fn smooth_leg(&self, raw: &[Point3], radius: f32) -> Vec<Point3> {
        if raw.len() < 3 {
            return resample(raw, self.actor_defaults.path_sample_step);
        }
        let mut curved = vec![raw[0]];
        for pair in raw.windows(2) {
            curved.push(lerp(pair[0], pair[1], 0.25));
            curved.push(lerp(pair[0], pair[1], 0.75));
        }
        curved.push(*raw.last().unwrap());
        let dense = resample(&curved, self.actor_defaults.path_sample_step);
        if dense
            .iter()
            .all(|p| self.has_floor_support(*p) && !self.intersects_collider(*p, radius))
        {
            dense
        } else {
            resample(raw, self.actor_defaults.path_sample_step)
        }
    }

    fn validate_path(&self, path: &[Point3], request: &RouteRequest) -> RouteValidation {
        let mut result = RouteValidation::default();
        for point in path {
            if self.intersects_collider(*point, request.actor_radius) {
                result.static_collision_intersections += 1;
                result.clearance_failures += 1;
            }
            if !self.has_floor_support(*point) {
                result.unsupported_floor_samples += 1;
            }
        }
        for portal_id in request.portal_states.keys() {
            if self.portal_state(portal_id, request) == PortalState::Closed
                && path.iter().any(|point| {
                    self.portals
                        .iter()
                        .find(|portal| portal.id == *portal_id)
                        .is_some_and(|portal| {
                            distance_xz(*point, portal.position) < portal.width * 0.5
                        })
                })
            {
                result.closed_portal_violations += 1;
            }
        }
        result
    }

    fn has_floor_support(&self, point: Point3) -> bool {
        self.floor_supports.iter().any(|support| {
            (point[1] - support.height).abs() <= 0.18
                && point_in_polygon([point[0], point[2]], &support.polygon)
        })
    }

    fn intersects_collider(&self, point: Point3, radius: f32) -> bool {
        self.colliders.iter().any(|collider| {
            let actor_bottom = point[1];
            let actor_top = point[1] + self.actor_defaults.capsule_half_height * 2.0;
            let obstacle_bottom = collider.center[1] - collider.half_extents[1];
            let obstacle_top = collider.center[1] + collider.half_extents[1];
            let vertical = actor_top >= obstacle_bottom && actor_bottom <= obstacle_top;
            vertical
                && (point[0] - collider.center[0]).abs() <= collider.half_extents[0] + radius
                && (point[2] - collider.center[2]).abs() <= collider.half_extents[2] + radius
        })
    }

    fn failure(&self, request: &RouteRequest, message: &str) -> RouteFailure {
        RouteFailure {
            route_id: request.route_id.clone(),
            message: message.to_string(),
        }
    }
}

fn append_leg(target: &mut Vec<Point3>, leg: &[Point3]) {
    for point in leg {
        if target
            .last()
            .is_none_or(|last| distance(*last, *point) > 1e-4)
        {
            target.push(*point);
        }
    }
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if values.last().is_none_or(|last| last != value) {
        values.push(value.to_string());
    }
}

fn point_in_polygon(point: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[j];
        if point_on_segment(point, a, b) {
            return true;
        }
        let crosses = (a[1] > point[1]) != (b[1] > point[1]);
        if crosses {
            let x = (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0];
            if point[0] < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn point_on_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> bool {
    let cross = (p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0]);
    if cross.abs() > 1e-4 {
        return false;
    }
    p[0] >= a[0].min(b[0]) - 1e-4
        && p[0] <= a[0].max(b[0]) + 1e-4
        && p[1] >= a[1].min(b[1]) - 1e-4
        && p[1] <= a[1].max(b[1]) + 1e-4
}

fn resample(path: &[Point3], step: f32) -> Vec<Point3> {
    if path.is_empty() {
        return Vec::new();
    }
    let mut out = vec![path[0]];
    for pair in path.windows(2) {
        let d = distance(pair[0], pair[1]);
        let count = (d / step.max(0.05)).ceil().max(1.0) as usize;
        for i in 1..=count {
            out.push(lerp(pair[0], pair[1], i as f32 / count as f32));
        }
    }
    out
}

fn sample_segment(a: Point3, b: Point3, step: f32) -> Vec<Point3> {
    resample(&[a, b], step)
}

fn simplify_waypoints(path: &[Point3], turn_radius: f32) -> Vec<Point3> {
    if path.len() <= 2 {
        return path.to_vec();
    }
    let mut keep = vec![path[0]];
    let spacing = turn_radius.max(0.35);
    for point in &path[1..path.len() - 1] {
        if distance(*keep.last().unwrap(), *point) >= spacing {
            keep.push(*point);
        }
    }
    if distance(*keep.last().unwrap(), *path.last().unwrap()) > 0.02 {
        keep.push(*path.last().unwrap());
    }
    keep
}

fn lerp(a: Point3, b: Point3, t: f32) -> Point3 {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn distance(a: Point3, b: Point3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn distance_xz(a: Point3, b: Point3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn normalized_sub(a: Point3, b: Point3) -> Point3 {
    let d = distance(a, b).max(1e-6);
    [(a[0] - b[0]) / d, 0.0, (a[2] - b[2]) / d]
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TimeWindow {
    pub start: f32,
    pub end: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reservation {
    pub actor_id: String,
    pub resource_id: String,
    pub window: TimeWindow,
    pub resolution: String,
}

#[derive(Debug, Default)]
pub struct ReservationBook {
    reservations: Vec<Reservation>,
}

impl ReservationBook {
    pub fn reserve(
        &mut self,
        actor_id: &str,
        resource_id: &str,
        mut window: TimeWindow,
        buffer: f32,
    ) -> Result<Reservation, RouteFailure> {
        if window.end <= window.start {
            return Err(RouteFailure {
                route_id: actor_id.to_string(),
                message: "reservation end must follow start".into(),
            });
        }
        let duration = window.end - window.start;
        let mut delayed_after = None;
        let mut conflicts: Vec<_> = self
            .reservations
            .iter()
            .filter(|existing| existing.resource_id == resource_id)
            .collect();
        conflicts.sort_by(|a, b| {
            a.window
                .start
                .total_cmp(&b.window.start)
                .then_with(|| a.actor_id.cmp(&b.actor_id))
        });
        for existing in conflicts {
            if window.start < existing.window.end + buffer
                && window.end + buffer > existing.window.start
            {
                window.start = existing.window.end + buffer;
                window.end = window.start + duration;
                delayed_after = Some(existing.actor_id.clone());
            }
        }
        let reservation = Reservation {
            actor_id: actor_id.to_string(),
            resource_id: resource_id.to_string(),
            window,
            resolution: delayed_after
                .map(|actor| format!("delayed_after_{actor}"))
                .unwrap_or_else(|| "reserved".into()),
        };
        self.reservations.push(reservation.clone());
        Ok(reservation)
    }

    pub fn conflicts(&self) -> usize {
        let mut seen = BTreeSet::new();
        let mut conflicts = 0;
        for a in &self.reservations {
            for b in &self.reservations {
                let key = if a.actor_id < b.actor_id {
                    format!("{}:{}:{}", a.actor_id, b.actor_id, a.resource_id)
                } else {
                    format!("{}:{}:{}", b.actor_id, a.actor_id, a.resource_id)
                };
                if a.actor_id != b.actor_id
                    && a.resource_id == b.resource_id
                    && a.window.start < b.window.end
                    && b.window.start < a.window.end
                    && seen.insert(key)
                {
                    conflicts += 1;
                }
            }
        }
        conflicts
    }
}
