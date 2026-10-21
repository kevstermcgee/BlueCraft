//! Quiet material contacts for building and wandering; the soundtrack is pre-rendered separately.
use bluecraft::{Event, BRICK, DIRT, GLASS, GRASS, LEAVES, PLANKS, SAND, STONE, WOOD};
use std::f32::consts::TAU;
use vesper3d::viewer::{
    devkit::{synth, Rng},
    kit::Rendered,
};

const RATE: u32 = 44_100;
const MATERIALS: usize = 7;
pub const EFFECT_VARIANTS: usize = 3;
pub const EFFECT_COUNT: usize = MATERIALS * 3 + 2;
pub const LOADED_EFFECTS: usize = EFFECT_COUNT * EFFECT_VARIANTS;
const JUMP: usize = MATERIALS * 3;
const LAND: usize = JUMP + 1;

/// Soil, stone, sand, timber, foliage, glass and turf have distinct short, unpitched textures.
fn material(block: u8) -> usize {
    match block {
        STONE | BRICK => 1,
        SAND => 2,
        WOOD | PLANKS => 3,
        LEAVES => 4,
        GLASS => 5,
        GRASS => 6,
        DIRT => 0,
        _ => 0,
    }
}

pub fn event_sound(event: &Event) -> Option<(usize, f32)> {
    match *event {
        Event::Mined { block, .. } => Some((material(block), 0.62)),
        Event::Placed { block, .. } => Some((MATERIALS + material(block), 0.55)),
        Event::Jumped => Some((JUMP, 0.25)),
        Event::Landed => Some((LAND, 0.38)),
        Event::Respawned => None,
    }
}

/// Pass the block immediately beneath the player's feet, so walking reflects the surface.
pub fn footstep(block: u8) -> (usize, f32) {
    (MATERIALS * 2 + material(block), 0.42)
}

/// A landing is a firmer contact with the current surface, without a falling pitch or boing.
pub fn landing(block: u8) -> (usize, f32) {
    (MATERIALS * 2 + material(block), 0.68)
}

pub fn render() -> Rendered {
    let sfx = (0..EFFECT_COUNT)
        .map(|cue| (0..EFFECT_VARIANTS).map(|variant| synth::wav_bytes(&effect(cue, variant), RATE)).collect())
        .collect();
    Rendered { sfx, stems: Vec::new() }
}

fn smooth(value: f32) -> f32 {
    let x = value.clamp(0., 1.);
    x * x * (3. - 2. * x)
}

fn effect(cue: usize, variant: usize) -> Vec<f32> {
    let family = cue % MATERIALS;
    let action = cue / MATERIALS;
    // Cutoff, low resonant body, noise/body balance. Short resonances communicate the material,
    // rather than using musical rewards, pitch ladders or comic jump sweeps.
    let (cutoff, frequency, texture, body) = match family {
        1 => (2_300., 145., 0.82, 0.18),
        2 => (1_850., 90., 0.97, 0.03),
        3 => (1_400., 225., 0.66, 0.34),
        4 => (3_500., 110., 0.99, 0.01),
        5 => (4_000., 1_650., 0.78, 0.22),
        6 => (1_650., 95., 0.95, 0.05),
        _ => (1_100., 105., 0.90, 0.10),
    };
    let special = cue == JUMP || cue == LAND;
    let seconds = if cue == JUMP {
        0.11
    } else if cue == LAND {
        0.16
    } else {
        [0.21, 0.13, 0.15][action] + variant as f32 * 0.007
    };
    let mut samples = vec![0.; (seconds * RATE as f32) as usize];
    let mut rng = Rng::new(0xB10C_A0D1 ^ ((cue * 17 + variant) as u64));
    let cutoff = if special { 850. } else { cutoff };
    let alpha = 1. - (-TAU * cutoff / RATE as f32).exp();
    let mut low = 0.;
    let duration = samples.len() as f32 / RATE as f32;
    for (index, sample) in samples.iter_mut().enumerate() {
        let t = index as f32 / RATE as f32;
        low += alpha * (rng.range(-1., 1.) - low);
        let decay = if cue == JUMP {
            0.030
        } else if special || action == 2 {
            0.038
        } else {
            0.042
        };
        let envelope = smooth(t / 0.004) * (-t / decay).exp() * smooth((duration - t) / 0.025);
        let resonance = (TAU * frequency * (1. + variant as f32 * 0.025) * t).sin() * (-t / 0.022).exp();
        // Mining includes two softer granular contacts; placement/steps remain one contact.
        let crumble = if !special && action == 0 {
            1. + 0.45 * (-((t - 0.043) / 0.009).powi(2)).exp() + 0.25 * (-((t - 0.078) / 0.013).powi(2)).exp()
        } else {
            1.
        };
        *sample = if special { low * envelope } else { (low * texture * crumble + resonance * body) * envelope };
    }
    // Preserve relative quietness across cue types, while avoiding a DC bias in noise transients.
    let mean = samples.iter().sum::<f32>() / samples.len() as f32;
    let length = samples.len();
    for (index, sample) in samples.iter_mut().enumerate() {
        let edge = smooth(index as f32 / 120.) * smooth((length - 1 - index) as f32 / 240.);
        *sample = (*sample - mean) * edge;
    }
    let peak = samples.iter().fold(0f32, |peak, v| peak.max(v.abs()));
    let target = if cue == JUMP {
        0.12
    } else if special {
        0.29
    } else {
        [0.40, 0.32, 0.27][action]
    };
    for sample in &mut samples {
        *sample *= target / peak.max(0.001);
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(samples: &[f32]) -> (f32, f32, f32) {
        let peak = samples.iter().fold(0f32, |p, v| p.max(v.abs()));
        let rms = (samples.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / samples.len() as f64).sqrt() as f32;
        let dc = samples.iter().map(|v| f64::from(*v)).sum::<f64>() / samples.len() as f64;
        (peak, rms, dc as f32)
    }

    #[test]
    fn contacts_reflect_material_and_event_without_a_respawn_jingle() {
        for block in [GRASS, DIRT, STONE, SAND, WOOD, LEAVES, GLASS] {
            let mined = event_sound(&Event::Mined { cell: [0; 3], block }).unwrap();
            let placed = event_sound(&Event::Placed { cell: [0; 3], block }).unwrap();
            let step = footstep(block);
            assert!(mined.0 < MATERIALS);
            assert_eq!(placed.0, mined.0 + MATERIALS);
            assert_eq!(step.0, mined.0 + MATERIALS * 2);
            assert_eq!(landing(block).0, step.0);
        }
        assert_eq!(footstep(WOOD), footstep(PLANKS));
        assert_eq!(footstep(STONE), footstep(BRICK));
        assert!(event_sound(&Event::Respawned).is_none());
    }

    #[test]
    fn all_contact_variants_are_short_quiet_click_free_and_reproducible() {
        for cue in 0..EFFECT_COUNT {
            for variant in 0..EFFECT_VARIANTS {
                let samples = effect(cue, variant);
                let (peak, rms, dc) = stats(&samples);
                assert!(samples.iter().all(|sample| sample.is_finite()));
                assert!(samples.len() < RATE as usize / 3);
                assert!((0.10..=0.401).contains(&peak), "cue {cue}: peak {peak}");
                assert!(rms > 0.003, "cue {cue}: rms {rms}");
                assert!(dc.abs() < 0.002, "cue {cue}: dc {dc}");
                assert_eq!(samples[0], 0.);
                assert_eq!(*samples.last().unwrap(), 0.);
                assert_eq!(samples, effect(cue, variant));
                if variant > 0 {
                    assert_ne!(samples, effect(cue, variant - 1));
                }
            }
        }
    }
}
