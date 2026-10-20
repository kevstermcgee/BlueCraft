use bluecraft::{
    Event, Input, Sim, World, AIR, BEDROCK, BRICK, CHUNK_SIZE, GLASS, GRASS, HEIGHT, LEAVES, PALETTE, PLANKS, SAND,
    SIZE, STONE, WATER, WOOD,
};
use vesper3d::math::V;
use vesper3d::viewer::devkit::{Simulation, Snapshot};

fn site() -> Sim {
    let mut sim = Sim::new(3);
    sim.world = World::empty(3);
    for x in 5..18 {
        for z in 3..18 {
            sim.world.set(x, 5, z, STONE);
        }
    }
    sim.player.set_physics_state(V(10.5, 7.68, 10.5), 0., true);
    sim.player.yaw = 0.;
    sim.player.pitch = 0.;
    sim
}

#[test]
fn generated_world_has_nature_and_leaves_construction_to_the_player() {
    let sim = Sim::new(3);
    let mut found = std::collections::BTreeSet::new();
    for z in 0..SIZE {
        for x in 0..SIZE {
            for y in 0..HEIGHT {
                found.insert(sim.world.get(x, y, z));
            }
            assert_eq!(sim.world.get(x, 0, z), BEDROCK);
        }
    }
    for block in [GRASS, STONE, SAND, WOOD, LEAVES, WATER, BEDROCK] {
        assert!(found.contains(&block), "missing natural block kind {block}");
    }
    assert!(!found.contains(&PLANKS), "new worlds have no prebuilt cottage");
    assert!(!found.contains(&GLASS));
    assert!(!found.contains(&BRICK));
    assert!(sim.world.height_at(32, 44) as f32 <= sim.player.feet_height());
    for (x, z) in [(-1000, -1000), (-1, 64), (64, -1), (1000, 1000)] {
        assert_eq!(sim.world.get(x, 0, z), BEDROCK);
        assert!(sim.world.height_at(x, z) > 4, "terrain continues past the original map");
    }
}

#[test]
fn mining_changes_only_the_first_target_and_emits_the_block() {
    let mut sim = site();
    sim.world.set(10, 7, 7, WOOD);
    sim.world.set(10, 7, 6, STONE);
    let hit = sim.target().unwrap();
    assert_eq!((hit.cell, hit.normal, hit.block), ([10, 7, 7], [0, 0, 1], WOOD));
    sim.step(&Input { mine: true, ..Default::default() });
    assert_eq!(sim.world.get(10, 7, 7), AIR);
    assert_eq!(sim.world.get(10, 7, 6), STONE);
    assert_eq!((sim.mined, sim.revision), (1, 1));
    assert!(sim.drain_events().contains(&Event::Mined { cell: [10, 7, 7], block: WOOD }));
}

#[test]
fn held_mining_waits_ten_ticks_between_edits_and_survives_a_save() {
    let mut sim = site();
    for z in 5..8 {
        sim.world.set(10, 7, z, STONE);
    }
    let mine = Input { mine: true, ..Default::default() };
    sim.step(&mine);
    for _ in 0..9 {
        sim.step(&mine);
    }
    assert_eq!(sim.mined, 1);
    let state = sim.capture();
    let mut resumed = Sim::new(999);
    resumed.restore(state).unwrap();
    sim.step(&mine);
    resumed.step(&mine);
    assert_eq!(sim.mined, 2);
    assert_eq!(sim.state_hash(), resumed.state_hash());
}

#[test]
fn placing_uses_the_hit_face_selected_material_and_unlimited_palette() {
    let mut sim = site();
    sim.world.set(10, 7, 7, STONE);
    sim.step(&Input { place: true, slot: Some(7), ..Default::default() });
    assert_eq!(sim.selected, 7);
    assert_eq!(sim.world.get(10, 7, 8), BRICK);
    assert_eq!((sim.placed, sim.revision), (1, 1));
    assert!(sim.drain_events().contains(&Event::Placed { cell: [10, 7, 8], block: BRICK }));
    for _ in 0..10 {
        sim.step(&Input { place: true, ..Default::default() });
    }
    assert_eq!(sim.world.get(10, 7, 9), BRICK);
    assert_eq!(sim.placed, 2);
    for _ in 0..20 {
        sim.step(&Input { place: true, ..Default::default() });
    }
    assert_eq!(sim.placed, 2, "cannot fill the player's body");
}

#[test]
fn a_block_cannot_be_placed_inside_the_player() {
    let mut sim = site();
    sim.world.set(10, 7, 9, STONE);
    sim.step(&Input { place: true, ..Default::default() });
    assert_eq!(sim.world.get(10, 7, 10), AIR);
    assert_eq!(sim.placed, 0);
}

#[test]
fn middle_pick_selects_only_available_materials_and_bad_slots_are_ignored() {
    let mut sim = site();
    sim.world.set(10, 7, 7, GLASS);
    sim.step(&Input { pick: true, ..Default::default() });
    assert_eq!(PALETTE[sim.selected as usize], GLASS);
    sim.step(&Input { slot: Some(255), ..Default::default() });
    assert_eq!(PALETTE[sim.selected as usize], GLASS);
}

#[test]
fn bedrock_is_immutable_even_when_targeted_directly() {
    let mut sim = site();
    sim.world = World::empty(3);
    sim.player.set_physics_state(V(10.5, 2.68, 10.5), 0., true);
    sim.player.pitch = -1.5;
    assert_eq!(sim.target().unwrap().block, BEDROCK);
    for _ in 0..40 {
        sim.step(&Input { mine: true, ..Default::default() });
    }
    assert_eq!(sim.mined, 0);
    assert_eq!(sim.world.get(10, 0, 10), BEDROCK);
    assert!(!sim.world.set(10, 0, 10, AIR));
    assert!(!sim.world.set(10, HEIGHT, 10, STONE));
}

#[test]
fn engine_controller_stops_at_voxel_walls_and_can_jump_a_full_block() {
    let mut blocked = site();
    for x in 5..18 {
        for y in 6..10 {
            blocked.world.set(x, y, 8, STONE);
        }
    }
    for _ in 0..90 {
        blocked.step(&Input { forward: 1., sprint: true, ..Default::default() });
    }
    assert!(blocked.player.position.2 >= 9.22);
    assert!(blocked.player.position.2 < 9.3);
    let mut jumping = site();
    jumping.world.set(10, 6, 9, STONE);
    let start_feet = jumping.player.feet_height();
    let mut apex = start_feet;
    for t in 0..60 {
        jumping.step(&Input { forward: 1., jump: t == 0, ..Default::default() });
        apex = apex.max(jumping.player.feet_height());
    }
    assert!(apex > start_feet + 1.05, "jump must clear a one-metre cube");
    assert!(jumping.player.position.2 < 9., "jumping passes the block: {:?}", jumping.player.position);
    assert!(jumping.drain_events().contains(&Event::Jumped));
}

#[test]
fn sprint_is_faster_and_jumping_eventually_lands() {
    let mut walk = site();
    let mut sprint = site();
    for _ in 0..40 {
        walk.step(&Input { forward: 1., ..Default::default() });
        sprint.step(&Input { forward: 1., sprint: true, ..Default::default() });
    }
    assert!(sprint.player.position.2 < walk.player.position.2 - 1.);
    let mut sim = site();
    for t in 0..90 {
        sim.step(&Input { jump: t == 0, ..Default::default() });
    }
    assert!(sim.player.is_grounded());
    let events = sim.drain_events();
    assert_eq!(events.iter().filter(|e| **e == Event::Jumped).count(), 1);
    assert_eq!(events.iter().filter(|e| **e == Event::Landed).count(), 1);
}

#[test]
fn raycast_has_six_metre_reach_axis_normals_and_consistent_grid_boundaries() {
    let mut world = World::empty(0);
    world.set(10, 5, 4, STONE);
    let origin = V(10.5, 5.5, 11.);
    assert!(world.raycast(origin, V(0., 0., -1.), 6.).is_some());
    assert!(world.raycast(origin, V(0., 0., -1.), 5.999).is_none());
    world.set(10, 5, 10, WOOD);
    let hit = world.raycast(origin, V(0., 0., -1.), 6.).unwrap();
    assert_eq!(hit.cell, [10, 5, 10], "negative ray starts on the inside side of the plane");
    assert_eq!(hit.normal, [0, 0, 1], "a grid-plane hit retains its outward face normal");
    world.set(10, 5, 10, AIR);
    world.set(12, 5, 11, GLASS);
    assert_eq!(world.raycast(V(10.5, 5.5, 11.5), V(1., 0., 0.), 6.).unwrap().normal, [-1, 0, 0]);
    world.set(10, 8, 11, STONE);
    assert_eq!(world.raycast(V(10.5, 5.5, 11.5), V(0., 1., 0.), 6.).unwrap().normal, [0, -1, 0]);
    assert!(world.raycast(origin, V::ZERO, 6.).is_none());
    assert!(world.raycast(V(f32::NAN, 0., 0.), V(0., 0., -1.), 6.).is_none());
}

#[test]
fn water_is_decorative_and_does_not_hide_blocks_from_the_ray() {
    let mut world = World::empty(3);
    world.set(10, 7, 8, WATER);
    world.set(10, 7, 7, STONE);
    let hit = world.raycast(V(10.5, 7.5, 10.5), V(0., 0., -1.), 6.).unwrap();
    assert_eq!(hit.cell, [10, 7, 7]);
}

#[test]
fn invalid_axes_cannot_poison_movement() {
    let mut sim = site();
    sim.step(&Input { forward: f32::NAN, right: f32::INFINITY, look: [f32::NAN, 0.], ..Default::default() });
    assert!(sim.player.position.finite());
    for position in [V(f32::NAN, 0., 0.), V(f32::MAX, 0., 0.), V(0., 0., f32::NEG_INFINITY)] {
        sim.player.position = position;
        sim.step(&Input::default());
        assert!(sim.player.position.finite());
        assert!(sim.drain_events().contains(&Event::Respawned));
    }
}

#[test]
fn all_four_original_map_edges_can_be_crossed_without_walls_or_respawning() {
    for (start, yaw, axis, positive) in [
        (V(1., 2.68, 10.5), -std::f32::consts::FRAC_PI_2, 0, false),
        (V(63., 2.68, 10.5), std::f32::consts::FRAC_PI_2, 0, true),
        (V(10.5, 2.68, 1.), 0., 2, false),
        (V(10.5, 2.68, 63.), std::f32::consts::PI, 2, true),
    ] {
        let mut sim = Sim::new(3);
        sim.world = World::empty(3);
        sim.player.set_physics_state(start, 0., true);
        sim.player.yaw = yaw;
        for _ in 0..120 {
            sim.step(&Input { forward: 1., sprint: true, ..Default::default() });
        }
        let value = if axis == 0 { sim.player.position.0 } else { sim.player.position.2 };
        assert!(if positive { value > 70. } else { value < -6. }, "edge crossing stopped at {value}");
        assert!(sim.player.is_grounded(), "bedrock also continues beyond every former edge");
        assert!(!sim.drain_events().contains(&Event::Respawned));
    }
}

#[test]
fn negative_coordinate_edits_and_chunk_seams_are_addressed_independently() {
    let mut world = World::empty(4);
    for (n, x) in [-CHUNK_SIZE - 1, -CHUNK_SIZE, -1, 0, CHUNK_SIZE - 1, CHUNK_SIZE].into_iter().enumerate() {
        assert!(world.set(x, 12, -1, PALETTE[n]));
        assert_eq!(world.get(x, 12, -1), PALETTE[n]);
        assert_eq!(world.get(x, 12, 0), AIR);
        assert_eq!(world.chunk_revision(x.div_euclid(CHUNK_SIZE), -1), [1, 1, 2, 1, 2, 1][n]);
    }
    assert!(!world.set(-1, 0, -1, AIR));
    assert_eq!(world.get(-1, 0, -1), BEDROCK);
    assert_eq!(world.get(i32::MIN, 0, i32::MAX), BEDROCK);
}

#[test]
fn natural_tree_crowns_continue_across_positive_and_negative_chunk_seams() {
    let world = World::new(42);
    let mut crossed = [false; 4];
    for z in -80i32..80 {
        for x in -80i32..80 {
            let Some(y) = (1..HEIGHT).rev().find(|&y| world.get(x, y, z) == WOOD) else {
                continue;
            };
            for (axis, coordinate) in [(0, x), (1, z)] {
                let offset = match coordinate.rem_euclid(CHUNK_SIZE) {
                    0 | 1 => -2,
                    14 | 15 => 2,
                    _ => continue,
                };
                let (leaf_x, leaf_z) = if axis == 0 { (x + offset, z) } else { (x, z + offset) };
                assert!(
                    matches!(world.get(leaf_x, y, leaf_z), LEAVES | WOOD),
                    "a tree crown was cut off at chunk seam {leaf_x},{y},{leaf_z}"
                );
                crossed[axis * 2 + usize::from(coordinate >= 0)] = true;
            }
        }
    }
    assert!(crossed.into_iter().all(|crossed| crossed), "the probe must cover both signs of both axes");
}

#[test]
fn crouching_lowers_the_player_slows_walking_and_releases_smoothly() {
    let mut walk = site();
    let mut crouch = site();
    for _ in 0..25 {
        walk.step(&Input { right: 1., ..Default::default() });
        crouch.step(&Input { right: 1., crouch: true, ..Default::default() });
    }
    assert!(crouch.player.is_crouched());
    assert!(crouch.player.body_height() < walk.player.body_height() - 0.5);
    assert!(crouch.player.position.0 < walk.player.position.0 - 0.5);
    for _ in 0..25 {
        crouch.step(&Input::default());
    }
    assert!(!crouch.player.is_crouched());
}
