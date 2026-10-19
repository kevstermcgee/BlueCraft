use bluecraft::{Input, Sim, World, STONE};
use vesper3d::math::V;
use vesper3d::viewer::devkit::{
    assert_deterministic, snapshot, Playback, SaveError, SaveSlots, Simulation, Snapshot, QUICK_SLOT,
};

fn scripted() -> Vec<Input> {
    (0..360)
        .map(|t| Input {
            forward: if t % 160 < 100 { 1. } else { -0.4 },
            right: if (t / 90) % 2 == 0 { 0.3 } else { -0.3 },
            look: [if t % 120 < 30 { 0.012 } else { 0. }, 0.],
            jump: t % 75 == 20,
            sprint: t % 90 < 40,
            mine: t % 120 < 35,
            place: t % 120 >= 70,
            slot: (t % 30 == 0).then_some(((t / 30) % 9) as u8),
            ..Default::default()
        })
        .collect()
}

fn build_site() -> Sim {
    let mut sim = Sim::new(7);
    sim.world = World::empty(7);
    for x in 7..15 {
        for z in 4..15 {
            sim.world.set(x, 5, z, STONE);
        }
    }
    for z in 5..8 {
        sim.world.set(10, 7, z, STONE);
    }
    sim.player.set_physics_state(V(10.5, 7.68, 10.5), 0., true);
    sim.player.yaw = 0.;
    sim.player.pitch = 0.;
    sim
}

#[test]
fn same_seed_and_intentions_replay_identically() {
    assert_deterministic(|| Sim::new(7), &scripted());
}

#[test]
fn seeds_change_the_actual_terrain() {
    let a = Sim::new(1);
    let b = Sim::new(2);
    assert_ne!(a.world.data, b.world.data);
    assert_ne!(a.state_hash(), b.state_hash());
    assert_eq!(a.world.data, Sim::new(1).world.data);
}

#[test]
fn recorded_input_round_trips_through_engine_playback() {
    let inputs = scripted();
    let mut playback = Playback::<Input>::from_json(&serde_json::to_vec(&inputs).unwrap()).unwrap();
    assert_eq!(playback.len(), inputs.len());
    assert_eq!(playback.next_input(), inputs[0]);
}

#[test]
fn saves_resume_exactly_during_walking_jumping_and_edit_cooldowns() {
    snapshot::assert_resumes_as_promised(|| Sim::new(7), &scripted(), 45);
    let inputs: Vec<_> = (0..80)
        .map(|t| Input { mine: t < 27, place: t >= 27, slot: Some(7), jump: t == 34, ..Default::default() })
        .collect();
    snapshot::assert_resumes_as_promised(build_site, &inputs, 4);
}

#[test]
fn restored_edits_and_controller_momentum_continue_in_another_seed() {
    let mut source = build_site();
    for t in 0..17 {
        source.step(&Input { mine: true, right: 0.1, jump: t == 9, ..Default::default() });
    }
    assert!(source.mined > 0);
    let bytes = snapshot::save(&source, "build").unwrap();
    let mut restored = Sim::new(99);
    snapshot::restore(&mut restored, &bytes).unwrap();
    assert_eq!(source.capture(), restored.capture());
    for input in scripted().iter().take(100) {
        source.step(input);
        restored.step(input);
        assert_eq!(source.state_hash(), restored.state_hash());
    }
}

#[test]
fn malformed_and_nonfinite_states_are_rejected_atomically() {
    let mut sim = Sim::new(11);
    sim.step(&Input { jump: true, ..Default::default() });
    let state = sim.capture();
    let mut bad_states = Vec::new();
    let mut bad = state.clone();
    bad.world.data.pop();
    bad_states.push(bad);
    let mut bad = state.clone();
    bad.world.data[0] = 0;
    bad_states.push(bad);
    let mut bad = state.clone();
    bad.world.data[6000] = 250;
    bad_states.push(bad);
    let mut bad = state.clone();
    bad.selected = 9;
    bad_states.push(bad);
    let mut bad = state.clone();
    bad.action_cooldown = 11;
    bad_states.push(bad);
    let mut bad = state.clone();
    bad.revision = 1;
    bad_states.push(bad);
    let mut bad = state.clone();
    bad.player.pitch = f32::NAN;
    bad_states.push(bad);
    let mut bad = state.clone();
    bad.player.velocity.0 = f32::INFINITY;
    bad_states.push(bad);
    let mut bad = state.clone();
    bad.player.feet += 1.;
    bad_states.push(bad);
    let mut bad = state.clone();
    bad.player.body_height = 0.01;
    bad_states.push(bad);
    for bad in bad_states {
        assert!(sim.restore(bad).is_err());
        assert_eq!(sim.capture(), state, "refused direct restores cannot partially mutate the game");
    }
    let bytes = snapshot::save(&sim, "safe").unwrap();
    assert!(snapshot::restore(&mut sim, &bytes[..bytes.len() / 2]).is_err());
    let mut damaged = bytes.clone();
    let mid = damaged.len() / 2;
    damaged[mid] ^= 0x20;
    assert!(snapshot::restore(&mut sim, &damaged).is_err());
    assert_eq!(sim.capture(), state);
}

#[test]
fn quick_save_load_uses_the_engine_slot_and_restores_world_edits() {
    let dir = std::env::temp_dir().join(format!("bluecraft-slots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let slots = SaveSlots::new(&dir);
    let mut sim = build_site();
    assert!(matches!(snapshot::load_from_slot(&mut sim, &slots, QUICK_SLOT), Err(SaveError::NotFound(_))));
    sim.step(&Input { mine: true, ..Default::default() });
    snapshot::save_to_slot(&sim, &slots, QUICK_SLOT, "first block").unwrap();
    let saved = sim.state_hash();
    for _ in 0..20 {
        sim.step(&Input { place: true, ..Default::default() });
    }
    assert_ne!(saved, sim.state_hash());
    let (header, _) = snapshot::load_from_slot(&mut sim, &slots, QUICK_SLOT).unwrap();
    assert_eq!(header.label, "first block");
    assert_eq!(sim.state_hash(), saved);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn hash_covers_hidden_controller_momentum_and_action_timers() {
    let mut sim = Sim::new(1);
    let original = sim.state_hash();
    sim.player.apply_impulse(V(0.01, 0., 0.));
    assert_ne!(sim.state_hash(), original);
    let mut state = sim.capture();
    let before = sim.state_hash();
    state.action_cooldown = 1;
    sim.restore(state).unwrap();
    assert_ne!(before, sim.state_hash());
}
