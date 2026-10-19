//! BlueCraft's rendering-free, seeded voxel world and fixed 60 Hz game rules.
use serde::{Deserialize, Serialize};
use vesper3d::math::V;
use vesper3d::viewer::controller::{Collider, Controller, ControllerState, Movement};
use vesper3d::viewer::devkit::{path::yaw_facing, Rng, SavePolicy, Simulation, Snapshot, StateHasher, TICK};
use vesper3d::viewer::profile::ControllerProfile;

pub const SIZE: i32 = 64;
pub const HEIGHT: i32 = 32;
pub const AIR: u8 = 0;
pub const GRASS: u8 = 1;
pub const DIRT: u8 = 2;
pub const STONE: u8 = 3;
pub const SAND: u8 = 4;
pub const WOOD: u8 = 5;
pub const LEAVES: u8 = 6;
pub const PLANKS: u8 = 7;
pub const BRICK: u8 = 8;
pub const GLASS: u8 = 9;
pub const WATER: u8 = 10;
pub const BEDROCK: u8 = 11;
pub const PALETTE: [u8; 9] = [GRASS, DIRT, STONE, SAND, WOOD, LEAVES, PLANKS, BRICK, GLASS];
pub const REACH: f32 = 6.;
const ACTION_DELAY: u8 = 10;
const CELLS: usize = (SIZE * SIZE * HEIGHT) as usize;

pub fn block_name(block: u8) -> &'static str {
    match block {
        GRASS => "Grass",
        DIRT => "Dirt",
        STONE => "Stone",
        SAND => "Sand",
        WOOD => "Oak log",
        LEAVES => "Leaves",
        PLANKS => "Oak planks",
        BRICK => "Brick",
        GLASS => "Glass",
        WATER => "Water",
        BEDROCK => "Bedrock",
        _ => "Air",
    }
}

pub fn block_color(block: u8) -> [f32; 3] {
    match block {
        GRASS => [0.38, 0.66, 0.24],
        DIRT => [0.45, 0.29, 0.17],
        STONE => [0.52, 0.56, 0.58],
        SAND => [0.86, 0.78, 0.53],
        WOOD => [0.43, 0.30, 0.17],
        LEAVES => [0.26, 0.53, 0.20],
        PLANKS => [0.69, 0.49, 0.28],
        BRICK => [0.67, 0.30, 0.23],
        GLASS => [0.64, 0.87, 0.92],
        WATER => [0.24, 0.57, 0.78],
        BEDROCK => [0.24, 0.26, 0.29],
        _ => [0., 0., 0.],
    }
}

pub fn is_solid(block: u8) -> bool {
    block != AIR && block != WATER
}

/// The client sends intentions; all block edits and movement happen here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Input {
    pub forward: f32,
    pub right: f32,
    /// Radians in the engine convention: hand right turns right; hand down looks down.
    pub look: [f32; 2],
    pub jump: bool,
    pub sprint: bool,
    pub mine: bool,
    pub place: bool,
    pub pick: bool,
    /// Zero-based palette selection, supplied on a number-key or scroll edge.
    pub slot: Option<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Mined { cell: [i32; 3], block: u8 },
    Placed { cell: [i32; 3], block: u8 },
    Jumped,
    Landed,
    Respawned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub cell: [i32; 3],
    /// Outward normal of the hit face; zero only if the ray starts inside a block.
    pub normal: [i32; 3],
    pub block: u8,
}

/// Cells are one metre across, stored in y/z/x order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct World {
    pub seed: u64,
    pub data: Vec<u8>,
}

impl World {
    pub fn empty(seed: u64) -> Self {
        let mut world = Self { seed, data: vec![AIR; CELLS] };
        world.data[..(SIZE * SIZE) as usize].fill(BEDROCK);
        world
    }

    pub fn in_bounds(x: i32, y: i32, z: i32) -> bool {
        (0..SIZE).contains(&x) && (0..HEIGHT).contains(&y) && (0..SIZE).contains(&z)
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        if Self::in_bounds(x, y, z) {
            self.data[((y * SIZE + z) * SIZE + x) as usize]
        } else {
            AIR
        }
    }

    /// Bedrock and the finite world's bounds are immutable.
    pub fn set(&mut self, x: i32, y: i32, z: i32, block: u8) -> bool {
        if y == 0 || !Self::in_bounds(x, y, z) || block > WATER {
            return false;
        }
        let index = ((y * SIZE + z) * SIZE + x) as usize;
        let changed = self.data[index] != block;
        self.data[index] = block;
        changed
    }

    /// Feet height on the highest solid block, ignoring decorative water.
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        (0..HEIGHT).rev().find(|&y| is_solid(self.get(x, y, z))).map_or(0, |y| y + 1)
    }

    pub fn new(seed: u64) -> Self {
        let mut world = Self::empty(seed);
        let mut rng = Rng::new(seed);
        let ox = rng.range(0., 1000.);
        let oz = rng.range(0., 1000.);
        for z in 0..SIZE {
            for x in 0..SIZE {
                let nx = x as f32 + ox;
                let nz = z as f32 + oz;
                let mut top =
                    (9. + noise(nx / 19., nz / 19., seed) * 4. + noise(nx / 8., nz / 8., seed.wrapping_add(1)) * 1.8)
                        .round() as i32;
                // A broad, gently blended meadow connects the arrival and cottage. Abruptly
                // flattening only their footprints would leave a wall across the player's view.
                let meadow_distance = ((x as f32 - 35.).powi(2) + (z as f32 - 37.).powi(2)).sqrt();
                let meadow = ((20. - meadow_distance) / 10.).clamp(0., 1.);
                top = (top as f32 * (1. - meadow) + 10. * meadow).round() as i32;
                let pond = ((x as f32 - 20.) / 8.).powi(2) + ((z as f32 - 29.) / 10.).powi(2);
                let shore = pond < 1.35;
                if pond < 0.78 {
                    top = 6 + (pond * 2.) as i32;
                } else if shore {
                    top = 8;
                }
                // A clear arrival meadow and a level foundation for the starter cottage.
                if (29..=36).contains(&x) && (40..=48).contains(&z) || (37..=44).contains(&x) && (27..=34).contains(&z)
                {
                    top = 10;
                }
                for y in 1..=top {
                    let block = if y == top {
                        if shore {
                            SAND
                        } else {
                            GRASS
                        }
                    } else if y >= top - 2 {
                        if shore {
                            SAND
                        } else {
                            DIRT
                        }
                    } else {
                        STONE
                    };
                    world.set(x, y, z, block);
                }
                if pond < 0.78 {
                    for y in top + 1..=8 {
                        world.set(x, y, z, WATER);
                    }
                }
            }
        }
        // Fixed seeded candidates keep trees separated and preserve the arrival sight line.
        let mut trees: Vec<(i32, i32)> = Vec::new();
        for _ in 0..140 {
            let x = 4 + rng.below((SIZE - 8) as usize) as i32;
            let z = 4 + rng.below((SIZE - 8) as usize) as i32;
            let top = world.height_at(x, z);
            let protected = (27..=46).contains(&x) && (25..=50).contains(&z);
            if protected
                || world.get(x, top - 1, z) != GRASS
                || trees.iter().any(|(tx, tz)| (x - tx).pow(2) + (z - tz).pow(2) < 36)
            {
                continue;
            }
            let trunk = 4 + rng.below(2) as i32;
            for y in top..top + trunk {
                world.set(x, y, z, WOOD);
            }
            for dy in -2i32..=1 {
                let radius: i32 = if dy == 1 { 1 } else { 2 };
                for dx in -radius..=radius {
                    for dz in -radius..=radius {
                        if dx.abs() == 2 && dz.abs() == 2 {
                            continue;
                        }
                        let y = top + trunk - 1 + dy;
                        if world.get(x + dx, y, z + dz) == AIR {
                            world.set(x + dx, y, z + dz, LEAVES);
                        }
                    }
                }
            }
            trees.push((x, z));
            if trees.len() == 25 {
                break;
            }
        }
        world.cottage();
        world
    }

    fn cottage(&mut self) {
        for x in 38..=43 {
            for z in 28..=33 {
                self.set(x, 11, z, PLANKS);
                for y in 12..=14 {
                    let wall = x == 38 || x == 43 || z == 28 || z == 33;
                    let corner = (x == 38 || x == 43) && (z == 28 || z == 33);
                    let door = z == 33 && (x == 40 || x == 41) && y < 14;
                    let window =
                        y == 13 && ((x == 38 || x == 43) && (z == 30 || z == 31) || z == 28 && (x == 40 || x == 41));
                    self.set(
                        x,
                        y,
                        z,
                        if !wall || door {
                            AIR
                        } else if corner {
                            WOOD
                        } else if window {
                            GLASS
                        } else {
                            PLANKS
                        },
                    );
                }
            }
        }
        // Overhanging stepped roof and a friendly two-block porch.
        for x in 37..=44 {
            for z in 27..=34 {
                self.set(x, 15, z, WOOD);
            }
        }
        for x in 38..=43 {
            for z in 28..=33 {
                self.set(x, 16, z, PLANKS);
            }
        }
        for x in 39..=42 {
            for z in 29..=32 {
                self.set(x, 17, z, PLANKS);
            }
        }
        for x in 40..=41 {
            for z in 34..=35 {
                self.set(x, 11, z, PLANKS);
            }
        }
    }

    /// Grid DDA: the returned normal identifies the adjoining cell for placement.
    pub fn raycast(&self, origin: V, direction: V, reach: f32) -> Option<Hit> {
        if !origin.finite() || !direction.finite() || !reach.is_finite() || reach < 0. || direction.length() < 1e-6 {
            return None;
        }
        let direction = direction.norm();
        let o = [origin.0, origin.1, origin.2];
        let d = [direction.0, direction.1, direction.2];
        let mut cell = [0; 3];
        let mut step = [0; 3];
        let mut next = [f32::INFINITY; 3];
        let mut delta = [f32::INFINITY; 3];
        let mut normal = [0; 3];
        for axis in 0..3 {
            cell[axis] = o[axis].floor() as i32;
            if d[axis] < 0. && o[axis] == o[axis].floor() {
                cell[axis] -= 1;
            }
            if d[axis] != 0. {
                step[axis] = if d[axis] > 0. { 1 } else { -1 };
                if o[axis] == o[axis].floor() && normal == [0; 3] {
                    normal[axis] = -step[axis];
                }
                let edge = cell[axis] + if step[axis] > 0 { 1 } else { 0 };
                next[axis] = (edge as f32 - o[axis]) / d[axis];
                delta[axis] = 1. / d[axis].abs();
            }
        }
        for _ in 0..256 {
            let block = self.get(cell[0], cell[1], cell[2]);
            if block != AIR && block != WATER {
                return Some(Hit { cell, normal, block });
            }
            let axis = if next[0] <= next[1] && next[0] <= next[2] {
                0
            } else if next[1] <= next[2] {
                1
            } else {
                2
            };
            if next[axis] > reach {
                return None;
            }
            cell[axis] += step[axis];
            next[axis] += delta[axis];
            normal = [0; 3];
            normal[axis] = -step[axis];
        }
        None
    }

    fn colliders(&self, player: &Controller) -> Vec<Collider> {
        let p = player.position;
        let mut colliders = Vec::with_capacity(260);
        for x in p.0.floor() as i32 - 2..=p.0.floor() as i32 + 2 {
            for z in p.2.floor() as i32 - 2..=p.2.floor() as i32 + 2 {
                for y in (player.feet_height().floor() as i32 - 2).max(0)
                    ..=(player.feet_height().ceil() as i32 + 4).min(HEIGHT - 1)
                {
                    if is_solid(self.get(x, y, z)) {
                        colliders.push(block_collider([x, y, z]));
                    }
                }
            }
        }
        for (min, max) in [
            (V(-1., -8., -1.), V(0., 44., 65.)),
            (V(64., -8., -1.), V(65., 44., 65.)),
            (V(0., -8., -1.), V(64., 44., 0.)),
            (V(0., -8., 64.), V(64., 44., 65.)),
        ] {
            colliders.push(Collider { min, max });
        }
        colliders
    }
}

fn noise(x: f32, z: f32, seed: u64) -> f32 {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let fx = x - x.floor();
    let fz = z - z.floor();
    let sx = fx * fx * (3. - 2. * fx);
    let sz = fz * fz * (3. - 2. * fz);
    let value = |x: i32, z: i32| {
        let mut rng =
            Rng::new(seed ^ (x as u64).wrapping_mul(0x9e3779b97f4a7c15) ^ (z as u64).wrapping_mul(0x517cc1b727220a95));
        rng.range(-1., 1.)
    };
    let a = value(ix, iz) * (1. - sx) + value(ix + 1, iz) * sx;
    let b = value(ix, iz + 1) * (1. - sx) + value(ix + 1, iz + 1) * sx;
    a * (1. - sz) + b * sz
}

fn block_collider(cell: [i32; 3]) -> Collider {
    let min = V(cell[0] as f32, cell[1] as f32, cell[2] as f32);
    Collider { min, max: min + V(1., 1., 1.) }
}

fn profile() -> ControllerProfile {
    ControllerProfile { walk_speed: 4.3, sprint_speed: 7., jump_height: 1.2, ..Default::default() }
}

fn spawn(world: &World) -> Controller {
    let feet = V(32.5, world.height_at(32, 44) as f32, 44.5);
    let mut player = Controller::for_profile(profile(), feet, yaw_facing(feet, V(40.5, feet.1, 31.)))
        .expect("the BlueCraft profile is valid");
    player.set_floor(None);
    player.pitch = -0.30;
    player
}

pub struct Sim {
    pub tick: u64,
    pub player: Controller,
    /// Palette index, rather than a block ID.
    pub selected: u8,
    pub world: World,
    pub mined: u32,
    pub placed: u32,
    pub revision: u64,
    action_cooldown: u8,
    was_grounded: bool,
    events: Vec<Event>,
}

impl Sim {
    pub fn new(seed: u64) -> Self {
        let world = World::new(seed);
        let player = spawn(&world);
        Self {
            tick: 0,
            player,
            selected: 0,
            world,
            mined: 0,
            placed: 0,
            revision: 0,
            action_cooldown: 0,
            was_grounded: true,
            events: Vec::new(),
        }
    }

    pub fn target(&self) -> Option<Hit> {
        self.world.raycast(self.player.position, self.player.direction(), REACH)
    }

    pub fn step(&mut self, input: &Input) {
        self.tick = self.tick.saturating_add(1);
        self.action_cooldown = self.action_cooldown.saturating_sub(1);
        if let Some(slot) = input.slot.filter(|slot| (*slot as usize) < PALETTE.len()) {
            self.selected = slot;
        }
        self.player.look(input.look[0], input.look[1], 1., false);
        let axis = |n: f32| if n.is_finite() { n.clamp(-1., 1.) } else { 0. };
        let colliders = self.world.colliders(&self.player);
        self.player.update(
            Movement {
                forward: axis(input.forward),
                right: axis(input.right),
                sprint: input.sprint,
                jump: input.jump,
                ..Default::default()
            },
            TICK,
            &colliders,
        );
        let grounded = self.player.is_grounded();
        if input.jump && self.was_grounded && !grounded {
            self.events.push(Event::Jumped);
        }
        if grounded && !self.was_grounded {
            self.events.push(Event::Landed);
        }
        self.was_grounded = grounded;
        if !self.player.position.finite()
            || self.player.feet_height() < -4.
            || self.player.position.0 < -2.
            || self.player.position.0 > SIZE as f32 + 2.
            || self.player.position.2 < -2.
            || self.player.position.2 > SIZE as f32 + 2.
        {
            self.player = spawn(&self.world);
            self.was_grounded = true;
            self.events.push(Event::Respawned);
        }
        let Some(hit) = self.target() else {
            return;
        };
        if input.pick {
            if let Some(slot) = PALETTE.iter().position(|block| *block == hit.block) {
                self.selected = slot as u8;
            }
        }
        if self.action_cooldown != 0 {
            return;
        }
        if input.mine && hit.cell[1] != 0 && hit.block != BEDROCK {
            if self.world.set(hit.cell[0], hit.cell[1], hit.cell[2], AIR) {
                self.mined = self.mined.saturating_add(1);
                self.changed();
                self.events.push(Event::Mined { cell: hit.cell, block: hit.block });
            }
        } else if input.place && !input.mine && hit.normal != [0; 3] {
            let cell = [hit.cell[0] + hit.normal[0], hit.cell[1] + hit.normal[1], hit.cell[2] + hit.normal[2]];
            if !World::in_bounds(cell[0], cell[1], cell[2])
                || cell[1] == 0
                || is_solid(self.world.get(cell[0], cell[1], cell[2]))
            {
                return;
            }
            let bounds = block_collider(cell);
            if bounds.overlaps_body(
                self.player.position,
                self.player.feet_height(),
                self.player.body_height(),
                self.player.profile().radius,
            ) {
                return;
            }
            let block = PALETTE[self.selected as usize];
            if self.world.set(cell[0], cell[1], cell[2], block) {
                self.placed = self.placed.saturating_add(1);
                self.changed();
                self.events.push(Event::Placed { cell, block });
            }
        }
    }

    fn changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
        self.action_cooldown = ACTION_DELAY;
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
}

/// Every value that determines future rules or movement is saved. Events are disposable presentation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimState {
    pub tick: u64,
    pub player: ControllerState,
    pub selected: u8,
    pub world: World,
    pub mined: u32,
    pub placed: u32,
    pub revision: u64,
    pub action_cooldown: u8,
    pub was_grounded: bool,
}

fn hash_player(state: &ControllerState) -> u64 {
    let mut h = StateHasher::new();
    for v in [state.position, state.velocity, state.push] {
        h.f32(v.0).f32(v.1).f32(v.2);
    }
    h.f32(state.yaw)
        .f32(state.pitch)
        .f32(state.feet)
        .f32(state.body_height)
        .f32(state.vertical_velocity)
        .bool(state.grounded);
    h.finish()
}

impl Simulation for Sim {
    type Input = Input;
    fn step(&mut self, input: &Input) {
        Sim::step(self, input);
    }
    fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        for (_, part) in self.hash_parts() {
            h.u64(part);
        }
        h.finish()
    }
    fn hash_parts(&self) -> Vec<(&'static str, u64)> {
        let mut rules = StateHasher::new();
        rules
            .u64(self.tick)
            .u32(self.selected as u32)
            .u32(self.mined)
            .u32(self.placed)
            .u64(self.revision)
            .u32(self.action_cooldown as u32)
            .bool(self.was_grounded);
        let mut world = StateHasher::new();
        world.u64(self.world.seed).bytes(&self.world.data);
        vec![
            ("rules", rules.finish()),
            ("player", hash_player(&self.player.network_state())),
            ("world", world.finish()),
        ]
    }
}

impl Snapshot for Sim {
    const KIND: &'static str = "bluecraft";
    const POLICY: SavePolicy = SavePolicy::Exact;
    type State = SimState;
    fn capture(&self) -> SimState {
        SimState {
            tick: self.tick,
            player: self.player.network_state(),
            selected: self.selected,
            world: self.world.clone(),
            mined: self.mined,
            placed: self.placed,
            revision: self.revision,
            action_cooldown: self.action_cooldown,
            was_grounded: self.was_grounded,
        }
    }
    fn restore(&mut self, state: SimState) -> Result<(), String> {
        if state.world.data.len() != CELLS
            || state.world.data.iter().any(|&block| block > BEDROCK)
            || state.world.data[..(SIZE * SIZE) as usize].iter().any(|&block| block != BEDROCK)
            || state.world.data[(SIZE * SIZE) as usize..].contains(&BEDROCK)
        {
            return Err("the save has invalid voxel dimensions, blocks or bedrock".into());
        }
        if state.selected as usize >= PALETTE.len()
            || state.action_cooldown > ACTION_DELAY
            || state.revision != u64::from(state.mined) + u64::from(state.placed)
        {
            return Err("the save has invalid build counters or palette selection".into());
        }
        let p = state.player;
        if !p.position.finite()
            || !p.velocity.finite()
            || !p.push.finite()
            || ![p.yaw, p.pitch, p.feet, p.body_height, p.vertical_velocity].iter().all(|v| v.is_finite())
            || !(-1.5..=1.5).contains(&p.pitch)
            || p.yaw.abs() > 1_000_000.
            || !(-2.0..=SIZE as f32 + 2.).contains(&p.position.0)
            || !(-2.0..=SIZE as f32 + 2.).contains(&p.position.2)
            || !(-4.0..=HEIGHT as f32 + 5.).contains(&p.feet)
            || (p.body_height - profile().height).abs() > 1e-5
            || (p.position.1 - p.feet - profile().eye_height).abs() > 1e-4
            || p.velocity.length() > 30.
            || p.push.length() > 30.
            || p.vertical_velocity.abs() > 40.
        {
            return Err("the save has an invalid player pose or movement state".into());
        }
        // Reconstruct fixed configuration so a load cannot inherit the loader's controller settings.
        let mut player = Controller::for_profile(profile(), V(p.position.0, p.feet, p.position.2), p.yaw)
            .map_err(|error| error.to_string())?;
        player.set_floor(None);
        player.restore_network_state(&p);
        self.tick = state.tick;
        self.player = player;
        self.selected = state.selected;
        self.world = state.world;
        self.mined = state.mined;
        self.placed = state.placed;
        self.revision = state.revision;
        self.action_cooldown = state.action_cooldown;
        self.was_grounded = state.was_grounded;
        self.events.clear();
        Ok(())
    }
    fn save_tick(&self) -> u64 {
        self.tick
    }
}
