use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PromptSegment {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RootPathSample {
    pub time: f32,
    pub position: [f32; 3],
    pub heading: [f32; 3],
}

pub type RootWaypoint = RootPathSample;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FullBodyKeyframe {
    pub id: String,
    pub time: f32,
    pub reference_motion: String,
    pub reference_frame: u32,
    pub target_root: [f32; 3],
    pub strict: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JointConstraint {
    pub id: String,
    pub time: f32,
    pub joint: String,
    pub position: Option<[f32; 3]>,
    pub rotation_xyzw: Option<[f32; 4]>,
    pub position_weight: f32,
    pub rotation_weight: f32,
    pub strict: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EndEffectorConstraint {
    pub id: String,
    pub time: f32,
    pub joint: String,
    pub position: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    pub position_weight: f32,
    pub rotation_weight: f32,
    pub strict: bool,
    pub reference_motion: String,
    pub reference_frame: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvironmentConstraint {
    pub id: String,
    pub smart_interaction_id: String,
    pub target_id: String,
    pub approach_region: String,
    pub staging_slot: [f32; 3],
    pub facing: [f32; 3],
    pub clearance_radius: f32,
    pub start: f32,
    pub end: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContactEvent {
    pub id: String,
    pub start: f32,
    pub end: f32,
    pub performer_joint: String,
    pub target_id: String,
    pub state_transition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContinuationPose {
    pub source_motion: String,
    pub source_frame: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MotionAuthoringRequest {
    pub schema_version: u32,
    pub request_id: String,
    pub performer: String,
    pub duration: f32,
    pub prompt_sequence: Vec<PromptSegment>,
    #[serde(default)]
    pub root_waypoints: Vec<RootWaypoint>,
    #[serde(default)]
    pub dense_root_path: Vec<RootPathSample>,
    pub arrival_heading: [f32; 3],
    #[serde(default)]
    pub full_body_keyframes: Vec<FullBodyKeyframe>,
    #[serde(default)]
    pub joint_constraints: Vec<JointConstraint>,
    #[serde(default)]
    pub end_effector_constraints: Vec<EndEffectorConstraint>,
    #[serde(default)]
    pub environment_constraints: Vec<EnvironmentConstraint>,
    #[serde(default)]
    pub contact_events: Vec<ContactEvent>,
    pub candidate_count: u32,
    pub seed: u64,
    pub strictness: f32,
    pub continuation_pose: Option<ContinuationPose>,
    pub output_stem: String,
}

impl MotionAuthoringRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported motion authoring schema {}",
                self.schema_version
            ));
        }
        if self.request_id.trim().is_empty() || self.performer.trim().is_empty() {
            return Err("request_id and performer are required".into());
        }
        if !self.duration.is_finite() || self.duration <= 0.0 {
            return Err("duration must be positive and finite".into());
        }
        if !(1..=4).contains(&self.candidate_count) {
            return Err("candidate_count must be between 1 and 4".into());
        }
        if !(0.0..=1.0).contains(&self.strictness) {
            return Err("strictness must be between zero and one".into());
        }
        if self.prompt_sequence.is_empty() {
            return Err("prompt_sequence must not be empty".into());
        }
        let mut cursor = 0.0;
        for segment in &self.prompt_sequence {
            if segment.text.trim().is_empty() || segment.end <= segment.start {
                return Err("prompt segments require text and positive duration".into());
            }
            if (segment.start - cursor).abs() > 0.002 {
                return Err(format!(
                    "prompt sequence gap or overlap at {:.3}; expected {:.3}",
                    segment.start, cursor
                ));
            }
            cursor = segment.end;
        }
        if (cursor - self.duration).abs() > 0.002 {
            return Err("prompt sequence must cover the full duration".into());
        }
        validate_path("root_waypoints", &self.root_waypoints, self.duration)?;
        validate_path("dense_root_path", &self.dense_root_path, self.duration)?;
        for keyframe in &self.full_body_keyframes {
            validate_time(keyframe.time, self.duration, &keyframe.id)?;
        }
        for joint in &self.joint_constraints {
            validate_time(joint.time, self.duration, &joint.id)?;
            if let Some(rotation) = joint.rotation_xyzw {
                validate_quaternion(rotation, &joint.id)?;
            }
        }
        for constraint in &self.end_effector_constraints {
            validate_time(constraint.time, self.duration, &constraint.id)?;
            validate_quaternion(constraint.rotation_xyzw, &constraint.id)?;
            if constraint.joint.trim().is_empty() || constraint.reference_motion.trim().is_empty() {
                return Err(format!(
                    "{} needs joint and reference_motion",
                    constraint.id
                ));
            }
        }
        for environment in &self.environment_constraints {
            if environment.end <= environment.start
                || environment.start < 0.0
                || environment.end > self.duration
            {
                return Err(format!("invalid environment window {}", environment.id));
            }
            if environment.clearance_radius <= 0.0 {
                return Err(format!("invalid clearance for {}", environment.id));
            }
        }
        for contact in &self.contact_events {
            if contact.end <= contact.start || contact.start < 0.0 || contact.end > self.duration {
                return Err(format!("invalid contact window {}", contact.id));
            }
        }
        if self.output_stem.trim().is_empty() {
            return Err("output_stem is required".into());
        }
        Ok(())
    }

    pub fn candidate_seeds(&self) -> Vec<u64> {
        (0..self.candidate_count)
            .map(|index| self.seed.saturating_add(u64::from(index)))
            .collect()
    }
}

fn validate_path(name: &str, samples: &[RootPathSample], duration: f32) -> Result<(), String> {
    let mut previous = -f32::INFINITY;
    for sample in samples {
        if !sample.time.is_finite() || sample.time < 0.0 || sample.time > duration {
            return Err(format!("{name} contains an invalid time"));
        }
        if sample.time <= previous {
            return Err(format!("{name} times must increase strictly"));
        }
        if sample.position.iter().any(|value| !value.is_finite()) {
            return Err(format!("{name} contains a non-finite position"));
        }
        let heading_len = sample.heading.iter().map(|v| v * v).sum::<f32>().sqrt();
        if heading_len < 0.5 {
            return Err(format!("{name} contains an invalid heading"));
        }
        previous = sample.time;
    }
    Ok(())
}

fn validate_time(time: f32, duration: f32, id: &str) -> Result<(), String> {
    if !time.is_finite() || time < 0.0 || time > duration {
        return Err(format!("constraint {id} time is outside the clip"));
    }
    Ok(())
}

fn validate_quaternion(rotation: [f32; 4], id: &str) -> Result<(), String> {
    let norm = rotation.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !norm.is_finite() || (norm - 1.0).abs() > 0.05 {
        return Err(format!("constraint {id} rotation is not normalized"));
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CandidateMetrics {
    pub root_path_deviation: f32,
    pub hand_target_error: f32,
    pub hand_orientation_error_deg: f32,
    pub foot_slide: f32,
    pub floor_penetration: f32,
    pub body_obstacle_intersections: u32,
    pub duration_error: f32,
    pub arrival_heading_error_deg: f32,
    pub contact_timing_error: f32,
    pub joint_limit_violations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MotionEvaluation {
    pub valid: bool,
    pub score: f32,
    pub rejection_reasons: Vec<String>,
    pub metrics: CandidateMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MotionCandidate {
    pub seed: u64,
    pub motion_path: Option<String>,
    pub evaluation: MotionEvaluation,
}

impl MotionCandidate {
    pub fn scored(seed: u64, metrics: CandidateMetrics) -> Self {
        let mut reasons = Vec::new();
        if metrics.body_obstacle_intersections > 0 {
            reasons.push("body_obstacle_intersection".into());
        }
        if metrics.floor_penetration > 0.03 {
            reasons.push("floor_penetration".into());
        }
        if metrics.joint_limit_violations > 0 {
            reasons.push("joint_limit_violation".into());
        }
        if metrics.root_path_deviation > 0.5 {
            reasons.push("root_corridor_violation".into());
        }
        if metrics.hand_target_error > 0.25 {
            reasons.push("interaction_contact_failure".into());
        }
        let score = metrics.root_path_deviation * 4.0
            + metrics.hand_target_error * 5.0
            + metrics.hand_orientation_error_deg / 30.0
            + metrics.foot_slide * 3.0
            + metrics.floor_penetration * 10.0
            + metrics.body_obstacle_intersections as f32 * 100.0
            + metrics.duration_error * 2.0
            + metrics.arrival_heading_error_deg / 45.0
            + metrics.contact_timing_error * 3.0
            + metrics.joint_limit_violations as f32 * 100.0;
        Self {
            seed,
            motion_path: None,
            evaluation: MotionEvaluation {
                valid: reasons.is_empty(),
                score,
                rejection_reasons: reasons,
                metrics,
            },
        }
    }
}

pub fn select_best_candidate(candidates: &[MotionCandidate]) -> Option<&MotionCandidate> {
    candidates
        .iter()
        .filter(|candidate| candidate.evaluation.valid)
        .min_by(|a, b| {
            a.evaluation
                .score
                .total_cmp(&b.evaluation.score)
                .then_with(|| a.seed.cmp(&b.seed))
        })
}

pub trait MotionBackend {
    fn name(&self) -> &'static str;
    fn author(
        &self,
        request: &MotionAuthoringRequest,
        working_directory: &Path,
    ) -> Result<Vec<MotionCandidate>, String>;
}

#[derive(Debug, Clone)]
pub struct KimodoMotionBackend {
    pub python: PathBuf,
    pub runtime_directory: PathBuf,
    pub script: PathBuf,
    pub checkpoint: PathBuf,
    pub diffusion_steps: u32,
}

#[derive(Debug, Deserialize)]
struct KimodoWorkerResponse {
    success: bool,
    candidate_scores: String,
}

#[derive(Debug, Deserialize)]
struct KimodoCandidateFile {
    seed: u64,
    npz: String,
    evaluation: MotionEvaluation,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum KimodoScores {
    Legacy(Vec<KimodoCandidateFile>),
    Versioned {
        candidates: Vec<KimodoCandidateFile>,
    },
}

impl KimodoScores {
    fn candidates(self) -> Vec<KimodoCandidateFile> {
        match self {
            Self::Legacy(candidates) | Self::Versioned { candidates } => candidates,
        }
    }
}

impl KimodoMotionBackend {
    pub fn command_preview(&self, requests_path: &Path, responses_path: &Path) -> Vec<String> {
        vec![
            self.python.display().to_string(),
            self.script.display().to_string(),
            "--checkpoint".into(),
            self.checkpoint.display().to_string(),
            "--requests".into(),
            requests_path.display().to_string(),
            "--responses".into(),
            responses_path.display().to_string(),
            "--diffusion-steps".into(),
            self.diffusion_steps.to_string(),
        ]
    }
}

impl MotionBackend for KimodoMotionBackend {
    fn name(&self) -> &'static str {
        "kimodo-soma-rp"
    }

    fn author(
        &self,
        request: &MotionAuthoringRequest,
        working_directory: &Path,
    ) -> Result<Vec<MotionCandidate>, String> {
        request.validate()?;
        std::fs::create_dir_all(working_directory).map_err(|error| error.to_string())?;
        let requests_path = working_directory.join("motion_authoring_request.json");
        let responses_path = working_directory.join("kimodo_worker_response.json");
        std::fs::write(
            &requests_path,
            serde_json::to_vec_pretty(request).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let args = self.command_preview(&requests_path, &responses_path);
        let output = Command::new(&args[0])
            .args(&args[1..])
            .current_dir(&self.runtime_directory)
            .env_remove("PYTHONPATH")
            .output()
            .map_err(|error| format!("failed to launch Kimodo backend: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "Kimodo backend failed with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let responses: Vec<KimodoWorkerResponse> = serde_json::from_slice(
            &std::fs::read(&responses_path).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let response = responses
            .first()
            .ok_or_else(|| "Kimodo backend returned no response".to_string())?;
        if !response.success {
            return Err("Kimodo rejected every generated candidate".into());
        }
        let score_file: KimodoScores = serde_json::from_slice(
            &std::fs::read(&response.candidate_scores).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        Ok(score_file
            .candidates()
            .into_iter()
            .map(|candidate| MotionCandidate {
                seed: candidate.seed,
                motion_path: Some(candidate.npz),
                evaluation: candidate.evaluation,
            })
            .collect())
    }
}
