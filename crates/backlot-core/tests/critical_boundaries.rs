//! Focused tests for the acceptance-critical boundaries of the production
//! pipeline. These are hermetic (no ffmpeg / espeak / network required) except
//! where noted, and guard the properties the PRD insists on:
//!   * humanoid rig joints never collide (so transforms don't overwrite),
//!   * a deterministic plan validates cleanly,
//!   * invalid commands are rejected (never enter the world),
//!   * authorship attribution is truthful (deterministic never claims "llm"),
//!   * TTS durations are *measured* from the audio, and TTS downgrades honestly,
//!   * the full offline producer yields a real, ffprobe-verifiable MP4.

use backlot_core::author::{AuthorSource, DeterministicAuthor, EpisodeAuthor};
use backlot_core::avatar::{HumanoidRig, SemanticJoint};
use backlot_core::config::TtsConfig;
use backlot_core::director::DirectorContext;
use backlot_core::protocol::AuthoredEpisode;
use backlot_core::render::{
    analyze_audio_quality, detect_continuous_noise_floor, detect_silence, mix_audio,
};
use backlot_core::tts::{build_tts, wav_duration_secs};
use backlot_core::validation::{
    adapt_authored_episode, validate_beat_command, validate_plan, ValidatedPlan,
};
use backlot_core::world::build_default_world;
use std::collections::{HashMap, HashSet};

fn ctx() -> DirectorContext {
    DirectorContext {
        world: build_default_world(),
        episode_number: 1,
        seed: 1,
        target_duration: 55.0,
        recent_summaries: vec![],
        tone: vec![],
    }
}

#[test]
fn semantic_joints_are_unique() {
    let mut seen = HashSet::new();
    for j in SemanticJoint::all() {
        assert!(seen.insert(*j), "duplicate SemanticJoint variant: {j:?}");
    }
}

#[test]
fn humanoid_rig_parts_have_unique_joint_keys() {
    // Two parts sharing a joint key would overwrite each other's world matrix.
    let rig = HumanoidRig::default_humanoid("mara", "mara", [0.31, 0.5, 0.9]);
    let mut seen = HashSet::new();
    for p in &rig.parts {
        assert!(
            seen.insert(p.joint),
            "duplicate joint key in rig parts: {:?}",
            p.joint
        );
    }
}

#[test]
fn deterministic_plan_validates_cleanly() {
    let c = ctx();
    let (planned, _) = DeterministicAuthor
        .author(&c)
        .expect("deterministic author");
    assert!(!planned.commands.is_empty());
    for (id, cmd) in &planned.commands {
        let rb = validate_beat_command(&c.world, &planned.plan, cmd);
        assert!(
            rb.is_ok(),
            "deterministic beat {id} should validate: {rb:?}"
        );
    }
}

#[test]
fn validation_rejects_unknown_action() {
    let c = ctx();
    let (planned, _) = DeterministicAuthor.author(&c).unwrap();
    let mut cmd = planned.commands.values().next().unwrap().clone();
    cmd.actions[0].action = "teleport".into();
    assert!(validate_beat_command(&c.world, &planned.plan, &cmd).is_err());
}

#[test]
fn validation_rejects_unknown_actor() {
    let c = ctx();
    let (planned, _) = DeterministicAuthor.author(&c).unwrap();
    let mut cmd = planned.commands.values().next().unwrap().clone();
    cmd.actions[0].actor = "ghost".into();
    assert!(validate_beat_command(&c.world, &planned.plan, &cmd).is_err());
}

#[test]
fn validation_rejects_empty_speak() {
    let c = ctx();
    let (planned, _) = DeterministicAuthor.author(&c).unwrap();
    let mut cmd = planned.commands.values().next().unwrap().clone();
    cmd.actions[0].action = "speak".into();
    cmd.actions[0].text = Some(String::new());
    assert!(validate_beat_command(&c.world, &planned.plan, &cmd).is_err());
}

#[test]
fn deterministic_authorship_is_truthful() {
    let c = ctx();
    let (_, auth) = DeterministicAuthor.author(&c).unwrap();
    assert_eq!(auth.plan_source, AuthorSource::Deterministic);
    assert_ne!(auth.plan_source, AuthorSource::Llm);
    for b in &auth.beats {
        assert_eq!(b.source, AuthorSource::Deterministic);
        assert_ne!(b.source, AuthorSource::Llm);
    }
}

#[test]
fn wav_duration_is_measured_from_header() {
    // Build a tiny valid 1-second mono 16-bit/44.1k WAV and measure it.
    let path = std::env::temp_dir().join("backlot_test_1s.wav");
    let p = path.to_string_lossy().to_string();
    let sr: u32 = 44100;
    let channels: u16 = 1;
    let bits: u16 = 16;
    let data_len: u32 = sr * channels as u32 * (bits as u32 / 8);
    let byte_rate = sr * channels as u32 * (bits as u32 / 8);
    let block_align = channels * (bits / 8);
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sr.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    buf.extend(std::iter::repeat(0u8).take(data_len as usize));
    std::fs::write(&path, &buf).unwrap();

    let d = wav_duration_secs(&p).expect("should parse duration");
    assert!((d - 1.0).abs() < 0.05, "expected ~1.0s, got {d}");
    let quality = analyze_audio_quality(&p).expect("audio quality report");
    assert_eq!(quality.sample_rate, 44100);
    assert_eq!(quality.clipped_samples, 0);
    assert!((quality.duration_secs - 1.0).abs() < 0.01);
    let silence = detect_silence(&p, 0.5, 0.001);
    assert!((silence.max_gap_secs - 1.0).abs() < 0.01);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn production_mix_defaults_to_silence_without_procedural_hum_or_noise() {
    let path = std::env::temp_dir().join("backlot_test_silent_mix.wav");
    let value = path.to_string_lossy().to_string();
    mix_audio(&[], &value, 22050, 3.0);
    let quality = analyze_audio_quality(&value).unwrap();
    assert_eq!(quality.clipped_samples, 0);
    assert_eq!(quality.rms_amplitude, 0.0);
    let silence = detect_silence(&value, 2.5, 0.012);
    assert!(silence.max_gap_secs >= 3.0);
    assert!(!detect_continuous_noise_floor(&value).unwrap().detected);
    let _ = std::fs::remove_file(path);
}

#[test]
fn continuous_low_level_tone_is_rejected_as_a_noise_floor() {
    let path = std::env::temp_dir().join("backlot_test_constant_hum.wav");
    let sample_rate = 22050u32;
    let samples = (0..sample_rate * 3)
        .map(|index| {
            let time = index as f32 / sample_rate as f32;
            ((std::f32::consts::TAU * 55.0 * time).sin() * 0.02 * 32767.0) as i16
        })
        .collect::<Vec<_>>();
    let data_len = samples.len() as u32 * 2;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(&path, wav).unwrap();
    let report = detect_continuous_noise_floor(&path.to_string_lossy()).unwrap();
    assert!(report.detected, "{report:#?}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn frozen_episode_compiles_required_movements_and_has_zero_root_overlaps() {
    let world = build_default_world();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/cached_episode_set_proof.json");
    let authored: AuthoredEpisode = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let (plan, commands) = adapt_authored_episode(&authored, &world).unwrap();
    let mut validated = validate_plan(&world, &plan).unwrap();
    validated.resolved_beats = plan
        .beats
        .iter()
        .map(|beat| validate_beat_command(&world, &plan, commands.get(&beat.id).unwrap()).unwrap())
        .collect();
    let schedule = backlot_core::timeline::build_schedule(&world, &validated, &HashMap::new());
    let unresolved = schedule
        .movement_resolutions
        .iter()
        .filter(|movement| !movement.executed)
        .collect::<Vec<_>>();
    assert!(
        unresolved.is_empty(),
        "unresolved movements: {unresolved:#?}"
    );
    assert!(schedule
        .blocking_plan
        .iter()
        .any(|action| action.actor == "ellis" && action.destination_slot.contains("elevator")));
    assert!(schedule
        .characters
        .iter()
        .find(|character| character.id == "mara")
        .unwrap()
        .actions
        .iter()
        .any(|action| action.action == "activate"
            && action.target.as_deref() == Some("maintenance_panel")));
    let elevator_cues = schedule
        .environment
        .iter()
        .filter(|cue| cue.event == backlot_core::protocol::EnvironmentEventKind::ElevatorDoors)
        .collect::<Vec<_>>();
    assert_eq!(
        elevator_cues.len(),
        2,
        "authored door-open/door-close prose must compile into persistent environment cues"
    );
    let rigs = plan
        .active_characters
        .iter()
        .map(|actor| {
            (
                actor.clone(),
                HumanoidRig::default_humanoid(actor, actor, [0.5, 0.5, 0.5]),
            )
        })
        .collect::<HashMap<_, _>>();
    let opened = backlot_core::timeline::evaluate_at(
        &schedule,
        &rigs,
        &world,
        elevator_cues[0].start + elevator_cues[0].duration,
    );
    assert!(opened.elevator_open > 0.95, "{opened:#?}");
    let closed = backlot_core::timeline::evaluate_at(
        &schedule,
        &rigs,
        &world,
        elevator_cues[1].start + elevator_cues[1].duration,
    );
    assert!(closed.elevator_open < 0.05, "{closed:#?}");
    let overlap = backlot_core::timeline::sample_schedule_overlap(&schedule, &rigs, &world, 30);
    assert_eq!(overlap.character_overlap_frames, 0, "{overlap:#?}");
    assert_eq!(overlap.character_interpenetration_failures, 0);
}

#[test]
fn performance_proof_plan_is_executable_and_overlap_free() {
    let world = build_default_world();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/performance_proof_episode.json");
    let authored: AuthoredEpisode = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let (plan, commands) = adapt_authored_episode(&authored, &world).unwrap();
    let mut validated = validate_plan(&world, &plan).unwrap();
    validated.resolved_beats = plan
        .beats
        .iter()
        .map(|beat| validate_beat_command(&world, &plan, commands.get(&beat.id).unwrap()).unwrap())
        .collect();
    let schedule = backlot_core::timeline::build_schedule(&world, &validated, &HashMap::new());
    let unresolved = schedule
        .movement_resolutions
        .iter()
        .filter(|movement| !movement.executed)
        .collect::<Vec<_>>();
    assert!(unresolved.is_empty(), "{unresolved:#?}");
    let rigs = plan
        .active_characters
        .iter()
        .map(|actor| {
            (
                actor.clone(),
                HumanoidRig::default_humanoid(actor, actor, [0.5; 3]),
            )
        })
        .collect::<HashMap<_, _>>();
    let crossing = backlot_core::timeline::evaluate_at(&schedule, &rigs, &world, 6.3);
    let mara = crossing
        .chars
        .iter()
        .find(|(frame, _)| frame.id == "mara")
        .map(|(frame, _)| frame.root.pos)
        .unwrap();
    let mara_crossing_start = schedule
        .movement_resolutions
        .iter()
        .find(|movement| movement.actor == "mara")
        .and_then(|movement| movement.path.first())
        .copied()
        .unwrap();
    assert!(
        (mara[0] - mara_crossing_start[0]).abs() > 0.1
            || (mara[2] - mara_crossing_start[2]).abs() > 0.1,
        "Mara's dynamically generated near-actor slot resolved in diagnostics but did not execute: {mara:?}"
    );
    let panel_press = backlot_core::timeline::evaluate_at(&schedule, &rigs, &world, 10.2);
    let mara_panel = panel_press
        .chars
        .iter()
        .find(|(frame, _)| frame.id == "mara")
        .map(|(frame, _)| &frame.root)
        .unwrap();
    assert!(
        mara_panel.rot[1].abs() > 1.5,
        "Mara must face the panel while pressing it: {:?}",
        mara_panel.rot
    );
    let overlap = backlot_core::timeline::sample_schedule_overlap(&schedule, &rigs, &world, 30);
    assert_eq!(overlap.character_overlap_frames, 0, "{overlap:#?}");
}

#[test]
fn tts_downgrades_honestly_when_espeak_unreachable() {
    let cfg = TtsConfig {
        provider: "espeak".into(),
        executable: "this_binary_does_not_exist_xyz".into(),
        ..Default::default()
    };
    let t = build_tts(&cfg);
    // Downgrades to the duration-only stub; crucially it does NOT pretend to be
    // a real provider.
    assert_eq!(t.provider_name(), "estimating");
}

#[test]
fn schedule_rebuild_uses_measured_tts_duration_for_dialogue_and_captions() {
    let c = ctx();
    let (planned, _) = DeterministicAuthor.author(&c).unwrap();
    let resolved_beats = planned
        .plan
        .beats
        .iter()
        .map(|beat| {
            validate_beat_command(
                &c.world,
                &planned.plan,
                planned.commands.get(&beat.id).unwrap(),
            )
            .unwrap()
        })
        .collect();
    let validated = ValidatedPlan {
        plan: planned.plan,
        resolved_beats,
    };
    let speech = validated
        .resolved_beats
        .iter()
        .flat_map(|beat| &beat.resolved_actions)
        .find(|action| action.text.is_some())
        .unwrap();
    let text = speech.text.clone().unwrap();

    let short = HashMap::from([((speech.actor_id.clone(), text.clone()), 1.25)]);
    let long = HashMap::from([((speech.actor_id.clone(), text.clone()), 3.75)]);
    let short_schedule = backlot_core::timeline::build_schedule(&c.world, &validated, &short);
    let long_schedule = backlot_core::timeline::build_schedule(&c.world, &validated, &long);
    let short_line = short_schedule
        .dialogue
        .iter()
        .find(|line| line.actor == speech.actor_id && line.text == text)
        .unwrap();
    let long_line = long_schedule
        .dialogue
        .iter()
        .find(|line| line.actor == speech.actor_id && line.text == text)
        .unwrap();
    assert!(((short_line.end - short_line.start) - 1.25).abs() < 0.01);
    assert!(((long_line.end - long_line.start) - 3.75).abs() < 0.01);
    assert!(long_schedule.duration > short_schedule.duration);
}

#[test]
#[ignore = "runs the full offline producer (needs ffmpeg + espeak on disk)"]
fn offline_producer_yields_real_mp4() {
    use backlot_core::config::Config;
    use backlot_core::render::{produce_episode, ProduceConfig};
    let c = ctx();
    let mut config = Config::default();
    config.runtime.ffmpeg_path = "ffmpeg".into();
    config.tts.provider = "espeak".into();
    config.tts.executable = "C:/Program Files/eSpeak NG/espeak-ng".into();
    config.runtime.output_dir = std::env::temp_dir()
        .join("backlot_test_out")
        .to_string_lossy()
        .to_string();
    let cfg = ProduceConfig {
        config,
        require_llm: false,
        world: c.world.clone(),
        seed: 1,
        episode_number: 1,
        keep_frames: false,
    };
    let report = produce_episode(cfg, Box::new(DeterministicAuthor)).unwrap();
    assert!(report.mp4_produced, "mp4 should be produced");
    assert!(report.ffprobe_ok, "ffprobe should verify the mp4");
    assert_eq!(report.plan_author_source, "deterministic");
}
