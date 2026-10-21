//! BlueCraft's native client. Fixed ticks, devices, saves and capture use BlueEngine.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod audio;
mod controls;
mod platform;
mod presentation;

use bluecraft::{Event, Input, Sim};
use macroquad::prelude::*;
use vesper3d::viewer::{
    devkit::{
        beside_exe, flag_value, has_flag, parse_size, snapshot, Lifecycle, MouseLook, Settings, Simulation, AUTO_SLOT,
    },
    game_client::{self, AudioMenu, GameShell},
    game_input::{mouse_pixels, ClientInput},
    identity::Identity,
    kit::{self, SoundBank, View},
};

const IDENTITY: &str = include_str!("../assets/identity.json");
const CUES: [&str; 12] =
    ["fwd", "back", "left", "right", "jump", "look", "mine", "place", "pick", "slot", "sprint", "crouch"];
const JUMP: u32 = 1;
const PICK: u32 = 2;
const MINE_EDGE: u32 = 4;
const PLACE_EDGE: u32 = 8;
const SLOT_START: u32 = 8;

fn identity() -> Identity {
    Identity::parse(IDENTITY).expect("BlueCraft identity must be valid")
}

fn window() -> macroquad::conf::Conf {
    platform::attach_console();
    let mut conf = game_client::window_config_with_icon(
        &identity().title,
        game_client::icon_from_rgba(
            include_bytes!("../assets/icon_16.rgba"),
            include_bytes!("../assets/icon_32.rgba"),
            include_bytes!("../assets/icon_64.rgba"),
        ),
    );
    let args: Vec<String> = std::env::args().collect();
    if let Some((w, h)) = flag_value(&args, "--size").and_then(parse_size) {
        conf.miniquad_conf.window_width = w.clamp(320, 7680) as i32;
        conf.miniquad_conf.window_height = h.clamp(240, 4320) as i32;
    }
    if has_flag(&args, "--novsync") {
        conf.miniquad_conf.platform.swap_interval = Some(0);
    }
    conf
}

#[derive(Clone, Copy, Default)]
struct Held {
    forward: f32,
    right: f32,
    sprint: bool,
    crouch: bool,
    mine: bool,
    place: bool,
}

#[derive(Default, serde::Serialize)]
struct DeviceEvidence {
    movement_frames: u64,
    look_frames: u64,
    jump_presses: u64,
    mine_frames: u64,
    place_frames: u64,
    selection_presses: u64,
    successful_saves: u64,
    successful_loads: u64,
}

#[macroquad::main(window)]
async fn main() {
    vesper3d::viewer::startup::show().await;
    let args: Vec<String> = std::env::args().collect();
    let identity = identity();
    let mut life = Lifecycle::<Held>::start_or_exit(&args, &CUES);
    let unattended = life.options.unattended();
    // Explicit seeds and unattended runs never overwrite the player's automatic world.
    let automatic_progress = !unattended && !has_flag(&args, "--seed") && !has_flag(&args, "--fresh");
    let mut sim = Sim::new(life.seed());
    let mut notice: Option<String> = None;
    let mut notice_left = 0.;
    if automatic_progress && life.options.load.is_none() {
        match snapshot::load_from_slot(&mut sim, &life.slots, AUTO_SLOT) {
            Ok(_) => {
                notice = Some("Welcome back. Your world is ready.".into());
                notice_left = 5.;
            }
            Err(vesper3d::viewer::devkit::SaveError::NotFound(_)) => {}
            Err(error) => {
                notice = Some(format!("Could not continue world: {error}"));
                notice_left = 8.;
            }
        }
    }
    life.load_flag_or_exit(&mut sim);
    let settings_path = beside_exe("settings.json");
    let mut settings = Settings::load(&settings_path);
    let mut sounds =
        SoundBank::start(life.options.silent(), settings.sfx_level(), settings.music_level(), audio::render).await;
    let mut shell = GameShell::new();
    let mut input = ClientInput::new();
    let mouse = MouseLook::default();
    let mut art = presentation::Presentation::new();
    let mut help = false;
    let mut controller_hints = false;
    let mut auto_time = 0.;
    let mut footsteps = 0.;
    let mut evidence = DeviceEvidence::default();
    let live_report = flag_value(&args, "--live-report").map(std::path::PathBuf::from);
    let keys = [
        KeyCode::Key1,
        KeyCode::Key2,
        KeyCode::Key3,
        KeyCode::Key4,
        KeyCode::Key5,
        KeyCode::Key6,
        KeyCode::Key7,
        KeyCode::Key8,
        KeyCode::Key9,
    ];

    loop {
        input.begin_frame_with_keyboard(&mut shell, true, unattended || platform::focused(), platform::keyboard());
        let dt = life.begin_frame(input.frame_seconds());
        sounds.poll().await;
        sounds.start_music();
        sounds.update_music(dt, &[if shell.paused { 0.35 } else { 1.0 }]);
        let pad = controls::gamepad_actions(input.gamepad(), sim.selected, shell.accepting_input());
        if !input.gamepad().neutral() {
            controller_hints = true;
        }
        notice_left -= dt;
        if notice_left <= 0. {
            notice = None;
        }
        let (held, edges, look) = if let Some(script) = life.script() {
            let mut edges = (u32::from(script.starts("jump")) * JUMP) | (u32::from(script.starts("pick")) * PICK);
            if script.starts("slot") {
                let slot = (script.value("slot", 0) as u32).clamp(1, 9) - 1;
                edges |= 1 << (SLOT_START + slot);
            }
            (
                Held {
                    forward: script.axis("fwd", "back"),
                    right: script.axis("right", "left"),
                    sprint: script.held("sprint"),
                    crouch: script.held("crouch"),
                    mine: script.held("mine"),
                    place: script.held("place"),
                },
                edges,
                [script.value("look", 0), script.value("look", 1)],
            )
        } else if shell.accepting_input() {
            let movement = input.movement(&shell);
            let px = if shell.playing() { mouse_pixels() } else { [0.; 2] };
            let mlook = mouse.look(px[0], px[1]);
            if px != [0.; 2] {
                controller_hints = false;
            }
            let stick = input.stick_look(&shell, dt);
            let look = [mlook[0] + stick[0], mlook[1] + stick[1]];
            let mut edges = (u32::from(movement.jump) * JUMP)
                | (u32::from(is_mouse_button_pressed(MouseButton::Middle) || pad.pick) * PICK);
            edges |= u32::from(is_mouse_button_pressed(MouseButton::Left) || pad.mine_pressed) * MINE_EDGE;
            edges |= u32::from(is_mouse_button_pressed(MouseButton::Right) || pad.place_pressed) * PLACE_EDGE;
            let mut selected = pad.slot;
            for (slot, key) in keys.iter().enumerate() {
                if input.pressed(*key) {
                    selected = Some(slot as u8);
                    controller_hints = false;
                }
            }
            let wheel = mouse_wheel().1;
            if wheel.abs() > 0.01 {
                controller_hints = false;
                selected = Some((i32::from(sim.selected) - wheel.signum() as i32).rem_euclid(9) as u8);
            }
            if let Some(slot) = selected {
                edges |= 1 << (SLOT_START + u32::from(slot));
                evidence.selection_presses += 1;
            }
            if input.pressed(KeyCode::E) || pad.guide {
                help = !help;
            }
            let held = Held {
                forward: movement.forward,
                right: movement.right,
                sprint: movement.sprint,
                crouch: movement.crouch || pad.crouch,
                mine: is_mouse_button_down(MouseButton::Left) || pad.mine,
                place: is_mouse_button_down(MouseButton::Right) || pad.place,
            };
            evidence.movement_frames += u64::from(held.forward != 0. || held.right != 0.);
            evidence.look_frames += u64::from(look != [0.; 2]);
            evidence.jump_presses += u64::from(movement.jump);
            evidence.mine_frames += u64::from(held.mine);
            evidence.place_frames += u64::from(held.place);
            (held, edges, look)
        } else {
            (Held::default(), 0, [0.; 2])
        };
        if shell.paused {
            life.reset_input();
        }
        life.feed(held, edges, look);
        let (save, load) =
            life.save_load_requested(input.pressed(KeyCode::F5) || pad.save, input.pressed(KeyCode::F9) || pad.load);
        if save {
            let result = life.quick_save(&sim, "BlueCraft world");
            evidence.successful_saves += u64::from(result.ok);
            notice = Some(format!("{}  {}", result.title, result.detail));
            notice_left = 5.;
        }
        if load {
            let result = life.quick_load(&mut sim);
            evidence.successful_loads += u64::from(result.ok);
            if result.ok {
                art.invalidate();
            }
            notice = Some(format!("{}  {}", result.title, result.detail));
            notice_left = 5.;
        }
        for _ in 0..life.ticks(dt, 1., !shell.paused) {
            let tick = life.take_tick();
            let slot = (0..9).find(|slot| tick.pressed(1 << (SLOT_START + slot))).map(|n| n as u8);
            let before = sim.player.position;
            sim.step(&Input {
                forward: tick.held.forward,
                right: tick.held.right,
                sprint: tick.held.sprint,
                crouch: tick.held.crouch,
                look: tick.look,
                jump: tick.pressed(JUMP),
                mine: tick.held.mine || tick.pressed(MINE_EDGE),
                place: tick.held.place || tick.pressed(PLACE_EDGE),
                pick: tick.pressed(PICK),
                slot,
            });
            if sim.player.is_grounded() {
                let delta = sim.player.position - before;
                footsteps += (delta.0 * delta.0 + delta.2 * delta.2).sqrt();
                if footsteps > 2.2 {
                    let p = sim.player.position;
                    let surface = sim.world.get(
                        p.0.floor() as i32,
                        sim.player.feet_height().floor() as i32 - 1,
                        p.2.floor() as i32,
                    );
                    let (cue, volume) = audio::footstep(surface);
                    sounds.play(cue, volume);
                    footsteps = 0.;
                }
            }
            for event in sim.drain_events() {
                let sound = if event == Event::Landed {
                    let p = sim.player.position;
                    Some(audio::landing(sim.world.get(
                        p.0.floor() as i32,
                        sim.player.feet_height().floor() as i32 - 1,
                        p.2.floor() as i32,
                    )))
                } else {
                    audio::event_sound(&event)
                };
                if let Some((cue, volume)) = sound {
                    sounds.play(cue, volume);
                }
                if event == Event::Respawned {
                    notice = Some("Back on solid ground.".into());
                    notice_left = 3.;
                }
            }
        }
        if let Some(path) = &live_report {
            // Optional read-only telemetry lets native device probes wait for fixed ticks.
            let state = serde_json::json!({"tick":sim.tick,"mined":sim.mined,"placed":sim.placed,
                "selected":sim.selected,"position":[sim.player.position.0,sim.player.position.1,sim.player.position.2],
                "yaw":sim.player.yaw,"pitch":sim.player.pitch,"paused":shell.paused});
            if let Err(error) = std::fs::write(path, state.to_string()) {
                eprintln!("Could not write live input evidence: {error}");
            }
        }
        if automatic_progress && !shell.paused {
            auto_time += dt;
            if auto_time >= 15. {
                auto_time = 0.;
                if let Err(error) = snapshot::save_to_slot(&sim, &life.slots, AUTO_SLOT, "BlueCraft automatic world") {
                    notice = Some(format!("World save failed: {error}"));
                    notice_left = 8.;
                }
            }
        }
        let pending = life.pending_look();
        let p = sim.player.position;
        let mut view = View::first_person(
            vec3(p.0, p.1, p.2),
            sim.player.yaw + pending[0],
            (sim.player.pitch - pending[1]).clamp(-1.5, 1.5),
        );
        view.fov = 70f32.to_radians();
        art.draw(&sim, &view, controller_hints, help, notice.as_deref(), shell.paused);
        let controls = if controller_hints {
            [
                "Left / right stick: move / look",
                "A: jump   L3: sprint   B: crouch",
                "RT: mine   LT: place   X / R3: pick",
                "LB / RB: previous / next material",
                "D-pad up / down: save / load checkpoint",
                "Y / View: guide   Start: pause",
                "Menus: stick / D-pad, A select, B back",
            ]
        } else {
            [
                "WASD: move   Mouse: look",
                "Space: jump   Shift: sprint",
                "Ctrl / C: crouch",
                "Left / right click: mine / place",
                "Wheel / 1-9: material   Middle click: pick",
                "F5 / F9: save / load checkpoint",
                "E: guide   Esc: pause   F: fullscreen",
            ]
        };
        let outcome = shell.local_menu_with_audio(
            &identity.title,
            &controls,
            AudioMenu { music_on: settings.music_on, sfx_on: settings.sfx_on, has_music: true },
        );
        if outcome.toggle_music {
            settings.toggle_music();
            sounds.music_volume = settings.music_level();
            settings.store(&settings_path);
        }
        if outcome.toggle_sfx {
            settings.toggle_sfx();
            sounds.sfx_volume = settings.sfx_level();
            settings.store(&settings_path);
        }
        if outcome.quit || game_client::exit_requested() {
            break;
        }
        if let Some(path) = life.capture_path() {
            life.captured(&path, kit::capture::save_frame(&path).map_err(|e| e.to_string()));
        }
        if life.end_frame(dt) {
            break;
        }
        next_frame().await;
    }
    if automatic_progress {
        if let Err(error) = snapshot::save_to_slot(&sim, &life.slots, AUTO_SLOT, "BlueCraft automatic world") {
            eprintln!("Could not save world: {error}");
        }
    }
    if let Some(report) = life.report() {
        println!("{report}");
    }
    let report = serde_json::json!({"game":"bluecraft", "engine": "1a5b696ee653179c6898b1c4195cb1d3d2715d6e",
        "tick":sim.tick,"mined":sim.mined,"placed":sim.placed,"selected":sim.selected,
        "position":[sim.player.position.0,sim.player.position.1,sim.player.position.2],
        "yaw":sim.player.yaw,"pitch":sim.player.pitch,
        "state_hash":format!("{:016x}",sim.state_hash()),"devices":evidence});
    println!("bluecraft: {report}");
    if let Some(path) = flag_value(&args, "--input-report") {
        if let Err(error) = std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()) {
            eprintln!("Could not write input evidence: {error}");
        }
    }
    if life.options.audible {
        let status = sounds.status();
        println!("audio: {status:?}");
        if let Err(error) = status.verify_playback(audio::LOADED_EFFECTS, audio::MUSIC_STEMS) {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
