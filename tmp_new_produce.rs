/// Result of the shared preparation stage (authoring + validation + TTS +
/// schedule + rigs). Both renderers consume this; neither re-derives state.
pub struct PreparedProduction {
    pub episode_id: String,
    pub planned: PlannedEpisode,
    pub auth: PlanAuthorship,
    pub validated: ValidatedPlan,
    pub schedule: Schedule,
    pub rigs: HashMap<String, HumanoidRig>,
    pub clips: Vec<(String, f32, f32)>,
    pub any_real: bool,
    pub tts_provider: String,
    pub world_before: WorldState,
}

/// Stage 1 (shared): author, validate, synthesize TTS, build the authoritative
/// schedule, build rigs, collect audio clips. No frames are rendered here.
pub fn prepare_production(
    config: &Config,
    require_llm: bool,
    world: &WorldState,
    seed: u64,
    episode_number: u64,
    author: &dyn EpisodeAuthor,
) -> crate::error::Result<PreparedProduction> {
    let episode_id = serial_id("episode", episode_number, 6);
    let ctx = crate::director::DirectorContext {
        world: world.clone(),
        episode_number,
        seed,
        target_duration: config.runtime.target_duration_secs,
        recent_summaries: vec![],
        tone: vec!["surreal".into(), "comedy".into()],
    };
    let (planned, auth) = author.author(&ctx)?;
    let plan = planned.plan.clone();
    let validated = build_validated(world, &planned)
        .ok_or_else(|| crate::error::CoreError::EmptyPlan)?;

    let tts = build_tts(&config.tts);
    let provider = tts.provider_name().to_string();
    let mut tts_durations: HashMap<(String, String), f32> = HashMap::new();
    let mut clips: Vec<(String, f32, f32)> = Vec::new();
    let mut any_real = false;
    for ra in validated.resolved_beats.iter().flat_map(|b| b.resolved_actions.iter()) {
        if matches!(action_kind(&ra.action), ActionKind::Speak) {
            let text = ra.text.clone().unwrap_or_default();
            let voice = world
                .character(&ra.actor_id)
                .map(|c| c.voice_id.clone())
                .unwrap_or_else(|| ra.actor_id.clone());
            let res = tts.synthesize(&text, &voice);
            if res.ok {
                any_real = true;
            }
            tts_durations.insert((ra.actor_id.clone(), text), res.duration);
        }
    }
    let sched = build_schedule(world, &validated, &tts_durations);
    for d in &sched.dialogue {
        let voice = world
            .character(&d.actor)
            .map(|c| c.voice_id.clone())
            .unwrap_or_else(|| d.actor.clone());
        let res = tts.synthesize(&d.text, &voice);
        if let Some(p) = &res.audio_path {
            clips.push((p.clone(), d.start, d.end - d.start));
        }
    }
    let rigs = build_rigs(world);
    Ok(PreparedProduction {
        episode_id,
        planned,
        auth,
        validated,
        schedule: sched,
        rigs,
        clips,
        any_real,
        tts_provider: provider,
        world_before: world.clone(),
    })
}

/// Build the committed camera plan (eye/look sampled at each shot midpoint).
pub fn build_camera_plan(
    world: &WorldState,
    sched: &Schedule,
    rigs: &HashMap<String, HumanoidRig>,
) -> Vec<CameraShot> {
    sched
        .camera_shots
        .iter()
        .map(|s| {
            let mid = (s.start + s.end) / 2.0;
            let st = evaluate_at(sched, rigs, world, mid);
            CameraShot {
                start: s.start,
                end: s.end,
                intent: s.intent.clone(),
                subject: s.subject.clone(),
                position: st.camera_eye,
                look_at: st.camera_look,
            }
        })
        .collect()
}

/// Stage 3 (shared): mix audio, encode MP4, verify, package, and write truthful
/// logs. `render_backend` is recorded in the diagnostics so the artifact never
/// lies about which renderer produced the imagery.
pub fn finalize_production(
    config: &Config,
    require_llm: bool,
    prep: &PreparedProduction,
    frames_dir: &Path,
    captured: u32,
    render_backend: &str,
) -> crate::error::Result<ProduceReport> {
    let out_dir = &config.runtime.output_dir;
    let ep_dir = Path::new(out_dir).join("episodes").join(&prep.episode_id);
    let audio_dir = ep_dir.join("audio");
    let llm_dir = ep_dir.join("llm");
    std::fs::create_dir_all(&audio_dir).map_err(io_err(&ep_dir))?;
    std::fs::create_dir_all(&llm_dir).map_err(io_err(&ep_dir))?;

    let sched = &prep.schedule;
    let rigs = &prep.rigs;
    let world = &prep.world_before;
    let plan = prep.planned.plan.clone();
    let auth = &prep.auth;

    // Mix audio
    let sr = config.tts.sample_rate;
    let mix_path = audio_dir.join("final_mix.wav");
    mix_audio(&prep.clips, mix_path.to_str().unwrap(), sr, sched.duration);

    // Encode MP4
    let fps = config.runtime.frame_rate.max(1);
    let cap_out = ep_dir.join("output").join("vertical_captioned.mp4");
    let clean_out = ep_dir.join("output").join("vertical_clean.mp4");
    std::fs::create_dir_all(ep_dir.join("output")).map_err(io_err(&ep_dir))?;
    let (cmd, enc_ok) = encode_mp4(
        config,
        frames_dir.to_str().unwrap(),
        mix_path.to_str().unwrap(),
        cap_out.to_str().unwrap(),
        clean_out.to_str().unwrap(),
        &sched.captions,
        config.runtime.resolution,
        fps,
    )?;

    // Verify
    let probe = verify_mp4(config, cap_out.to_str().unwrap());
    let ffprobe_ok = probe.has_video && probe.has_audio && probe.duration >= sched.duration * 0.8;

    // Package
    let mut world_after = world.clone();
    let _delta = apply_persistent_changes(&mut world_after, &plan.persistent_changes);

    let llm_used = auth.plan_source == AuthorSource::Llm
        || auth.beats.iter().any(|b| b.source == AuthorSource::Llm);
    let plan_source = auth.plan_source.as_str().to_string();

    let mut m = EpisodeMetrics::default();
    m.hook_latency_secs = sched.camera_shots.first().map(|s| s.start).unwrap_or(0.0);
    m.objective_understandable_secs = sched
        .dialogue
        .first()
        .map(|d| d.start)
        .unwrap_or(sched.duration);
    m.dead_air_secs = compute_max_gap(&sched.dialogue, sched.duration);
    m.avg_shot_duration = if sched.camera_shots.is_empty() {
        0.0
    } else {
        sched.camera_shots.iter().map(|s| s.end - s.start).sum::<f32>() / sched.camera_shots.len() as f32
    };
    m.longest_shot_duration = sched
        .camera_shots
        .iter()
        .map(|s| s.end - s.start)
        .fold(0.0f32, f32::max);
    m.visual_changes_per_min = (sched.events.len() as f32) / (sched.duration / 60.0);
    m.payoff_complete = !plan.payoff.trim().is_empty();
    m.persistent_consequence = !plan.persistent_changes.is_empty();

    let transcript: String = sched
        .dialogue
        .iter()
        .map(|d| format!("{}: {}", d.actor, d.text))
        .collect::<Vec<_>>()
        .join("\n");
    let camera_plan = build_camera_plan(world, sched, rigs);

    let diagnostics = Diagnostics {
        episode_id: prep.episode_id.clone(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        director: plan_source.clone(),
        llm_requests: auth.beats.iter().map(|b| b.attempts).sum::<u32>() + auth.attempts,
        llm_failures: auth
            .beats
            .iter()
            .filter(|b| b.source == AuthorSource::DeterministicFallback)
            .count() as u32,
        validation_errors: vec![],
        repairs: 0,
        metrics: m.clone(),
        issues: vec![],
        require_llm,
        llm_used,
        plan_author_source: plan_source.clone(),
        authorship: Some(auth.clone()),
        tts_provider: prep.tts_provider.clone(),
        tts_real: prep.any_real,
        audio_real: prep.any_real,
        frames_captured: captured > 0,
        mp4_produced: enc_ok,
        ffmpeg_command: Some(cmd.clone()),
        ffprobe_ok,
        replay_no_llm: true,
        render_backend: render_backend.to_string(),
    };

    let gemmy = GemmyManifest {
        title: plan.episode_title.clone(),
        summary: plan.logline.clone(),
        hook_text: sched
            .captions
            .first()
            .map(|c| c.text.clone())
            .unwrap_or_default(),
        duration_secs: sched.duration,
        characters: plan.active_characters.clone(),
        transcript: transcript.clone(),
        caption_style: config.runtime.caption_style.clone(),
        render_paths: vec![
            "output/vertical_captioned.mp4".into(),
            "output/vertical_clean.mp4".into(),
        ],
        thumbnail_candidates: vec!["output/thumbnail_01.png".into()],
        story_tags: plan.tone.clone(),
        quality_scores: Default::default(),
        detected_issues: vec![],
        canonical: true,
        suggested_posting_caption: format!("{} #shorts", plan.episode_title),
        suggested_compilation_category: "surreal-comedy".into(),
    };

    let mut pkg = EpisodePackage {
        id: prep.episode_id.clone(),
        title: plan.episode_title.clone(),
        logline: plan.logline.clone(),
        duration_secs: sched.duration,
        canonical: true,
        plan: plan.clone(),
        world_before: world.clone(),
        world_after,
        events: sched.events.clone(),
        dialogue: sched.dialogue.clone(),
        captions: sched.captions.clone(),
        camera_plan,
        metrics: m.clone(),
        diagnostics: diagnostics.clone(),
        gemmy,
        report_md: String::new(),
    };
    pkg.build_report();
    pkg.write(out_dir)?;

    write_llm_logs(&llm_dir, auth, &prep.planned, require_llm, llm_used);
    write_render_manifest(
        &ep_dir,
        &cmd,
        &probe,
        cap_out.to_str().unwrap(),
        clean_out.to_str().unwrap(),
        sched.duration,
    );
    write_tts_manifest(&audio_dir, &prep.clips, prep.tts_provider.clone(), prep.any_real);

    Ok(ProduceReport {
        episode_id: prep.episode_id.clone(),
        mp4_captioned: cap_out.to_string_lossy().into_owned(),
        mp4_clean: clean_out.to_string_lossy().into_owned(),
        duration_secs: sched.duration,
        frames: captured,
        require_llm,
        plan_author_source: plan_source,
        llm_used,
        tts_provider: prep.tts_provider.clone(),
        tts_real: prep.any_real,
        audio_real: prep.any_real,
        frames_captured: captured > 0,
        mp4_produced: enc_ok,
        ffprobe_ok,
        probe,
        issues: diagnostics.issues.clone(),
        ffmpeg_command: Some(cmd),
    })
}

/// CPU software-rasterizer production path (regression / offline fallback).
pub fn produce_episode(
    cfg: ProduceConfig,
    author: Box<dyn EpisodeAuthor>,
) -> crate::error::Result<ProduceReport> {
    let ProduceConfig { config, require_llm, world, seed, episode_number, keep_frames } = cfg;
    let out_dir = config.runtime.output_dir.clone();
    let ep_dir = Path::new(&out_dir).join("episodes").join(serial_id("episode", episode_number, 6));
    let frames_dir = ep_dir.join("frames");
    std::fs::create_dir_all(&frames_dir).map_err(io_err(&ep_dir))?;

    let prep = prepare_production(&config, require_llm, &world, seed, episode_number, &*author)?;

    // CPU software-rasterizer frame capture (deterministic, no LLM).
    let fps = config.runtime.frame_rate.max(1);
    let (rw, rh) = (config.runtime.resolution.0 / 2, config.runtime.resolution.1 / 2);
    let renderer = StageRenderer::new(rw.max(2), rh.max(2));
    let n_frames = (prep.schedule.duration * fps as f32).ceil() as u32;
    let mut captured = 0u32;
    for i in 0..n_frames {
        let t = i as f32 / fps as f32;
        let state = evaluate_at(&prep.schedule, &prep.rigs, &world, t);
        let rgba = renderer.render(&state, &prep.rigs, &world);
        let path = frames_dir.join(format!("frame_{:06}.png", i + 1));
        if write_png(&path, rw, rh, &rgba).is_err() {
            tracing::warn!("frame write failed");
            break;
        }
        captured += 1;
    }

    let report = finalize_production(&config, require_llm, &prep, &frames_dir, captured, "cpu_software")?;

    if !keep_frames && captured > 0 {
        let _ = std::fs::remove_dir_all(&frames_dir);
    }
    Ok(report)
}

// ---- helpers for produce ----
