use crate::avatar::PerformanceState;
use crate::timeline::{action_kind, ActionKind, Schedule};
use backlot_motion::bvh::{parse_bvh, to_processed_clip, MotionSidecar};
use backlot_motion::compiler::{
    classify_transition, summarize_clip, MotionSegment, MotionSource, PoseSummary,
    TimedInteractionEvent, TransitionDecision, TransitionPolicy,
};
use backlot_motion::library::{
    cache_key, read_clip, write_clip, ClipApproval, MotionLibrary, MotionManifest,
    ProcessedMotionClip,
};
use backlot_motion::{process_clip, MotionProcessingConfig, RetargetMap};
use backlot_runtime::kimodo::{KimodoConfig, KimodoRequest};
use backlot_runtime::{ModelRuntimeManager, RuntimeKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledTransition {
    pub actor: String,
    pub at: f32,
    pub decision: TransitionDecision,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProductionMotionPlan {
    pub segments: HashMap<String, Vec<MotionSegment>>,
    pub transitions: Vec<CompiledTransition>,
    pub unresolved: Vec<String>,
    #[serde(skip)]
    pub clips: HashMap<PathBuf, ProcessedMotionClip>,
}

#[derive(Debug, Clone, Default)]
pub struct MissingMotionGeneration {
    pub generated_manifests: Vec<PathBuf>,
    pub generation_secs: f32,
    pub processing_secs: f32,
}

#[derive(Debug, Deserialize)]
struct KimodoWorkerResponse {
    semantic: String,
    bvh: PathBuf,
    motion_sidecar: PathBuf,
    success: bool,
}

impl ProductionMotionPlan {
    pub fn active(&self, actor: &str, time: f32) -> Option<(&MotionSegment, &ProcessedMotionClip)> {
        let segment = self
            .segments
            .get(actor)?
            .iter()
            .find(|segment| time >= segment.start && time < segment.start + segment.duration)?;
        let path = segment.clip.as_ref()?;
        self.clips.get(path).map(|clip| (segment, clip))
    }
}

pub fn compile_motion_plan(
    schedule: &Schedule,
    library_root: &Path,
) -> Result<ProductionMotionPlan, String> {
    let library = MotionLibrary::scan(&library_root).map_err(|error| error.to_string())?;
    let review_pending = std::env::var_os("BACKLOT_MOTION_REVIEW").is_some();
    let mut plan = ProductionMotionPlan::default();
    for character in &schedule.characters {
        let mut segments = Vec::new();
        for action in &character.actions {
            let semantic = action
                .performance
                .as_ref()
                .map(|cue| cue.motion.trim())
                .filter(|motion| !motion.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| semantic_for_action(&action.action).to_string());
            let approved = library.approved(&semantic);
            let manifests = if approved.is_empty() && review_pending {
                library.pending(&semantic)
            } else {
                approved
            };
            let (source, clip_path, start_pose, end_pose) =
                if let Some(manifest) = manifests.first() {
                    let clip = read_clip(&manifest.clip).map_err(|error| error.to_string())?;
                    let start_pose = summarize_clip(&clip, false);
                    let end_pose = summarize_clip(&clip, true);
                    plan.clips.insert(manifest.clip.clone(), clip);
                    (
                        MotionSource::ApprovedLibrary,
                        Some(manifest.clip.clone()),
                        start_pose,
                        end_pose,
                    )
                } else {
                    plan.unresolved
                        .push(format!("{}:{}", character.id, semantic));
                    let pose = neutral_pose();
                    (MotionSource::EpisodeKimodo, None, pose.clone(), pose)
                };
            segments.push(MotionSegment {
                actor: character.id.clone(),
                semantic,
                source,
                clip: clip_path,
                start: action.start,
                duration: action.dur.max(0.05),
                start_pose,
                end_pose,
                interruptible: !matches!(action_kind(&action.action), ActionKind::Interact),
                interaction_events: interaction_events(&action.action, action.target.as_deref()),
            });
        }
        segments.sort_by(|a, b| a.start.total_cmp(&b.start));
        for pair in segments.windows(2) {
            let decision = classify_transition(&pair[0], &pair[1], &TransitionPolicy::default());
            plan.transitions.push(CompiledTransition {
                actor: character.id.clone(),
                at: pair[0].start + pair[0].duration,
                decision,
            });
        }
        plan.segments.insert(character.id.clone(), segments);
    }
    plan.unresolved.sort();
    plan.unresolved.dedup();
    Ok(plan)
}

/// Generate unresolved expressive semantics as cached, pending-review Kimodo
/// clips. The final replay never enters this function: once a reviewed manifest
/// is approved, `compile_motion_plan` resolves it directly from disk.
pub fn generate_unresolved_motion(
    unresolved: &[String],
    project_root: &Path,
    library_root: &Path,
    cache_root: &Path,
    seed: u64,
) -> Result<MissingMotionGeneration, String> {
    let project_root = project_root
        .canonicalize()
        .map_err(|error| format!("resolve project root: {error}"))?;
    let library_root = if library_root.is_absolute() {
        library_root.to_path_buf()
    } else {
        project_root.join(library_root)
    };
    let cache_root = if cache_root.is_absolute() {
        cache_root.to_path_buf()
    } else {
        project_root.join(cache_root)
    };
    let library = MotionLibrary::scan(&library_root).map_err(|error| error.to_string())?;
    let mut semantics = unresolved
        .iter()
        .filter_map(|entry| {
            entry
                .split_once(':')
                .map(|(_, semantic)| semantic.to_string())
        })
        .collect::<Vec<_>>();
    semantics.sort();
    semantics.dedup();
    let mut generated_manifests = Vec::new();
    semantics.retain(|semantic| {
        let pending = library.pending(semantic);
        generated_manifests.extend(pending.iter().filter_map(|manifest| {
            manifest
                .clip
                .parent()
                .map(|directory| directory.join("manifest.json"))
        }));
        pending.is_empty()
    });
    if semantics.is_empty() {
        return Ok(MissingMotionGeneration {
            generated_manifests,
            ..Default::default()
        });
    }
    std::fs::create_dir_all(&cache_root).map_err(|error| error.to_string())?;
    let requests = semantics
        .iter()
        .enumerate()
        .map(|(index, semantic)| {
            let prompt = prompt_for_semantic(semantic);
            let key = cache_key(&[semantic, &prompt, &(seed + index as u64).to_string()]);
            let output_stem = cache_root.join(semantic).join(key).join("motion");
            KimodoRequest {
                semantic: semantic.clone(),
                prompt,
                duration: 3.0,
                output_stem,
                seed: seed + index as u64,
                root_waypoints: vec![],
                constraints: None,
            }
        })
        .collect::<Vec<_>>();
    for request in &requests {
        if let Some(parent) = request.output_stem.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
    }
    let batch = uuid::Uuid::new_v4().simple().to_string();
    let request_file = cache_root.join(format!("batch_{batch}.request.json"));
    let response_file = cache_root.join(format!("batch_{batch}.response.json"));
    std::fs::write(
        &request_file,
        serde_json::to_vec_pretty(&requests).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut worker =
        KimodoConfig::project_default(&project_root, request_file.clone(), response_file.clone());
    worker.python_executable =
        PathBuf::from(r"C:\Projects\gemmy\runtimes\kimodo\.venv\Scripts\python.exe");
    let generation_started = Instant::now();
    let mut runtime = ModelRuntimeManager::default();
    runtime
        .start(
            RuntimeKind::Kimodo,
            worker.process_spec(),
            Some("Kimodo-SOMA-RP-v1.1".into()),
        )
        .map_err(|error| error.to_string())?;
    let completed = runtime
        .wait_for_exit(Duration::from_secs(1800))
        .map_err(|error| error.to_string())?;
    let _ = runtime.mark_work_complete(0, requests.len() as u32);
    let _ = runtime.stop();
    let generation_secs = generation_started.elapsed().as_secs_f32();
    if !completed {
        return Err("Kimodo batch failed or timed out".into());
    }
    let responses: Vec<KimodoWorkerResponse> =
        serde_json::from_slice(&std::fs::read(&response_file).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let processing_started = Instant::now();
    for response in responses.into_iter().filter(|response| response.success) {
        let request = requests
            .iter()
            .find(|request| request.semantic == response.semantic)
            .ok_or_else(|| format!("Kimodo returned unknown semantic {}", response.semantic))?;
        let key = cache_key(&[
            &request.semantic,
            &request.prompt,
            &request.seed.to_string(),
            "Kimodo-SOMA-RP-v1.1",
        ]);
        let directory = library_root.join(&request.semantic).join(key);
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let bvh =
            parse_bvh(&std::fs::read_to_string(&response.bvh).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let sidecar: MotionSidecar = serde_json::from_slice(
            &std::fs::read(&response.motion_sidecar).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let mut clip = to_processed_clip(
            &bvh,
            &sidecar,
            &request.semantic,
            &RetargetMap::soma77_to_kaykit(),
            false,
        )
        .map_err(|error| error.to_string())?;
        let validation = process_clip(&mut clip, &MotionProcessingConfig::default());
        if !validation.valid {
            return Err(format!(
                "Kimodo {} failed contact/root validation: {:?}",
                request.semantic, validation.errors
            ));
        }
        write_clip(&directory.join("clip.motion"), &clip).map_err(|error| error.to_string())?;
        let manifest_path = directory.join("manifest.json");
        let manifest = MotionManifest {
            schema_version: 2,
            semantic: request.semantic.clone(),
            cache_key: directory
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            source_revision: "Kimodo-SOMA-RP-v1.1".into(),
            checkpoint: r"F:\Models\Kimodo\Kimodo-SOMA-RP-v1.1".into(),
            prompt: request.prompt.clone(),
            seed: request.seed,
            approval: ClipApproval::Pending,
            clip: PathBuf::from("clip.motion"),
            preview: None,
        };
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        generated_manifests.push(manifest_path);
    }
    let _ = std::fs::remove_file(request_file);
    let _ = std::fs::remove_file(response_file);
    Ok(MissingMotionGeneration {
        generated_manifests,
        generation_secs,
        processing_secs: processing_started.elapsed().as_secs_f32(),
    })
}

pub fn semantic_for_action(action: &str) -> &'static str {
    match action_kind(action) {
        ActionKind::Move => "walk",
        ActionKind::Speak => "talk",
        ActionKind::React => match action {
            "laugh" => "laugh",
            "sigh" => "sigh",
            _ => "reaction",
        },
        ActionKind::Gesture => "talk_gesture",
        ActionKind::Point => "point",
        ActionKind::Look => "look",
        ActionKind::Listen => "listen",
        ActionKind::Interact => match action {
            "activate" => "panel_press",
            "inspect" => "panel_inspect",
            "open" | "close" => "reach_contact",
            "pick_up" | "take" => "pick_up",
            "give" | "put_down" => "hand_off",
            _ => "interaction",
        },
        ActionKind::Environment | ActionKind::Narrative => "idle",
        ActionKind::Unknown => "unmapped",
    }
}

fn prompt_for_semantic(semantic: &str) -> String {
    match semantic {
        "idle" => "A performer stands in a relaxed natural idle with stable planted feet, subtle breathing, small weight shifts, loose arms, and alert head movement, returning seamlessly to the initial stance.".into(),
        "walk" => "A performer walks forward naturally with confident heel-to-toe steps, relaxed arm swing, stable floor contact, and a clean balanced stop in neutral standing.".into(),
        "hurry" => "A performer moves forward in a quick urgent walk with controlled momentum, natural arm swing, stable foot contacts, and decelerates into balanced neutral standing.".into(),
        "turn" => "A performer anticipates, shifts weight, turns the whole body ninety degrees with natural footwork, settles both feet, and ends in relaxed neutral standing.".into(),
        "talk" | "talk_gesture" => "A performer speaks conversationally with grounded posture, natural asymmetric hand gestures, attentive head motion, small weight shifts, and lowers both arms back to relaxed neutral standing.".into(),
        "listen" => "A performer listens attentively with relaxed lowered arms, subtle breathing, a small weight shift, responsive head and torso movement, and remains in a natural neutral stance.".into(),
        "look" => "A performer notices something off to one side, leads with the eyes and head, turns the upper torso slightly, holds the look, then returns naturally to neutral standing.".into(),
        "point" => "A performer anticipates, raises one arm to point clearly at something ahead, holds the readable point briefly, lowers the arm completely, and returns to relaxed neutral standing.".into(),
        "reaction" => "A performer notices something impossible, recoils with a sharp full-body startle and backward weight shift, holds the reaction briefly, then recovers into wary neutral standing with lowered arms.".into(),
        "laugh" => "A performer gives a brief natural laugh through the chest and shoulders, gestures lightly with one hand, then settles with both arms lowered in neutral standing.".into(),
        "sigh" => "A performer exhales visibly, shoulders and chest drop, posture softens with a small weight shift, then returns to quiet neutral standing with lowered arms.".into(),
        "panel_press" => "A performer approaches a wall control panel, aligns the right shoulder, reaches with the right hand, presses one button, releases, lowers the arm, and returns to relaxed standing.".into(),
        "panel_inspect" => "A performer leans toward a wall control panel, examines the display closely, touches the panel carefully, recoils slightly, and returns to a neutral attentive stance.".into(),
        "pick_up" => "A performer steps to a small object, bends with stable feet, reaches, grasps it, rises, and settles into a balanced standing pose holding it.".into(),
        "hand_off" => "A performer presents a small object with one hand, waits for contact, releases it, withdraws the hand, and returns to a relaxed standing pose.".into(),
        other => format!("A performer executes {other} clearly, with anticipation, action, a brief readable hold, recovery, stable feet, and a neutral final pose."),
    }
}

fn neutral_pose() -> PoseSummary {
    PoseSummary {
        root: [0.0; 3],
        velocity: [0.0; 3],
        joints: BTreeMap::new(),
        contacts: vec![],
    }
}

fn interaction_events(action: &str, target: Option<&str>) -> Vec<TimedInteractionEvent> {
    if !matches!(action_kind(action), ActionKind::Interact) {
        return vec![];
    }
    vec![TimedInteractionEvent {
        normalized_time: 0.55,
        event: format!("{action}.contact"),
        target: target.map(str::to_string),
    }]
}

pub fn native_state_for_semantic(semantic: &str) -> PerformanceState {
    match semantic {
        "walk" | "hurry" => PerformanceState::Walk,
        "talk" => PerformanceState::Talk,
        "listen" => PerformanceState::Listen,
        "look" => PerformanceState::Look,
        "point" => PerformanceState::Point,
        "reaction" | "laugh" | "sigh" => PerformanceState::React,
        "talk_gesture" => PerformanceState::Gesture,
        _ => PerformanceState::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_resolution_is_exhaustive_for_production_actions() {
        for action in [
            "move_to",
            "approach",
            "speak",
            "react",
            "gesture",
            "point_at",
            "look_at",
            "pause",
            "activate",
            "inspect",
            "pick_up",
            "open_elevator",
            "add_fact",
        ] {
            assert_ne!(semantic_for_action(action), "unmapped", "{action}");
        }
    }
}
