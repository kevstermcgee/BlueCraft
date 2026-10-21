//! Seeded, addressable chunks. Only player edits are save authority; generated caches are disposable.
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use vesper3d::viewer::devkit::{Rng, StateHasher};

use crate::{AIR, BEDROCK, DIRT, GRASS, HEIGHT, LEAVES, SAND, STONE, WATER, WOOD};

pub const CHUNK_SIZE: i32 = 16;
const CHUNK_CELLS: usize = (CHUNK_SIZE * CHUNK_SIZE * HEIGHT) as usize;
const CACHE_LIMIT: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Terrain {
    Empty,
    NaturalV1,
}

#[derive(Debug)]
struct CachedChunk {
    base: Vec<u8>,
    cells: Vec<u8>,
    used: u64,
}

#[derive(Debug, Default)]
struct ChunkCache {
    chunks: BTreeMap<i64, CachedChunk>,
    clock: u64,
    identity: Option<(u64, Terrain)>,
}

/// Infinite horizontal terrain with a fixed build height. Coordinates and noise are global,
/// so a chunk is identical regardless of its discovery order or its neighbours' cache state.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct World {
    pub seed: u64,
    terrain: Terrain,
    /// Packed signed chunk coordinates -> local y/z/x index -> replacement block.
    edits: BTreeMap<i64, BTreeMap<u16, u8>>,
    #[serde(skip)]
    cache: RefCell<ChunkCache>,
    #[serde(skip)]
    edit_hash: Cell<Option<u64>>,
    #[serde(skip)]
    revisions: BTreeMap<i64, u64>,
}

impl Clone for World {
    fn clone(&self) -> Self {
        Self {
            seed: self.seed,
            terrain: self.terrain,
            edits: self.edits.clone(),
            cache: RefCell::default(),
            edit_hash: Cell::new(self.edit_hash.get()),
            revisions: self.revisions.clone(),
        }
    }
}

impl PartialEq for World {
    fn eq(&self, other: &Self) -> bool {
        self.seed == other.seed && self.terrain == other.terrain && self.edits == other.edits
    }
}

fn key(cx: i32, cz: i32) -> i64 {
    ((cx as i64) << 32) | (cz as u32 as i64)
}

fn coordinates(key: i64) -> (i32, i32) {
    ((key >> 32) as i32, key as i32)
}

fn index(x: i32, y: i32, z: i32) -> usize {
    ((y * CHUNK_SIZE + z.rem_euclid(CHUNK_SIZE)) * CHUNK_SIZE + x.rem_euclid(CHUNK_SIZE)) as usize
}

impl World {
    fn with_terrain(seed: u64, terrain: Terrain) -> Self {
        Self {
            seed,
            terrain,
            edits: BTreeMap::new(),
            cache: RefCell::default(),
            edit_hash: Cell::new(Some(0)),
            revisions: BTreeMap::new(),
        }
    }

    /// A build/test world with an endless bedrock floor and air above it.
    pub fn empty(seed: u64) -> Self {
        Self::with_terrain(seed, Terrain::Empty)
    }

    /// A natural world contains terrain, water and trees, leaving construction to the player.
    pub fn new(seed: u64) -> Self {
        Self::with_terrain(seed, Terrain::NaturalV1)
    }

    /// Horizontal coordinates are unrestricted; only the build height is bounded.
    pub fn in_bounds(_x: i32, y: i32, _z: i32) -> bool {
        (0..HEIGHT).contains(&y)
    }

    fn ensure_chunk(&self, cache: &mut ChunkCache, cx: i32, cz: i32) -> i64 {
        if cache.identity != Some((self.seed, self.terrain)) {
            cache.chunks.clear();
            cache.identity = Some((self.seed, self.terrain));
        }
        cache.clock = cache.clock.wrapping_add(1);
        let key = key(cx, cz);
        if !cache.chunks.contains_key(&key) {
            if cache.chunks.len() >= CACHE_LIMIT {
                if let Some(oldest) = cache.chunks.iter().min_by_key(|(_, chunk)| chunk.used).map(|(&key, _)| key) {
                    cache.chunks.remove(&oldest);
                }
            }
            let base = self.generate_chunk(cx, cz);
            let mut cells = base.clone();
            if let Some(edits) = self.edits.get(&key) {
                for (&index, &block) in edits {
                    // Deserialized states are validated before entering the simulation.
                    if let Some(cell) = cells.get_mut(index as usize) {
                        *cell = block;
                    }
                }
            }
            cache.chunks.insert(key, CachedChunk { base, cells, used: cache.clock });
        }
        cache.chunks.get_mut(&key).unwrap().used = cache.clock;
        key
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        if !Self::in_bounds(x, y, z) {
            return AIR;
        }
        if y == 0 {
            return BEDROCK;
        }
        let mut cache = self.cache.borrow_mut();
        let key = self.ensure_chunk(&mut cache, x.div_euclid(CHUNK_SIZE), z.div_euclid(CHUNK_SIZE));
        cache.chunks[&key].cells[index(x, y, z)]
    }

    /// Edits survive cache eviction. Restoring a generated block removes its redundant saved edit.
    pub fn set(&mut self, x: i32, y: i32, z: i32, block: u8) -> bool {
        if y == 0 || !Self::in_bounds(x, y, z) || block > WATER {
            return false;
        }
        let mut cache = self.cache.borrow_mut();
        let key = self.ensure_chunk(&mut cache, x.div_euclid(CHUNK_SIZE), z.div_euclid(CHUNK_SIZE));
        let index = index(x, y, z);
        let chunk = cache.chunks.get_mut(&key).unwrap();
        if chunk.cells[index] == block {
            return false;
        }
        let old_edit = self.edits.get(&key).and_then(|edits| edits.get(&(index as u16))).copied();
        if block == chunk.base[index] {
            if let Some(edits) = self.edits.get_mut(&key) {
                edits.remove(&(index as u16));
                if edits.is_empty() {
                    self.edits.remove(&key);
                }
            }
        } else {
            self.edits.entry(key).or_default().insert(index as u16, block);
        }
        if let Some(mut hash) = self.edit_hash.get() {
            if let Some(old) = old_edit {
                hash ^= edit_fingerprint(key, index as u16, old);
            }
            if block != chunk.base[index] {
                hash ^= edit_fingerprint(key, index as u16, block);
            }
            self.edit_hash.set(Some(hash));
        }
        chunk.cells[index] = block;
        let revision = self.revisions.entry(key).or_default();
        *revision = revision.wrapping_add(1);
        true
    }

    /// A presentation invalidation counter; it is disposable along with generated meshes.
    pub fn chunk_revision(&self, cx: i32, cz: i32) -> u64 {
        self.revisions.get(&key(cx, cz)).copied().unwrap_or(0)
    }

    /// Drop distant generated data without dropping any edited blocks.
    pub fn retain_chunks_near(&self, cx: i32, cz: i32, radius: i32) {
        let radius = radius.max(0) as i64;
        self.cache.borrow_mut().chunks.retain(|&key, _| {
            let (x, z) = coordinates(key);
            (x as i64 - cx as i64).abs() <= radius && (z as i64 - cz as i64).abs() <= radius
        });
    }

    pub fn cached_chunk_count(&self) -> usize {
        self.cache.borrow().chunks.len()
    }

    /// Feet height on the highest solid block, ignoring decorative water.
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        (0..HEIGHT).rev().find(|&y| crate::is_solid(self.get(x, y, z))).map_or(0, |y| y + 1)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        for (&chunk_key, edits) in &self.edits {
            let (cx, cz) = coordinates(chunk_key);
            // Every valid i32 world coordinate maps into this inclusive chunk range.
            let range = i32::MIN.div_euclid(CHUNK_SIZE)..=i32::MAX.div_euclid(CHUNK_SIZE);
            if !range.contains(&cx) || !range.contains(&cz) || edits.is_empty() {
                return Err("the save has an invalid edited chunk".into());
            }
            if edits.iter().any(|(&index, &block)| {
                index as usize >= CHUNK_CELLS || index < (CHUNK_SIZE * CHUNK_SIZE) as u16 || block > WATER
            }) {
                return Err("the save has invalid voxel dimensions, blocks or bedrock".into());
            }
        }
        Ok(())
    }

    pub(crate) fn checksum(&self) -> u64 {
        let edits = self.edit_hash.get().unwrap_or_else(|| {
            let hash = self.edits.iter().fold(0, |hash, (&key, edits)| {
                edits.iter().fold(hash, |hash, (&index, &block)| hash ^ edit_fingerprint(key, index, block))
            });
            self.edit_hash.set(Some(hash));
            hash
        });
        StateHasher::new().u64(self.seed).u32(self.terrain as u32).u64(edits).finish()
    }

    fn generate_chunk(&self, cx: i32, cz: i32) -> Vec<u8> {
        let mut cells = vec![AIR; CHUNK_CELLS];
        cells[..(CHUNK_SIZE * CHUNK_SIZE) as usize].fill(BEDROCK);
        if self.terrain == Terrain::Empty {
            return cells;
        }
        let ox = cx as i64 * CHUNK_SIZE as i64;
        let oz = cz as i64 * CHUNK_SIZE as i64;
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let (top, shore) = terrain_column((ox + x as i64) as i32, (oz + z as i64) as i32, self.seed);
                for y in 1..=top {
                    cells[((y * CHUNK_SIZE + z) * CHUNK_SIZE + x) as usize] = if shore && y >= top - 2 {
                        SAND
                    } else if y == top {
                        GRASS
                    } else if y >= top - 2 {
                        DIRT
                    } else {
                        STONE
                    };
                }
                for y in top + 1..=8 {
                    cells[((y * CHUNK_SIZE + z) * CHUNK_SIZE + x) as usize] = WATER;
                }
            }
        }
        // Globally anchored trees include candidates just outside this chunk, so crowns cross
        // seams. Trunks are written first; tree overlap can never erase another tree's trunk.
        let mut trees = Vec::new();
        for gz in (oz - 2).div_euclid(8)..=(oz + CHUNK_SIZE as i64 + 1).div_euclid(8) {
            for gx in (ox - 2).div_euclid(8)..=(ox + CHUNK_SIZE as i64 + 1).div_euclid(8) {
                let mut rng = Rng::new(coordinate_seed(self.seed ^ 0x7472_6565, gx, gz));
                if rng.below(3) != 0 {
                    continue;
                }
                let x = gx * 8 + 1 + rng.below(6) as i64;
                let z = gz * 8 + 1 + rng.below(6) as i64;
                if x < i32::MIN as i64 || x > i32::MAX as i64 || z < i32::MIN as i64 || z > i32::MAX as i64 {
                    continue;
                }
                let (top, shore) = terrain_column(x as i32, z as i32, self.seed);
                let arrival = (x - 32).abs() < 9 && (z - 44).abs() < 9 && (x - 32).pow(2) + (z - 44).pow(2) < 81;
                if shore || top < 9 || arrival {
                    continue;
                }
                trees.push((x, z, top + 1, 4 + rng.below(2) as i32));
            }
        }
        let mut write = |x: i64, y: i32, z: i64, block: u8, only_air: bool| {
            if x >= ox
                && x < ox + CHUNK_SIZE as i64
                && z >= oz
                && z < oz + CHUNK_SIZE as i64
                && (1..HEIGHT).contains(&y)
            {
                let index = ((y as i64 * CHUNK_SIZE as i64 + z - oz) * CHUNK_SIZE as i64 + x - ox) as usize;
                if !only_air || cells[index] == AIR {
                    cells[index] = block;
                }
            }
        };
        for &(x, z, top, trunk) in &trees {
            for y in top..top + trunk {
                write(x, y, z, WOOD, false);
            }
        }
        for &(x, z, top, trunk) in &trees {
            for dy in -2i32..=1 {
                let radius: i64 = if dy == 1 { 1 } else { 2 };
                for dx in -radius..=radius {
                    for dz in -radius..=radius {
                        if dx.abs() != 2 || dz.abs() != 2 {
                            write(x + dx, top + trunk - 1 + dy, z + dz, LEAVES, true);
                        }
                    }
                }
            }
        }
        cells
    }
}

fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

fn edit_fingerprint(key: i64, index: u16, block: u8) -> u64 {
    mix(key as u64 ^ mix(((index as u64) << 8) | block as u64))
}

fn coordinate_seed(seed: u64, x: i64, z: i64) -> u64 {
    seed ^ (x as u64).wrapping_mul(0x9e3779b97f4a7c15) ^ (z as u64).wrapping_mul(0x517cc1b727220a95)
}

fn terrain_column(x: i32, z: i32, seed: u64) -> (i32, bool) {
    let mut rng = Rng::new(seed);
    let nx = x as f64 + rng.range(0., 1000.) as f64;
    let nz = z as f64 + rng.range(0., 1000.) as f64;
    let mut top = (9. + noise(nx / 19., nz / 19., seed) * 4. + noise(nx / 8., nz / 8., seed.wrapping_add(1)) * 1.8)
        .round() as i32;
    let meadow_distance = ((x as f64 - 32.).powi(2) + (z as f64 - 44.).powi(2)).sqrt();
    let meadow = ((15. - meadow_distance) / 7.).clamp(0., 1.);
    top = (top as f64 * (1. - meadow) + 10. * meadow).round() as i32;
    let pond = ((x as f64 - 20.) / 8.).powi(2) + ((z as f64 - 29.) / 10.).powi(2);
    if pond < 0.78 {
        top = 6 + (pond * 2.) as i32;
    } else if pond < 1.35 {
        top = 8;
    }
    (top.clamp(3, HEIGHT - 9), top <= 8)
}

fn noise(x: f64, z: f64, seed: u64) -> f64 {
    let ix = x.floor() as i64;
    let iz = z.floor() as i64;
    let fx = x - x.floor();
    let fz = z - z.floor();
    let sx = fx * fx * (3. - 2. * fx);
    let sz = fz * fz * (3. - 2. * fz);
    let value = |x, z| Rng::new(coordinate_seed(seed, x, z)).range(-1., 1.) as f64;
    let a = value(ix, iz) * (1. - sx) + value(ix + 1, iz) * sx;
    let b = value(ix, iz + 1) * (1. - sx) + value(ix + 1, iz + 1) * sx;
    a * (1. - sz) + b * sz
}
