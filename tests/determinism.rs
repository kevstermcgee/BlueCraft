use bluecraft::{Input, Sim, World, AIR, BEDROCK, BRICK, CHUNK_SIZE, GLASS, HEIGHT, PLANKS, SIZE, STONE};
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
            crouch: t % 160 >= 130,
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
    let terrain = |world: &World| (0..64).map(|x| world.height_at(x, -30)).collect::<Vec<_>>();
    assert_ne!(terrain(&a.world), terrain(&b.world));
    assert_ne!(a.state_hash(), b.state_hash());
    assert_eq!(terrain(&a.world), terrain(&Sim::new(1).world));
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
    for edits in [
        serde_json::json!({"0":{"0":0}}),
        serde_json::json!({"0":{"9000":3}}),
        serde_json::json!({"0":{"600":250}}),
        serde_json::json!({"0":{}}),
        serde_json::json!({"9223372036854775807":{"600":3}}),
    ] {
        let mut bad = serde_json::to_value(&state).unwrap();
        bad["world"]["edits"] = edits;
        bad_states.push(serde_json::from_value(bad).unwrap());
    }
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

#[test]
fn chunk_generation_is_independent_of_order_cache_eviction_and_negative_seams() {
    let mut a = Sim::new(42);
    let b = Sim::new(42);
    let original = a.state_hash();
    let untouched = a.capture();
    let probes = [-10001, -65, -64, -17, -16, -1, 0, 15, 16, 63, 64, 10001];
    let mut sample = Vec::new();
    for &z in &probes {
        for &x in &probes {
            for y in 0..HEIGHT {
                sample.push((x, y, z, a.world.get(x, y, z)));
            }
        }
    }
    for &(x, y, z, block) in sample.iter().rev() {
        assert_eq!(b.world.get(x, y, z), block, "generation order changed {x},{y},{z}");
    }
    for n in 0..200 {
        a.world.get(n * CHUNK_SIZE, 10, -10000);
    }
    assert!(a.world.cached_chunk_count() <= 128);
    a.world.retain_chunks_near(0, 0, 0);
    assert!(a.world.cached_chunk_count() <= 1);
    for &(x, y, z, block) in &sample {
        assert_eq!(a.world.get(x, y, z), block, "regenerating changed {x},{y},{z}");
    }
    assert_eq!(a.capture(), untouched, "generated caches must never become save authority");
    assert_eq!(a.state_hash(), original, "looking around must not change the world's hash");
    // Extreme accessor coordinates remain safe even though player travel is limited by f32 precision.
    for (x, z) in [(i32::MIN, i32::MIN), (i32::MAX, i32::MAX)] {
        assert_eq!(a.world.get(x, 0, z), BEDROCK);
        assert_ne!(a.world.get(x, 1, z), AIR);
    }
    a.step(&Input::default());
}

#[test]
fn distant_builds_and_mined_air_survive_unload_reload_and_exact_continuation() {
    let mut source = Sim::new(42);
    let positions = [(-1001, -1001), (-16, 65), (64, -1), (2049, 4097)];
    for &(x, z) in &positions {
        assert!(source.world.set(x, 20, z, BRICK));
        assert!(source.world.set(x, 1, z, AIR));
    }
    let state = source.capture();
    let hash = source.state_hash();
    source.world.retain_chunks_near(0, 0, 0);
    for &(x, z) in &positions {
        assert_eq!(source.world.get(x, 20, z), BRICK);
        assert_eq!(source.world.get(x, 1, z), AIR, "mined blocks must not regenerate");
    }
    assert_eq!(source.capture(), state);
    assert_eq!(source.state_hash(), hash);
    source.player.set_physics_state(V(-1000.5, 21.68, -1000.5), 0., true);
    let bytes = snapshot::save(&source, "distant build").unwrap();
    let mut loaded = Sim::new(7);
    snapshot::restore(&mut loaded, &bytes).unwrap();
    assert_eq!(source.capture(), loaded.capture());
    for input in scripted().iter().take(120) {
        source.step(input);
        loaded.step(input);
        assert_eq!(source.capture(), loaded.capture());
        assert_eq!(source.state_hash(), loaded.state_hash());
    }
}

#[test]
fn restoring_a_generated_block_removes_the_saved_edit_and_hash_change() {
    let mut sim = Sim::new(73);
    let before = sim.capture();
    let hash = sim.state_hash();
    let block = sim.world.get(-17, 3, 79);
    assert!(sim.world.set(-17, 3, 79, GLASS));
    assert_ne!(sim.state_hash(), hash);
    assert!(sim.world.set(-17, 3, 79, block));
    assert_eq!(sim.capture(), before);
    assert_eq!(sim.state_hash(), hash);
}

/// Writes the actual released v1 payload shape through the engine's framing/checksum API.
struct LegacyGame(serde_json::Value);
impl Simulation for LegacyGame {
    type Input = Input;
    fn step(&mut self, _: &Input) {}
    fn state_hash(&self) -> u64 {
        1234
    }
}
impl Snapshot for LegacyGame {
    const KIND: &'static str = "bluecraft";
    type State = serde_json::Value;
    fn capture(&self) -> Self::State {
        self.0.clone()
    }
    fn restore(&mut self, state: Self::State) -> Result<(), String> {
        self.0 = state;
        Ok(())
    }
}

fn legacy_game() -> (LegacyGame, Vec<u8>) {
    let current = build_site();
    let mut state = serde_json::to_value(current.capture()).unwrap();
    let mut cells = vec![AIR; (SIZE * SIZE * HEIGHT) as usize];
    cells[..(SIZE * SIZE) as usize].fill(BEDROCK);
    for z in 0..SIZE {
        for x in 0..SIZE {
            for y in 1..=5 {
                cells[((y * SIZE + z) * SIZE + x) as usize] = STONE;
            }
        }
    }
    // Frozen old cottage/build cells, plus a mined cell that must remain air.
    for (x, y, z, block) in [(40, 11, 33, PLANKS), (38, 13, 30, GLASS), (61, 6, 61, BRICK), (10, 5, 10, AIR)] {
        cells[((y * SIZE + z) * SIZE + x) as usize] = block;
    }
    state["world"] = serde_json::json!({"seed":7,"data":cells});
    (LegacyGame(state), cells)
}

#[test]
fn version_one_saves_preserve_the_entire_old_world_and_continue_into_new_terrain() {
    let (old, cells) = legacy_game();
    let saved_player = old.0["player"].clone();
    let bytes = snapshot::save(&old, "legacy cottage and builds").unwrap();
    let mut loaded = Sim::new(99);
    let header = snapshot::restore(&mut loaded, &bytes).unwrap();
    assert_eq!(header.version, 1);
    assert_eq!(serde_json::to_value(loaded.capture().player).unwrap(), saved_player);
    for y in 0..HEIGHT {
        for z in 0..SIZE {
            for x in 0..SIZE {
                assert_eq!(
                    loaded.world.get(x, y, z),
                    cells[((y * SIZE + z) * SIZE + x) as usize],
                    "legacy cell {x},{y},{z}"
                );
            }
        }
    }
    assert_eq!(loaded.world.get(40, 11, 33), PLANKS, "existing houses are preserved");
    assert_eq!(loaded.world.get(61, 6, 61), BRICK, "existing builds are preserved");
    assert_eq!(loaded.world.get(10, 5, 10), AIR, "mined air is preserved");
    for (x, z) in [(-1, -1), (64, 64), (1000, -1000)] {
        assert!(loaded.world.height_at(x, z) > 4);
    }
    let upgraded = snapshot::save(&loaded, "upgraded").unwrap();
    let mut again = Sim::new(17);
    assert_eq!(snapshot::restore(&mut again, &upgraded).unwrap().version, 2);
    assert_eq!(loaded.capture(), again.capture());
    for input in scripted().iter().take(90) {
        loaded.step(input);
        again.step(input);
        assert_eq!(loaded.state_hash(), again.state_hash());
    }
}

#[test]
fn invalid_legacy_saves_are_rejected_without_altering_the_current_world() {
    let (mut old, _) = legacy_game();
    let mut loaded = Sim::new(99);
    let before = loaded.capture();
    old.0["world"]["data"][0] = serde_json::json!(AIR);
    let bytes = snapshot::save(&old, "bad legacy bedrock").unwrap();
    assert!(snapshot::restore(&mut loaded, &bytes).is_err());
    assert_eq!(loaded.capture(), before);
}
