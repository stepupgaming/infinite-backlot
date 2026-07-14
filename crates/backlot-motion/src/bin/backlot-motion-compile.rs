use backlot_motion::bvh::{parse_bvh, to_processed_clip, MotionSidecar};
use backlot_motion::library::{cache_key, write_clip, ClipApproval, MotionManifest};
use backlot_motion::{process_clip, MotionProcessingConfig, RetargetMap};
use std::path::{Path, PathBuf};

fn value(args: &[String], flag: &str) -> Result<String, String> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
        .ok_or_else(|| format!("missing {flag}"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let semantic = value(&args, "--semantic")?;
    let prompt = value(&args, "--prompt")?;
    let seed: u64 = value(&args, "--seed")?.parse()?;
    let bvh_path = PathBuf::from(value(&args, "--bvh")?);
    let sidecar_path = PathBuf::from(value(&args, "--sidecar")?);
    let output_root = PathBuf::from(value(&args, "--output-root")?);
    let source_revision = value(&args, "--source-revision")?;
    let checkpoint = value(&args, "--checkpoint")?;
    let approved = args.iter().any(|arg| arg == "--approve");

    let key = cache_key(&[
        &semantic,
        &prompt,
        &seed.to_string(),
        &source_revision,
        &checkpoint,
        &std::fs::read_to_string(&sidecar_path)?,
    ]);
    let directory = output_root.join(&semantic).join(&key);
    std::fs::create_dir_all(&directory)?;
    let motion = parse_bvh(&std::fs::read_to_string(&bvh_path)?)?;
    let sidecar: MotionSidecar = serde_json::from_slice(&std::fs::read(&sidecar_path)?)?;
    let retarget = RetargetMap::soma77_to_kaykit();
    retarget
        .validate()
        .map_err(|error| format!("invalid retarget map: {error}"))?;
    let mut clip = to_processed_clip(&motion, &sidecar, &semantic, &retarget, false)?;
    let validation = process_clip(&mut clip, &MotionProcessingConfig::default());
    if !validation.valid {
        return Err(format!("motion validation failed: {:?}", validation.errors).into());
    }
    let clip_path = directory.join("clip.motion");
    write_clip(&clip_path, &clip)?;
    let manifest = MotionManifest {
        schema_version: 2,
        semantic: semantic.clone(),
        cache_key: key,
        source_revision,
        checkpoint,
        prompt,
        seed,
        approval: if approved {
            ClipApproval::Approved
        } else {
            ClipApproval::Pending
        },
        clip: relative_or_absolute(&directory, &clip_path),
        preview: None,
    };
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    std::fs::write(
        directory.join("validation.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "valid": validation.valid,
            "frame_count": validation.frame_count,
            "contact_drift_m": validation.contact_drift,
            "errors": validation.errors,
            "source_bvh": bvh_path,
            "source_sidecar": sidecar_path,
        }))?,
    )?;
    println!("{}", directory.display());
    Ok(())
}

fn relative_or_absolute(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}
