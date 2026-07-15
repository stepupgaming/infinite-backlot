use backlot_motion::bvh::{parse_bvh, to_processed_clip, MotionSidecar};
use backlot_motion::library::{cache_key, write_clip, ClipApproval, MotionManifest};
use backlot_motion::{process_clip, MotionProcessingConfig, RetargetMap};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct Request {
    semantic: String,
    prompt: String,
    seed: u64,
    #[serde(default)]
    category: String,
    #[serde(default)]
    root_waypoints: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Response {
    semantic: String,
    bvh: PathBuf,
    motion_sidecar: PathBuf,
    #[serde(default)]
    npz: Option<PathBuf>,
    success: bool,
}

fn arg_value(args: &[String], name: &str, default: &str) -> PathBuf {
    args.iter()
        .position(|value| value == name)
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let request_path = arg_value(&args, "--requests", "output/motion-lab/batch.request.json");
    let response_path = arg_value(
        &args,
        "--responses",
        "output/motion-lab/batch.response.json",
    );
    let library_root = arg_value(&args, "--library", "assets/animations/library");
    let requests: Vec<Request> = serde_json::from_slice(&std::fs::read(&request_path)?)?;
    let responses: Vec<Response> = serde_json::from_slice(&std::fs::read(&response_path)?)?;
    let requests = requests
        .into_iter()
        .map(|request| (request.semantic.clone(), request))
        .collect::<HashMap<_, _>>();
    let mut index = Vec::new();
    for response in responses.into_iter().filter(|response| response.success) {
        let request = requests
            .get(&response.semantic)
            .ok_or_else(|| format!("response references unknown semantic {}", response.semantic))?;
        let key = cache_key(&[
            &request.semantic,
            &request.prompt,
            &request.seed.to_string(),
            "Kimodo-SOMA-RP-v1.1",
        ]);
        let directory = library_root.join(&request.semantic).join(&key);
        std::fs::create_dir_all(&directory)?;
        let bvh = parse_bvh(&std::fs::read_to_string(&response.bvh)?)?;
        let sidecar: MotionSidecar =
            serde_json::from_slice(&std::fs::read(&response.motion_sidecar)?)?;
        let mut clip = to_processed_clip(
            &bvh,
            &sidecar,
            &request.semantic,
            &RetargetMap::soma77_to_kaykit(),
            false,
        )?;
        let validation = process_clip(&mut clip, &MotionProcessingConfig::default());
        let approval = if validation.valid {
            ClipApproval::Pending
        } else {
            ClipApproval::Rejected
        };
        let clip_path = directory.join("clip.motion");
        write_clip(&clip_path, &clip)?;
        let manifest = MotionManifest {
            schema_version: 2,
            semantic: request.semantic.clone(),
            cache_key: key,
            source_revision: "Kimodo-SOMA-RP-v1.1".into(),
            checkpoint: r"F:\Models\Kimodo\Kimodo-SOMA-RP-v1.1".into(),
            prompt: request.prompt.clone(),
            seed: request.seed,
            approval,
            clip: PathBuf::from("clip.motion"),
            preview: None,
        };
        std::fs::write(
            directory.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        index.push(serde_json::json!({
            "semantic": request.semantic,
            "category": request.category,
            "prompt": request.prompt,
            "seed": request.seed,
            "model_revision": "Kimodo-SOMA-RP-v1.1",
            "constraints": request.root_waypoints,
            "source_npz": response.npz,
            "source_bvh": response.bvh,
            "processed_clip": clip_path,
            "duration": clip.duration,
            "sample_rate": clip.sample_rate,
            "root_path": clip.root_positions,
            "contact_data": clip.foot_contacts,
            "validation": {
                "valid": validation.valid,
                "frame_count": validation.frame_count,
                "contact_drift": validation.contact_drift,
                "errors": validation.errors,
            },
            "approval_state": if manifest.approval == ClipApproval::Rejected { "rejected" } else { "generated" },
            "preview": serde_json::Value::Null,
        }));
    }
    index.sort_by(|a, b| a["semantic"].as_str().cmp(&b["semantic"].as_str()));
    let index_path = library_root.join("motion_lab_index.json");
    std::fs::write(&index_path, serde_json::to_vec_pretty(&index)?)?;
    println!(
        "imported {} Kimodo motions; review index={}",
        index.len(),
        index_path.display()
    );
    Ok(())
}
