//! Original, quiet audio for building and wandering. All synthesis runs on SoundBank's worker.
//!
//! "Open Meadow" is a 3:12 score in F major, at 60 BPM: four different piano phrases over
//! twelve related, softly voiced chords. There are no borrowed recordings or melodies. A single
//! stereo stem keeps the instruments on the same sample clock; release and room tails wrap into
//! the beginning rather than being cut off when the backend loops it.
use bluecraft::{Event, BRICK, DIRT, GLASS, GRASS, LEAVES, PLANKS, SAND, STONE, WOOD};
use std::f32::consts::{FRAC_PI_2, TAU};
use vesper3d::viewer::{
    devkit::{synth, Rng},
    kit::Rendered,
};

const RATE: u32 = 44_100;
const MATERIALS: usize = 7;
pub const EFFECT_VARIANTS: usize = 3;
pub const EFFECT_COUNT: usize = MATERIALS * 3 + 2;
pub const LOADED_EFFECTS: usize = EFFECT_COUNT * EFFECT_VARIANTS;
pub const MUSIC_STEMS: usize = 1;
pub const MUSIC_SECONDS: usize = 192;
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
    let (left, right) = music();
    Rendered { sfx, stems: vec![synth::wav_bytes_stereo(&left, &right, RATE)] }
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

#[derive(Clone, Copy)]
enum Voice {
    Piano,
    Pad,
    Bass,
}

/// Each chord lasts four seconds. The upper voices move by small intervals; suspended ninths
/// and open fifths give the piano room, without a drum beat or an insistent arpeggio.
const CHORDS: [[u8; 4]; 12] = [
    [53, 57, 60, 64],
    [52, 55, 59, 62],
    [53, 57, 60, 64],
    [53, 57, 62, 65],
    [53, 57, 60, 67],
    [53, 57, 62, 65],
    [52, 55, 60, 62],
    [53, 57, 60, 64],
    [53, 57, 60, 64],
    [53, 57, 62, 65],
    [53, 57, 62, 65],
    [52, 55, 60, 62],
];
const BASS: [u8; 12] = [41, 45, 38, 46, 45, 43, 48, 41, 38, 46, 43, 48];
// Four original, deliberately sparse phrases. Zero is a rest. Notes occur within the current
// chord at gently uneven offsets, leaving long stretches for the tail and surrounding world.
const MELODY: [[[u8; 2]; 12]; 4] = [
    [[69, 0], [67, 72], [69, 65], [65, 0], [72, 69], [70, 0], [67, 62], [65, 0], [64, 69], [65, 0], [62, 65], [67, 0]],
    [[72, 69], [71, 0], [69, 72], [74, 0], [72, 0], [69, 65], [67, 72], [69, 0], [65, 64], [65, 69], [70, 0], [67, 0]],
    [[65, 0], [67, 0], [69, 64], [65, 0], [60, 65], [62, 0], [64, 67], [65, 0], [69, 0], [74, 69], [65, 0], [62, 64]],
    [[69, 72], [74, 0], [72, 69], [65, 0], [67, 69], [65, 62], [64, 0], [65, 69], [64, 0], [65, 0], [62, 65], [67, 0]],
];

fn music() -> (Vec<f32>, Vec<f32>) {
    let frames = MUSIC_SECONDS * RATE as usize;
    let mut left = vec![0.; frames];
    let mut right = vec![0.; frames];
    for (phrase, melody) in MELODY.iter().enumerate() {
        for (bar, notes) in melody.iter().enumerate() {
            let at = (phrase * 48 + bar * 4) as f32;
            for (voice, &midi) in CHORDS[bar].iter().enumerate() {
                note(&mut left, &mut right, at, 3.75, midi, 0.020, (voice as f32 - 1.5) * 0.28, Voice::Pad);
            }
            note(&mut left, &mut right, at + 0.08, 3.0, BASS[bar], 0.036, 0., Voice::Bass);
            for (index, &midi) in notes.iter().enumerate().filter(|(_, midi)| **midi != 0) {
                let start = at + [0.46, 2.57][index] + (bar % 3) as f32 * 0.07;
                let pan = ((bar * 7 + phrase * 3) % 9) as f32 * 0.065 - 0.26;
                let gain = if index == 0 { 0.115 } else { 0.080 };
                note(&mut left, &mut right, start, 2.1, midi, gain, pan, Voice::Piano);
            }
            // Widely spaced, low piano intervals appear only in the two fuller middle phrases.
            if (phrase == 1 || phrase == 2) && bar % 3 == 0 {
                note(&mut left, &mut right, at + 1.37, 2.4, CHORDS[bar][1], 0.047, -0.35, Voice::Piano);
            }
        }
    }
    // A small, fixed stereo room: diffuse reflections, no chorus, modulation or pitch wobble.
    let dry_left = left.clone();
    let dry_right = right.clone();
    for (seconds, gain) in [(0.137, 0.12), (0.283, 0.075), (0.431, 0.045)] {
        let delay = (seconds * RATE as f32) as usize;
        for index in 0..frames {
            let source = (index + frames - delay) % frames;
            left[index] += dry_right[source] * gain;
            right[index] += dry_left[source] * gain;
        }
    }
    // Quiet background headroom is authored into the asset rather than depending on menu defaults.
    let peak = left.iter().chain(&right).fold(0f32, |peak, sample| peak.max(sample.abs()));
    let gain = 0.32 / peak.max(0.32);
    for sample in left.iter_mut().chain(&mut right) {
        *sample *= gain;
    }
    (left, right)
}

#[allow(clippy::too_many_arguments)]
fn note(left: &mut [f32], right: &mut [f32], at: f32, hold: f32, midi: u8, gain: f32, pan: f32, voice: Voice) {
    let frequency = 440. * 2f32.powf((f32::from(midi) - 69.) / 12.);
    let (attack, release) = match voice {
        Voice::Piano => (0.015, 3.6),
        Voice::Pad => (0.95, 1.15),
        Voice::Bass => (0.09, 0.65),
    };
    let frames = ((hold + release) * RATE as f32) as usize;
    let start = (at * RATE as f32) as usize;
    let angle = (pan + 1.) * 0.5 * FRAC_PI_2;
    let (lgain, rgain) = (angle.cos() * gain, angle.sin() * gain);
    for index in 0..frames {
        let t = index as f32 / RATE as f32;
        let envelope = smooth(t / attack) * smooth((hold + release - t) / release);
        let phase = TAU * frequency * t;
        let sample = match voice {
            Voice::Piano => {
                phase.sin() * (-t / 2.3).exp()
                    + 0.18 * (phase * 2.).sin() * (-t / 0.85).exp()
                    + 0.035 * (phase * 3.).sin() * (-t / 0.4).exp()
            }
            Voice::Pad => phase.sin() + 0.075 * (phase * 2.).sin(),
            Voice::Bass => (phase.sin() + 0.10 * (phase * 2.).sin()) * (-t / 2.8).exp(),
        } * envelope;
        let destination = (start + index) % left.len();
        left[destination] += sample * lgain;
        right[destination] += sample * rgain;
    }
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

    #[test]
    fn complete_score_has_background_headroom_and_a_continuous_stereo_wrap() {
        let (left, right) = music();
        assert_eq!(left.len(), MUSIC_SECONDS * RATE as usize);
        assert_eq!(left.len(), right.len());
        assert_ne!(left, right);
        for channel in [&left, &right] {
            let (peak, rms, dc) = stats(channel);
            eprintln!(
                "Open Meadow: {}s, peak {peak:.5}, RMS {rms:.5}, DC {dc:.7}, seam {:.7}",
                MUSIC_SECONDS,
                (channel[0] - channel[channel.len() - 1]).abs()
            );
            assert!(channel.iter().all(|sample| sample.is_finite()));
            assert!(peak <= 0.321);
            assert!((0.01..0.10).contains(&rms));
            assert!(dc.abs() < 0.0001);
            assert!((channel[0] - channel[channel.len() - 1]).abs() < 0.003);
        }
        // Optional offline evidence, never an audio-device or physical-listening claim.
        if let Some(directory) = std::env::var_os("BLUECRAFT_AUDIO_PREVIEW_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("open-meadow.wav"), synth::wav_bytes_stereo(&left, &right, RATE)).unwrap();
            let mut contacts = Vec::new();
            for cue in 0..EFFECT_COUNT {
                contacts.extend(effect(cue, 0));
                contacts.resize(contacts.len() + RATE as usize / 5, 0.);
            }
            std::fs::write(directory.join("material-contacts.wav"), synth::wav_bytes(&contacts, RATE)).unwrap();
        }
    }
}
