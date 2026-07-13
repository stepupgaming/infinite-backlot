//! Episode player: turns a validated plan into deterministic, watchable motion.
//!
//! Runs during `Rehearsing` (authoring pass, records the timeline) and
//! `Rendering` (deterministic replay at higher quality). The LLM is never
//! touched here — only the committed plan is replayed.

use crate::scene::Hud;
use crate::state::*;
use backlot_core::validation::ResolvedAction;
use bevy::prelude::*;

const NAV_EPS: f32 = 0.12;

/// Resolve a target id to a world position (mark, character, or prop).
fn resolve_pos(
    target: &str,
    scene: &SceneIndex,
    char_map: &std::collections::HashMap<String, (Vec3, Entity, String)>,
    prop_map: &std::collections::HashMap<String, Vec3>,
) -> Option<Vec3> {
    if let Some(p) = scene.marks.get(target) {
        return Some(*p);
    }
    if let Some((p, _, _)) = char_map.get(target) {
        return Some(*p);
    }
    if let Some(p) = prop_map.get(target) {
        return Some(*p);
    }
    None
}

/// Pick a camera transform for an intent + subject (PRD §18).
fn compute_camera(
    intent: &str,
    subj: Vec3,
    react: Option<Vec3>,
    anchors: &[(Vec3, Vec3)],
) -> (Vec3, Vec3) {
    let base = subj + Vec3::new(0.0, 1.0, 0.0);
    let (pos, look) = match intent {
        "establish" | "comedic_wide" | "group_coverage" => {
            if let Some((a, b)) = anchors.first() {
                (*a, *b)
            } else {
                (subj + Vec3::new(0.0, 3.0, 7.0), base)
            }
        }
        "speaker_closeup" | "follow" | "conversation" => (subj + Vec3::new(0.0, 1.0, 3.0), base),
        "reaction" => {
            let r = react.unwrap_or(subj);
            (r + Vec3::new(1.0, 1.2, 3.0), r + Vec3::new(0.0, 1.0, 0.0))
        }
        "reveal" | "insert_object" => (subj + Vec3::new(1.5, 1.2, 3.0), base),
        "tension_push" => (subj + Vec3::new(0.0, 1.4, 2.2), base),
        "cliffhanger_hold" => (subj + Vec3::new(0.0, 1.6, 2.4), base),
        "over_the_shoulder" => (subj + Vec3::new(-1.5, 1.4, 2.6), base),
        "exit_transition" => (subj + Vec3::new(0.0, 2.0, 5.0), base),
        _ => (subj + Vec3::new(0.0, 1.5, 4.0), base),
    };
    (pos, look)
}

pub fn navigation_system(
    time: Res<Time>,
    mut chars: Query<(&mut CharacterAvatar, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut av, mut tf) in chars.iter_mut() {
        if let Some(target) = av.nav_target {
            let dir = target - tf.translation;
            let dist = dir.length();
            if dist <= NAV_EPS {
                tf.translation = target;
                av.nav_target = None;
            } else {
                let step = dir.normalize() * (av.speed * dt).min(dist);
                tf.translation += step;
                // Face direction of travel.
                let yaw = dir.x.atan2(dir.z);
                tf.rotation = Quat::from_rotation_y(yaw);
            }
        }
    }
}

pub fn flicker_system(time: Res<Time>, mut lights: Query<(&mut PointLight, &mut FlickerLight)>) {
    for (mut light, mut fl) in lights.iter_mut() {
        if fl.active {
            fl.phase += time.delta_secs() * 22.0;
            let f = 0.45 + 0.55 * (fl.phase.sin().abs());
            light.intensity = fl.base_intensity * f;
        } else {
            light.intensity = fl.base_intensity;
        }
    }
}

pub fn camera_system(
    time: Res<Time>,
    mut cams: Query<(&mut Transform, &mut CameraRig), With<MainCamera>>,
) {
    let k = 1.0 - (-3.0 * time.delta_secs()).exp();
    for (mut tf, mut rig) in cams.iter_mut() {
        rig.current_look = rig.current_look.lerp(rig.desired_look, k);
        tf.translation = tf.translation.lerp(rig.desired_pos, k);
        tf.look_at(rig.current_look, Vec3::Y);
    }
}

pub fn hud_system(
    mut indicators: Query<&mut Transform, With<SpeechIndicator>>,
    chars: Query<(Entity, &CharacterAvatar, &Transform), Without<SpeechIndicator>>,
    hud: ResMut<Hud>,
    active: Res<ActiveCaption>,
    state: Res<State<AppState>>,
    mut bar_q: Query<&mut BackgroundColor>,
) {
    // Position the speech indicator above the active speaker.
    if let Some(mut ind) = indicators.iter_mut().next() {
        if active.active && !active.text.is_empty() {
            // find speaker entity position
            for (_, av, tf) in chars.iter() {
                if av.display == active.speaker {
                    ind.translation = tf.translation + Vec3::new(0.0, 1.9, 0.0);
                    break;
                }
            }
        } else {
            ind.translation = Vec3::new(0.0, -10.0, 0.0);
        }
    }

    // Overlay color reflects state; bottom bar width reflects caption presence.
    if let Some(h) = &hud.0 {
        if let Ok(mut top) = bar_q.get_mut(h.top) {
            let c = match state.get() {
                AppState::Rehearsing => Color::srgb(0.9, 0.7, 0.2),
                AppState::Rendering => Color::srgb(0.9, 0.3, 0.3),
                AppState::Reviewing => Color::srgb(0.3, 0.7, 0.9),
                AppState::EpisodePlanning | AppState::PlanValidation => Color::srgb(0.6, 0.4, 0.9),
                _ => Color::srgb(0.2, 0.8, 0.4),
            };
            *top = BackgroundColor(c);
        }
        if let Ok(mut bottom) = bar_q.get_mut(h.bottom) {
            bottom.0 = bottom.0.with_alpha(if active.active { 0.85 } else { 0.0 });
        }
    }
}

/// The main deterministic stepper. Drives one frame of the current episode.
#[allow(clippy::too_many_arguments)]
pub fn player_system(
    mut player: ResMut<Player>,
    mut clock: ResMut<EpisodeClock>,
    mut log: ResMut<RehearsalLog>,
    current: Res<CurrentEpisode>,
    scene: Res<SceneIndex>,
    mut active: ResMut<ActiveCaption>,
    run: ResMut<RunControl>,
    mut next: ResMut<NextState<AppState>>,
    time: Res<Time>,
    world: Res<CanonicalWorld>,
    mut chars: Query<
        (Entity, &mut CharacterAvatar, &mut Transform),
        (Without<PropMarker>, Without<MainCamera>),
    >,
    props: Query<
        (Entity, &PropMarker, &Transform),
        (Without<CharacterAvatar>, Without<MainCamera>),
    >,
    mut cams: Query<
        (Entity, &mut Transform, &mut CameraRig),
        (
            With<MainCamera>,
            Without<CharacterAvatar>,
            Without<PropMarker>,
        ),
    >,
    mut lights: Query<(&mut PointLight, &mut FlickerLight)>,
) {
    if !player.active {
        return;
    }
    let validated = match &current.validated {
        Some(v) => v,
        None => {
            player.active = false;
            return;
        }
    };

    // Initialize the current beat on first entry.
    if player.initialized_beat != Some(player.beat_index) {
        begin_beat(
            &mut player,
            &mut clock,
            &mut log,
            validated,
            &scene,
            &mut chars,
            &mut cams,
        );
    }

    let dt = time.delta_secs() * clock.scale;
    clock.elapsed += dt;
    player.beat_elapsed += dt;
    player.since_event += dt;
    if player.since_event > log.dead_air_max {
        log.dead_air_max = player.since_event;
    }

    // Build position maps for resolution (immutable pass over mutable query).
    let mut char_map: std::collections::HashMap<String, (Vec3, Entity, String)> =
        std::collections::HashMap::new();
    let mut prop_map: std::collections::HashMap<String, Vec3> = std::collections::HashMap::new();
    for (e, av, tf) in chars.iter() {
        char_map.insert(av.id.clone(), (tf.translation, e, av.display.clone()));
    }
    for (_, p, tf) in props.iter() {
        prop_map.insert(p.id.clone(), tf.translation);
    }

    // Fire any due actions.
    while player.action_cursor < player.schedule.len()
        && player.schedule[player.action_cursor].0 <= player.beat_elapsed
    {
        let (_, act) = player.schedule[player.action_cursor].clone();
        fire_action(
            &act,
            &mut player,
            &mut clock,
            &mut log,
            &mut active,
            &scene,
            &char_map,
            &prop_map,
            &world,
            &mut chars,
            &mut lights,
            &run,
        );
        player.action_cursor += 1;
    }

    // Watchability governor: never allow more than max_dead_air without a beat
    // ending (a deterministic repair / skip).
    if player.since_event > run.config.runtime.max_dead_air_secs && !player.finished {
        log.repairs += 1;
        tracing::warn!(
            "dead-air {:.1}s exceeded; forcing beat completion (repair)",
            player.since_event
        );
        player.beat_elapsed = player.beat_duration;
    }

    // Beat completion.
    if player.beat_elapsed >= player.beat_duration {
        if let Some(shot) = log.camera.last_mut() {
            shot.end = clock.elapsed;
        }
        player.beat_index += 1;
        if player.beat_index >= validated.resolved_beats.len() {
            player.finished = true;
            player.active = false;
            if player.render_pass {
                if run.replaying {
                    next.set(AppState::Reviewing);
                } else {
                    next.set(AppState::Committing);
                }
            } else {
                next.set(AppState::EpisodeReady);
            }
        } else {
            player.initialized_beat = None;
        }
    }
}

fn begin_beat(
    player: &mut Player,
    clock: &mut EpisodeClock,
    log: &mut RehearsalLog,
    validated: &backlot_core::validation::ValidatedPlan,
    scene: &SceneIndex,
    chars: &mut Query<
        (Entity, &mut CharacterAvatar, &mut Transform),
        (Without<PropMarker>, Without<MainCamera>),
    >,
    cams: &mut Query<
        (Entity, &mut Transform, &mut CameraRig),
        (
            With<MainCamera>,
            Without<CharacterAvatar>,
            Without<PropMarker>,
        ),
    >,
) {
    let rb = &validated.resolved_beats[player.beat_index];
    let mut t = 0.0;
    let mut sched = Vec::new();
    for a in &rb.resolved_actions {
        sched.push((t, a.clone()));
        t += a.estimated_duration;
    }
    player.schedule = sched;
    player.action_cursor = 0;
    player.beat_duration = (t + 0.8).max(rb.completion.seconds.unwrap_or(0.0));
    player.beat_elapsed = 0.0;
    player.initialized_beat = Some(player.beat_index);
    player.since_event = 0.0;

    // Camera intent.
    let intent = rb.camera_intent.r#type.clone();
    let subject_pos = scene
        .characters
        .get(&rb.camera_intent.subject)
        .and_then(|e| {
            chars
                .iter()
                .find(|(ent, _, _)| *ent == *e)
                .map(|(_, _, tf)| tf.translation)
        })
        .unwrap_or(Vec3::ZERO);
    let reaction_pos = rb
        .camera_intent
        .reaction_subject
        .as_ref()
        .and_then(|r| scene.characters.get(r))
        .and_then(|e| {
            chars
                .iter()
                .find(|(ent, _, _)| *ent == *e)
                .map(|(_, _, tf)| tf.translation)
        });
    let (pos, look) = compute_camera(&intent, subject_pos, reaction_pos, &scene.anchors);
    for (_, _, mut rig) in cams.iter_mut() {
        rig.intent = intent.clone();
        rig.desired_pos = pos;
        rig.desired_look = look;
    }

    log.camera.push(backlot_core::package::CameraShot {
        start: clock.elapsed,
        end: clock.elapsed,
        intent: intent.clone(),
        subject: rb.camera_intent.subject.clone(),
        position: [pos.x, pos.y, pos.z],
        look_at: [look.x, look.y, look.z],
    });

    if rb.outline.beat_type == "hook" {
        log.hook_time = Some(clock.elapsed);
    }
}

#[allow(clippy::too_many_arguments)]
fn fire_action(
    act: &ResolvedAction,
    player: &mut Player,
    clock: &mut EpisodeClock,
    log: &mut RehearsalLog,
    active: &mut ActiveCaption,
    scene: &SceneIndex,
    char_map: &std::collections::HashMap<String, (Vec3, Entity, String)>,
    prop_map: &std::collections::HashMap<String, Vec3>,
    world: &CanonicalWorld,
    chars: &mut Query<
        (Entity, &mut CharacterAvatar, &mut Transform),
        (Without<PropMarker>, Without<MainCamera>),
    >,
    lights: &mut Query<(&mut PointLight, &mut FlickerLight)>,
    run: &RunControl,
) {
    let display = char_map
        .get(&act.actor_id)
        .map(|(_, _, d)| d.clone())
        .unwrap_or_else(|| act.actor_id.clone());

    let record = !player.render_pass;
    if record {
        log.events.push(backlot_core::package::TimedEvent {
            t: clock.elapsed,
            kind: act.action.clone(),
            actor: Some(act.actor_id.clone()),
            target: act.target_id.clone(),
            detail: act.text.clone().unwrap_or_default(),
        });
        log.visual_changes += 1;
    }
    player.since_event = 0.0;

    match act.action.as_str() {
        "speak" | "whisper" | "shout" => {
            let text = act.text.clone().unwrap_or_default();
            let dur = act.estimated_duration;
            // Mark speaker as speaking.
            if let Some((_, e, _)) = char_map.get(&act.actor_id) {
                if let Ok((_, mut av, _)) = chars.get_mut(*e) {
                    av.speaking_until = clock.elapsed + dur;
                    av.emote = "speaking".into();
                }
            }
            let voice = world
                .0
                .character(&act.actor_id)
                .map(|c| c.voice_id.clone())
                .unwrap_or_else(|| act.actor_id.clone());
            active.text = text.clone();
            active.speaker = display.clone();
            active.until = clock.elapsed + dur;
            active.active = true;
            if record {
                if log.objective_time.is_none() {
                    log.objective_time = Some(clock.elapsed);
                }
                log.dialogue.push(backlot_core::package::DialogueLine {
                    start: clock.elapsed,
                    end: clock.elapsed + dur,
                    actor: act.actor_id.clone(),
                    text: text.clone(),
                    voice_id: voice,
                });
                log.captions.push(backlot_core::package::Caption {
                    start: clock.elapsed,
                    end: clock.elapsed + dur,
                    text: text.clone(),
                });
                // Caption to the production console.
                println!("  [{}] {}", display, text);
            }
        }
        "flicker_lights" => {
            for (_, mut fl) in lights.iter_mut() {
                fl.active = true;
            }
        }
        "move_to" | "approach" | "retreat_from" | "follow" | "flee_to" | "enter_room"
        | "exit_room" => {
            if let Some(pos) = act
                .target_id
                .as_ref()
                .and_then(|t| resolve_pos(t, scene, char_map, prop_map))
            {
                if let Some((_, e, _)) = char_map.get(&act.actor_id) {
                    if let Ok((_, mut av, _)) = chars.get_mut(*e) {
                        av.nav_target = Some(pos);
                    }
                }
            }
        }
        "point_at" | "look_at" | "turn_toward" => {
            if let Some((_, e, _)) = char_map.get(&act.actor_id) {
                if let Ok((_, mut av, _)) = chars.get_mut(*e) {
                    av.emote = "point".into();
                }
            }
        }
        "react" | "sigh" | "laugh" | "gesture" | "display_emotion" | "conceal_emotion" => {
            if let Some((_, e, _)) = char_map.get(&act.actor_id) {
                if let Ok((_, mut av, _)) = chars.get_mut(*e) {
                    av.emote = act.action.clone();
                }
            }
        }
        _ => {
            // Interaction / object actions → small emote + optional approach.
            if let Some((_, e, _)) = char_map.get(&act.actor_id) {
                if let Ok((_, mut av, _)) = chars.get_mut(*e) {
                    av.emote = "act".into();
                }
            }
        }
    }
    let _ = run;
}
