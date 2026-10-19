//! Client-only voxel drawing. The simulation owns every editable block.
use bluecraft::{
    block_name, Sim, World, AIR, BEDROCK, BRICK, DIRT, GLASS, GRASS, HEIGHT, LEAVES, PALETTE, PLANKS, SAND, SIZE,
    STONE, WATER, WOOD,
};
use macroquad::miniquad::{Comparison, PipelineParams, UniformDesc, UniformType};
use macroquad::prelude::*;
use vesper3d::viewer::kit::{hud, View};

const TILE: u32 = 16;
const ATLAS: u32 = 64;
const FACE_LIMIT: usize = 780;
const SKY: Color = Color::new(0.66, 0.82, 0.92, 1.);
const INK: Color = Color::new(0.055, 0.10, 0.12, 0.86);
const CREAM: Color = Color::new(0.96, 0.98, 0.94, 1.);
const MINT: Color = Color::new(0.70, 0.91, 0.67, 1.);

// Every face is counterclockwise when seen from outside its block. These bases also drive AO.
const NORMALS: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];
const CORNERS: [[[i32; 3]; 4]; 6] = [
    [[1, 0, 1], [1, 0, 0], [1, 1, 0], [1, 1, 1]],
    [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]],
    [[0, 1, 1], [1, 1, 1], [1, 1, 0], [0, 1, 0]],
    [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]],
    [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
    [[1, 0, 0], [0, 0, 0], [0, 1, 0], [1, 1, 0]],
];
const SHADES: [f32; 6] = [0.84, 0.68, 1.0, 0.54, 0.77, 0.89];

const VERTEX_SHADER: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
uniform mat4 Model;
uniform mat4 Projection;
varying lowp vec4 tint;
varying mediump vec2 uv;
varying highp vec3 world;
void main() {
    vec4 p = Model * vec4(position, 1.0);
    gl_Position = Projection * p;
    world = p.xyz;
    tint = color0 / 255.0;
    uv = texcoord;
}
"#;
const FRAGMENT_SHADER: &str = r#"#version 100
precision highp float;
varying lowp vec4 tint;
varying mediump vec2 uv;
varying highp vec3 world;
uniform sampler2D Texture;
uniform vec3 Eye;
uniform vec3 Fog;
void main() {
    vec4 texel = texture2D(Texture, uv);
    if (texel.a < 0.1) discard;
    vec3 color = texel.rgb * tint.rgb;
    float distanceFog = smoothstep(30.0, 105.0, length(world - Eye)) * 0.86;
    gl_FragColor = vec4(mix(color, Fog, distanceFog), 1.0);
}
"#;

pub struct Presentation {
    atlas: Texture2D,
    material: Material,
    meshes: Vec<Mesh>,
    clouds: Vec<Mesh>,
    revision: Option<u64>,
}

impl Presentation {
    pub fn new() -> Self {
        let atlas = make_atlas();
        let material = load_material(
            ShaderSource::Glsl { vertex: VERTEX_SHADER, fragment: FRAGMENT_SHADER },
            MaterialParams {
                pipeline_params: PipelineParams {
                    depth_test: Comparison::LessOrEqual,
                    depth_write: true,
                    ..Default::default()
                },
                uniforms: vec![
                    UniformDesc::new("Eye", UniformType::Float3),
                    UniformDesc::new("Fog", UniformType::Float3),
                ],
                ..Default::default()
            },
        )
        .expect("BlueCraft's voxel material must compile");
        let clouds = cloud_meshes(&atlas);
        Self { atlas, material, meshes: Vec::new(), clouds, revision: None }
    }

    pub fn invalidate(&mut self) {
        self.revision = None;
    }

    pub fn draw(&mut self, sim: &Sim, view: &View, _dt: f32, help: bool, notice: Option<&str>, paused: bool) {
        if self.revision != Some(sim.revision) {
            self.meshes = world_meshes(&sim.world, &self.atlas);
            self.revision = Some(sim.revision);
        }
        set_default_camera();
        draw_sky(view);
        set_camera(&view.camera_checked(0.08, 150.));
        self.material.set_uniform("Eye", view.eye);
        self.material.set_uniform("Fog", vec3(SKY.r, SKY.g, SKY.b));
        gl_use_material(&self.material);
        for mesh in &self.clouds {
            draw_mesh(mesh);
        }
        for mesh in &self.meshes {
            draw_mesh(mesh);
        }
        gl_use_default_material();
        if !paused {
            if let Some(hit) = sim.target() {
                target_outline(hit.cell);
            }
        }
        set_default_camera();
        self.hud(sim, view, help && !paused, notice, paused);
    }

    fn hud(&self, sim: &Sim, view: &View, help: bool, notice: Option<&str>, paused: bool) {
        let s = hud::ui_scale();
        let w = screen_width();
        let h = screen_height();
        let left = 24. * s;
        // Two little cubes make the title readable without taking over the landscape.
        draw_rectangle(left, 25. * s, 17. * s, 17. * s, Color::new(0.27, 0.61, 0.71, 1.));
        draw_rectangle(left + 7. * s, 19. * s, 17. * s, 17. * s, MINT);
        hud::text_outlined("BlueCraft", left + 34. * s, 43. * s, 28. * s, CREAM);
        panel(left, 55. * s, 85. * s, 22. * s, s);
        hud::text_centered("CREATIVE", left + 42.5 * s, 70. * s, 12. * s, MINT);
        hud::text_outlined(&format!("SEED {}", sim.world.seed), left + 98. * s, 70. * s, 12. * s, CREAM);
        hud::text_right(
            &format!("{:.0}  /  {:.0}  /  {:.0}", view.eye.x, view.eye.y, view.eye.z),
            w - left,
            35. * s,
            16. * s,
            CREAM,
        );
        hud::text_right("X      Y      Z", w - left, 54. * s, 10. * s, Color::new(0.9, 0.97, 1., 0.8));

        let selected = (sim.selected as usize).min(PALETTE.len() - 1);
        let slot = 54. * s;
        let gap = 4. * s;
        let bar_w = PALETTE.len() as f32 * (slot + gap) - gap;
        let bar_x = (w - bar_w) * 0.5;
        let bar_y = h - 84. * s;
        panel(bar_x - 8. * s, bar_y - 8. * s, bar_w + 16. * s, slot + 16. * s, s);
        for (index, block) in PALETTE.iter().copied().enumerate() {
            let x = bar_x + index as f32 * (slot + gap);
            let active = index == selected;
            draw_rectangle(
                x,
                bar_y,
                slot,
                slot,
                if active { Color::new(0.81, 0.92, 0.76, 0.20) } else { Color::new(0.01, 0.04, 0.055, 0.38) },
            );
            if active {
                draw_rectangle_lines(x - s, bar_y - s, slot + 2. * s, slot + 2. * s, 2. * s, MINT);
                draw_rectangle(x + 12. * s, bar_y + slot + 4. * s, slot - 24. * s, 2. * s, MINT);
            }
            block_icon(&self.atlas, block, x + slot * 0.5, bar_y + 29. * s, 14. * s);
            hud::text_outlined(
                &(index + 1).to_string(),
                x + 5. * s,
                bar_y + 13. * s,
                11. * s,
                if active { MINT } else { Color::new(0.8, 0.88, 0.87, 0.85) },
            );
        }
        hud::text_centered(block_name(PALETTE[selected]), w * 0.5, bar_y - 20. * s, 20. * s, CREAM);
        hud::text_centered(
            "LMB  Mine     RMB  Place     1-9 / Scroll  Blocks     E  Guide     Esc  Menu",
            w * 0.5,
            h - 12. * s,
            12. * s,
            CREAM,
        );
        if !paused {
            crosshair(s);
            if let Some(hit) = sim.target() {
                hud::text_centered(
                    block_name(hit.block),
                    w * 0.5,
                    h * 0.5 + 31. * s,
                    12. * s,
                    Color::new(0.96, 0.99, 0.94, 0.85),
                );
            }
        }
        if let Some(message) = notice {
            let pw = hud::width_of(message, 17. * s) + 36. * s;
            panel((w - pw) * 0.5, 90. * s, pw, 37. * s, s);
            hud::text_centered(message, w * 0.5, 114. * s, 17. * s, CREAM);
        }
        if help {
            let x = left;
            let y = 103. * s;
            let pw = 294. * s;
            panel(x, y, pw, 255. * s, s);
            draw_rectangle(x, y, 3. * s, 255. * s, MINT);
            hud::text_outlined("MAKE SOMETHING YOURS", x + 18. * s, y + 30. * s, 16. * s, MINT);
            let rows = [
                ("W A S D", "Move"),
                ("MOUSE", "Look around"),
                ("SPACE", "Jump"),
                ("SHIFT", "Sprint"),
                ("LEFT / RIGHT CLICK", "Mine / place"),
                ("MIDDLE CLICK", "Pick a block"),
                ("1-9 / SCROLL", "Choose material"),
                ("F5 / F9", "Save / load world"),
            ];
            for (i, (key, action)) in rows.iter().enumerate() {
                let row = y + (58. + i as f32 * 22.) * s;
                hud::text_outlined(key, x + 18. * s, row, 11. * s, Color::new(0.68, 0.80, 0.81, 1.));
                hud::text_right(action, x + pw - 18. * s, row, 13. * s, CREAM);
            }
            hud::text_outlined("Unlimited blocks. Endless possibilities.", x + 18. * s, y + 239. * s, 11. * s, MINT);
        }
    }
}

fn panel(x: f32, y: f32, w: f32, h: f32, s: f32) {
    draw_rectangle(x + 3. * s, y + 4. * s, w, h, Color::new(0., 0.025, 0.035, 0.16));
    draw_rectangle(x, y, w, h, INK);
    draw_rectangle_lines(x, y, w, h, s, Color::new(0.76, 0.91, 0.89, 0.17));
}

fn crosshair(s: f32) {
    let p = vec2(screen_width() * 0.5, screen_height() * 0.5);
    for (color, thickness) in [(Color::new(0.02, 0.09, 0.10, 0.65), 4. * s), (CREAM, 1.5 * s)] {
        draw_line(p.x - 7. * s, p.y, p.x - 2. * s, p.y, thickness, color);
        draw_line(p.x + 2. * s, p.y, p.x + 7. * s, p.y, thickness, color);
        draw_line(p.x, p.y - 7. * s, p.x, p.y - 2. * s, thickness, color);
        draw_line(p.x, p.y + 2. * s, p.x, p.y + 7. * s, thickness, color);
    }
}

fn target_outline(cell: [i32; 3]) {
    let p = vec3(cell[0] as f32, cell[1] as f32, cell[2] as f32) - Vec3::splat(0.003);
    let d = 1.006;
    let c = Color::new(0.12, 0.20, 0.16, 0.9);
    for a in 0..3 {
        let b = (a + 1) % 3;
        let c_axis = (a + 2) % 3;
        for u in [0., d] {
            for v in [0., d] {
                let mut begin = p;
                begin[b] += u;
                begin[c_axis] += v;
                let mut end = begin;
                end[a] += d;
                draw_line_3d(begin, end, c);
            }
        }
    }
}

fn draw_sky(view: &View) {
    clear_background(SKY);
    let w = screen_width();
    let h = screen_height();
    let horizon = (h * (0.53 + view.pitch * 0.62)).clamp(1., h * 1.8);
    for i in 0..32 {
        let y = h * i as f32 / 32.;
        let t = (y / horizon).clamp(0., 1.);
        draw_rectangle(0., y, w, h / 32. + 1., Color::new(0.37 + 0.29 * t, 0.65 + 0.17 * t, 0.87 + 0.05 * t, 1.));
    }
    let sun_dir = vec3(-0.48, 0.76, -0.42).normalize();
    if let Some(p) = view.project(view.eye + sun_dir * 100., w, h) {
        let r = h * 0.035;
        draw_rectangle(p.x - r * 1.5, p.y - r * 1.5, r * 3., r * 3., Color::new(1., 0.96, 0.73, 0.07));
        draw_rectangle(p.x - r, p.y - r, r * 2., r * 2., Color::new(1., 0.98, 0.83, 1.));
    }
}

fn tile_for(block: u8, face: usize) -> u32 {
    match block {
        GRASS => {
            if face == 2 {
                0
            } else if face == 3 {
                2
            } else {
                1
            }
        }
        DIRT => 2,
        STONE => 3,
        SAND => 4,
        WOOD => {
            if face == 2 || face == 3 {
                6
            } else {
                5
            }
        }
        LEAVES => 7,
        PLANKS => 8,
        BRICK => 9,
        GLASS => 10,
        WATER => 11,
        BEDROCK => 12,
        _ => 3,
    }
}

fn tile_uv(tile: u32, u: f32, v: f32) -> Vec2 {
    vec2(
        ((tile % 4) * TILE) as f32 + 0.1 + u * (TILE as f32 - 0.2),
        ((tile / 4) * TILE) as f32 + 0.1 + v * (TILE as f32 - 0.2),
    ) / ATLAS as f32
}

fn blank_mesh(atlas: &Texture2D) -> Mesh {
    Mesh {
        vertices: Vec::with_capacity(FACE_LIMIT * 4),
        indices: Vec::with_capacity(FACE_LIMIT * 6),
        texture: Some(atlas.clone()),
    }
}

fn flush(mesh: &mut Mesh, meshes: &mut Vec<Mesh>, atlas: &Texture2D) {
    if !mesh.indices.is_empty() {
        meshes.push(std::mem::replace(mesh, blank_mesh(atlas)));
    }
}

fn face(mesh: &mut Mesh, points: [Vec3; 4], tile: u32, light: [f32; 4]) {
    let base = mesh.vertices.len() as u16;
    let coords = [(0., 1.), (1., 1.), (1., 0.), (0., 0.)];
    for i in 0..4 {
        mesh.vertices.push(Vertex::new2(
            points[i],
            tile_uv(tile, coords[i].0, coords[i].1),
            Color::new(light[i], light[i], light[i], 1.),
        ));
    }
    // Choose the diagonal that keeps ambient occlusion smooth across a corner.
    if light[0] + light[2] > light[1] + light[3] {
        mesh.indices.extend([base, base + 1, base + 3, base + 1, base + 2, base + 3]);
    } else {
        mesh.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn occludes(block: u8) -> bool {
    block != AIR && block != WATER && block != GLASS
}

fn corner_ao(world: &World, cell: [i32; 3], face_index: usize, corner: [i32; 3]) -> f32 {
    let n = NORMALS[face_index];
    let mut p = [cell[0] + n[0], cell[1] + n[1], cell[2] + n[2]];
    let (a, b) = if n[0] != 0 {
        (1, 2)
    } else if n[1] != 0 {
        (0, 2)
    } else {
        (0, 1)
    };
    let da = if corner[a] == 0 { -1 } else { 1 };
    let db = if corner[b] == 0 { -1 } else { 1 };
    p[a] += da;
    let side_a = occludes(world.get(p[0], p[1], p[2]));
    p[a] -= da;
    p[b] += db;
    let side_b = occludes(world.get(p[0], p[1], p[2]));
    p[a] += da;
    let diagonal = occludes(world.get(p[0], p[1], p[2]));
    if side_a && side_b {
        0.65
    } else {
        1. - (side_a as u8 + side_b as u8 + diagonal as u8) as f32 * 0.105
    }
}

fn world_meshes(world: &World, atlas: &Texture2D) -> Vec<Mesh> {
    let mut meshes = Vec::new();
    let mut mesh = blank_mesh(atlas);
    for y in 0..HEIGHT {
        for z in 0..SIZE {
            for x in 0..SIZE {
                let block = world.get(x, y, z);
                if block == AIR {
                    continue;
                }
                let cell = [x, y, z];
                let origin = vec3(x as f32, y as f32, z as f32);
                for (f, n) in NORMALS.iter().enumerate() {
                    let neighbor = world.get(x + n[0], y + n[1], z + n[2]);
                    if neighbor != AIR
                        && !(neighbor == GLASS && block != GLASS)
                        && !(neighbor == WATER && block != WATER)
                    {
                        continue;
                    }
                    let points = CORNERS[f].map(|p| origin + vec3(p[0] as f32, p[1] as f32, p[2] as f32));
                    let lights =
                        CORNERS[f].map(|c| SHADES[f] * if block == WATER { 1. } else { corner_ao(world, cell, f, c) });
                    face(&mut mesh, points, tile_for(block, f), lights);
                    if mesh.indices.len() >= FACE_LIMIT * 6 {
                        flush(&mut mesh, &mut meshes, atlas);
                    }
                }
            }
        }
    }
    flush(&mut mesh, &mut meshes, atlas);
    meshes
}

fn cloud_meshes(atlas: &Texture2D) -> Vec<Mesh> {
    let mut mesh = blank_mesh(atlas);
    for z in 0..4 {
        for x in 0..5 {
            let shift = ((x * 13 + z * 7) % 9) as f32;
            let p = vec3(x as f32 * 24. - 24. + shift, 39. + (z % 2) as f32 * 3., z as f32 * 29. - 25.);
            for (offset, size) in [(Vec3::ZERO, vec3(12., 1.6, 5.)), (vec3(3., 0.3, -3.), vec3(7., 1.3, 10.))] {
                for (f, corners) in CORNERS.iter().enumerate() {
                    let points = corners.map(|c| p + offset + vec3(c[0] as f32, c[1] as f32, c[2] as f32) * size);
                    face(&mut mesh, points, 13, [if f == 3 { 0.83 } else { 1. }; 4]);
                }
            }
        }
    }
    vec![mesh]
}

fn hash(x: u32, y: u32, tile: u32) -> u32 {
    let mut n = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ tile.wrapping_mul(1_274_126_177);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    n ^ (n >> 16)
}

fn make_atlas() -> Texture2D {
    let mut image = Image::gen_image_color(ATLAS as u16, ATLAS as u16, WHITE);
    for tile in 0..16 {
        for y in 0..TILE {
            for x in 0..TILE {
                let h = hash(x, y, tile);
                let n = ((h % 19) as f32 - 9.) / 255.;
                let (mut r, mut g, mut b, a) = match tile {
                    0 => (0.43, 0.63, 0.23, 1.),
                    1 => {
                        if y < 3 + hash(x, 0, 1) % 3 {
                            (0.40, 0.59, 0.22, 1.)
                        } else {
                            (0.48, 0.34, 0.22, 1.)
                        }
                    }
                    2 => (0.48, 0.34, 0.22, 1.),
                    3 => (0.53, 0.55, 0.54, 1.),
                    4 => (0.83, 0.78, 0.54, 1.),
                    5 => {
                        let stripe = (hash(x / 2, y / 6, 5) % 7) as f32 * 0.017;
                        (0.40 + stripe, 0.27 + stripe, 0.14 + stripe, 1.)
                    }
                    6 => {
                        let ring = (x.abs_diff(7).max(y.abs_diff(7)) % 3) as f32 * 0.04;
                        (0.68 - ring, 0.52 - ring, 0.30 - ring, 1.)
                    }
                    7 => {
                        let d = (hash(x / 2, y / 2, 7) % 5) as f32 * 0.023;
                        (0.24 + d, 0.46 + d, 0.14 + d, 1.)
                    }
                    8 => {
                        if y % 4 == 0 || (x + (y / 4 % 2) * 8) % 16 == 0 {
                            (0.47, 0.32, 0.17, 1.)
                        } else {
                            (0.68, 0.51, 0.29, 1.)
                        }
                    }
                    9 => {
                        if y % 5 == 0 || (x + (y / 5 % 2) * 4) % 8 == 0 {
                            (0.70, 0.64, 0.52, 1.)
                        } else {
                            (0.64, 0.32, 0.23, 1.)
                        }
                    }
                    10 => {
                        if x == 0 || x == 15 || y == 0 || y == 15 {
                            (0.72, 0.87, 0.89, 1.)
                        } else if (x + y == 11 || x + y == 12 || x + y == 22) && x > 3 && x < 12 {
                            (0.84, 0.95, 0.95, 1.)
                        } else {
                            (0.75, 0.91, 0.95, 0.)
                        }
                    }
                    11 => {
                        if (y + x / 6) % 7 == 0 {
                            (0.27, 0.60, 0.77, 1.)
                        } else {
                            (0.19, 0.49, 0.68, 1.)
                        }
                    }
                    12 => {
                        let d = (hash(x / 2, y / 2, 12) % 4) as f32 * 0.045;
                        (0.22 + d, 0.23 + d, 0.24 + d, 1.)
                    }
                    13 => (0.96, 0.98, 0.96, 1.),
                    _ => (1., 1., 1., 1.),
                };
                let noise = if tile == 10 || tile == 13 { 0. } else { n };
                r = (r + noise).clamp(0., 1.);
                g = (g + noise).clamp(0., 1.);
                b = (b + noise).clamp(0., 1.);
                if (tile == 1 && y > 5 || tile == 2) && h.is_multiple_of(31) {
                    r *= 0.72;
                    g *= 0.76;
                    b *= 0.8;
                }
                if tile == 3 && (x == (y * 3 / 5 + 5) % 16 || h.is_multiple_of(43)) {
                    r *= 0.85;
                    g *= 0.85;
                    b *= 0.85;
                }
                image.set_pixel(tile % 4 * TILE + x, tile / 4 * TILE + y, Color::new(r, g, b, a));
            }
        }
    }
    let texture = Texture2D::from_image(&image);
    texture.set_filter(FilterMode::Nearest);
    texture
}

// A miniature isometric cube uses the same texels as its full-size counterpart.
fn block_icon(atlas: &Texture2D, block: u8, x: f32, y: f32, r: f32) {
    let top = vec2(x, y - r);
    let left = vec2(x - r, y - r * 0.5);
    let right = vec2(x + r, y - r * 0.5);
    let center = vec2(x, y);
    let bottom = vec2(x, y + r);
    let quads = [
        ([left, center, right, top], 2, 1.),
        ([left + vec2(0., r), bottom, center, left], 1, 0.72),
        ([bottom, right + vec2(0., r), right, center], 0, 0.88),
    ];
    let mut mesh =
        Mesh { vertices: Vec::with_capacity(12), indices: Vec::with_capacity(18), texture: Some(atlas.clone()) };
    for (points, f, light) in quads {
        face(&mut mesh, points.map(|p| vec3(p.x, p.y, 0.)), tile_for(block, f), [light; 4]);
    }
    draw_mesh(&mesh);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_voxel_faces_are_non_degenerate_and_point_outward() {
        for f in 0..6 {
            let p = CORNERS[f].map(|p| vec3(p[0] as f32, p[1] as f32, p[2] as f32));
            let n = NORMALS[f];
            let expected = vec3(n[0] as f32, n[1] as f32, n[2] as f32);
            assert!((p[1] - p[0]).cross(p[2] - p[0]).dot(expected) > 0.99);
            assert!((p[2] - p[0]).cross(p[3] - p[0]).dot(expected) > 0.99);
        }
    }
}
