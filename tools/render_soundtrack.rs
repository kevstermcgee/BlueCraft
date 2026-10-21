//! Offline, deterministic BlueCraft score renderer. No device, network, or external samples.
//! Build/run through `python3 tools/render_soundtrack.py`; this is never a startup renderer.
//! Each song below has its own authored harmony, rhythm, motifs, instrumentation and form.
use std::{
    f64::consts::{FRAC_PI_2, TAU},
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
};

const RATE: usize = 44_100;
const INTRO: f64 = 8.0;
const TAIL: f64 = 12.0;

#[derive(Clone, Copy, Debug)]
enum Voice {
    Felt,
    Glass,
    Dulcimer,
    Pluck,
    Pad,
    Air,
    Bass,
}
#[derive(Clone, Copy)]
struct Chord {
    bass: u8,
    notes: [u8; 4],
}
const fn c(bass: u8, notes: [u8; 4]) -> Chord {
    Chord { bass, notes }
}
// Shared timbral vocabulary; these voicings are deliberately selected, not random chords.
const F: Chord = c(41, [53, 57, 60, 64]);
const C: Chord = c(36, [52, 55, 60, 62]);
const G: Chord = c(43, [55, 59, 62, 67]);
const AM: Chord = c(45, [55, 57, 60, 64]);
const DM: Chord = c(38, [53, 57, 62, 65]);
const EM: Chord = c(40, [52, 55, 59, 62]);
const BB: Chord = c(46, [53, 58, 62, 65]);
const GSUS: Chord = c(43, [55, 60, 62, 67]);
const CSUS: Chord = c(36, [55, 60, 62, 65]);
const DORIAN: Chord = c(38, [53, 57, 60, 64]);
// (beat in an eight-bar phrase, MIDI note, hold in beats). Rests are real space.
type N = (f64, u8, f64);
struct Song {
    id: &'static str,
    title: &'static str,
    bpm: f64,
    meter: usize,
    bars: usize,
    key: &'static str,
    description: &'static str,
    chords: &'static [Chord],
    phrases: [&'static [N]; 3],
    form: [usize; 6],
    lead: Voice,
    companion: Voice,
    texture: u8,
}

fn songs() -> Vec<Song> {
    vec![
    Song { id: "open-meadow", title: "Open Meadow", bpm: 60., meter: 4, bars: 43, key: "F major",
        description: "An expanded edition of the original meadow theme: unhurried felt piano, broad ninth chords and a warm middle register.",
        chords: &[F, AM, DM, BB, AM, GSUS, C, F],
        phrases: [
            &[(0.5,69,1.7),(3.,67,1.),(5.3,72,1.8),(8.5,69,1.3),(11.,65,2.),(16.5,72,1.8),(19.,69,1.4),(22.6,70,2.),(27.,67,1.2),(29.,65,2.)],
            &[(0.5,72,1.6),(3.2,69,1.),(6.,71,2.),(9.,69,1.1),(11.5,72,1.5),(16.5,74,2.),(21.,72,2.),(24.5,69,1.1),(27.,65,3.)],
            &[(1.,65,2.3),(6.,67,2.),(9.,69,1.2),(11.,64,2.),(16.4,65,2.),(20.5,62,1.5),(24.4,64,1.1),(27.,67,1.7),(30.,65,1.5)]],
        form: [0,1,2,0,1,2], lead: Voice::Felt, companion: Voice::Felt, texture: 0 },
    Song { id: "glasswater", title: "Glasswater", bpm: 68., meter: 3, bars: 56, key: "C major",
        description: "A lilting three-beat glass-key motif reflects over open C and G voicings, with droplets answered by low piano.",
        chords: &[C, G, AM, F, C, EM, F, GSUS],
        phrases: [
            &[(0.,76,0.7),(1.5,79,1.),(4.,74,1.4),(6.5,72,1.),(9.,76,1.8),(12.,77,0.7),(13.5,76,1.2),(17.,74,1.6),(21.,72,2.)],
            &[(0.5,79,1.),(3.,76,1.2),(5.,74,0.8),(7.5,72,1.8),(12.,69,1.),(14.,72,1.3),(18.,74,1.),(19.5,76,0.8),(22.,72,1.5)],
            &[(1.,67,2.),(5.,72,1.2),(9.,76,2.),(13.,74,1.),(16.,72,1.7),(20.,67,2.5)]],
        form: [0,1,0,2,1,2], lead: Voice::Glass, companion: Voice::Felt, texture: 1 },
    Song { id: "below-the-ferns", title: "Below the Ferns", bpm: 56., meter: 4, bars: 36, key: "D minor",
        description: "A shaded, descending piano melody, muted plucked answers and close, soft D-minor string voicings.",
        chords: &[DM, BB, F, C, DM, AM, BB, CSUS],
        phrases: [
            &[(1.,77,2.),(5.5,76,1.8),(9.,74,2.8),(14.,69,2.),(17.,72,1.2),(19.5,69,1.5),(24.,65,3.),(29.,64,1.8)],
            &[(1.,69,2.),(5.,70,1.6),(8.,74,2.6),(13.,72,1.2),(17.,69,2.5),(23.,67,2.),(27.,65,3.)],
            &[(2.,62,3.),(8.,65,2.),(12.,69,2.5),(18.,67,2.),(24.,65,2.8),(29.,62,2.)]],
        form: [0,1,2,1,0,2], lead: Voice::Felt, companion: Voice::Pluck, texture: 2 },
    Song { id: "clouds-in-pieces", title: "Clouds in Pieces", bpm: 64., meter: 4, bars: 44, key: "G major",
        description: "Wide, gently rising felt-key intervals drift over changing G-major inversions; a high glass voice appears in the central passage.",
        chords: &[G, EM, C, G, AM, EM, CSUS, G],
        phrases: [
            &[(0.8,67,1.8),(4.,74,2.),(8.5,71,1.2),(11.,76,2.2),(16.8,72,1.3),(20.,79,2.2),(25.,74,2.),(29.,67,2.5)],
            &[(0.8,71,2.),(5.,74,1.8),(10.,79,2.6),(16.,76,2.),(20.5,74,1.2),(24.,72,2.),(29.,71,2.)],
            &[(2.,62,2.),(8.,67,2.4),(13.,71,1.8),(18.,69,2.2),(24.,67,2.8),(29.,62,2.)]],
        form: [0,1,2,1,0,2], lead: Voice::Felt, companion: Voice::Glass, texture: 3 },
    Song { id: "lantern-current", title: "Lantern Current", bpm: 72., meter: 4, bars: 44, key: "A minor",
        description: "A gentle offbeat wooden pluck carries interlocking A-minor figures, answered by a warm felt-key countermelody.",
        chords: &[AM, EM, F, C, DM, AM, F, GSUS],
        phrases: [
            &[(0.5,69,0.8),(2.,72,0.8),(4.5,76,1.),(7.,72,0.9),(9.5,67,1.5),(13.,64,1.5),(16.5,65,1.),(19.,69,0.9),(22.,72,1.7),(27.,67,1.5),(30.,69,1.)],
            &[(0.5,76,0.8),(3.,74,1.1),(6.5,71,1.2),(10.,72,1.8),(16.5,74,0.8),(19.,72,1.2),(23.,69,1.8),(28.,67,2.)],
            &[(1.,64,2.),(6.,67,1.6),(10.5,69,2.2),(17.,65,1.6),(22.,64,2.),(28.,62,2.)]],
        form: [0,1,2,0,1,2], lead: Voice::Pluck, companion: Voice::Felt, texture: 4 },
    Song { id: "a-place-to-rest", title: "A Place to Rest", bpm: 54., meter: 4, bars: 36, key: "F major",
        description: "Long, low piano tones and a nearly still organ-like pad leave generous space; the middle phrase opens into the upper octave.",
        chords: &[F, CSUS, BB, F, DM, AM, BB, C],
        phrases: [
            &[(1.5,60,3.4),(8.5,65,3.),(16.,64,3.),(23.,62,3.),(29.,60,2.)],
            &[(1.5,65,3.),(8.,69,3.),(15.,72,3.),(22.,70,2.4),(28.,67,3.)],
            &[(2.,72,3.),(9.,69,2.5),(16.,65,3.2),(24.,64,3.),(29.,60,2.)]],
        form: [0,1,2,0,1,2], lead: Voice::Felt, companion: Voice::Air, texture: 5 },
    Song { id: "copper-dawn", title: "Copper Dawn", bpm: 66., meter: 4, bars: 44, key: "C major",
        description: "Soft dulcimer pairs trace a rising morning figure over a brighter C-major progression, with sparse left-hand piano responses.",
        chords: &[C, F, AM, G, C, EM, DM, G],
        phrases: [
            &[(0.3,64,0.8),(1.6,67,1.),(4.3,72,1.4),(8.3,69,0.8),(10.,72,1.5),(16.3,76,1.),(18.,74,1.5),(22.,72,2.),(28.,67,2.)],
            &[(0.3,67,0.8),(1.8,72,1.),(5.,76,1.7),(9.,77,1.6),(13.,76,1.7),(17.,74,1.5),(22.,69,2.),(28.,67,2.)],
            &[(1.,60,2.5),(6.,64,1.8),(10.,67,2.),(17.,65,2.),(23.,62,2.),(28.,67,2.5)]],
        form: [0,1,0,2,1,2], lead: Voice::Dulcimer, companion: Voice::Felt, texture: 6 },
    Song { id: "far-hills", title: "Far Hills", bpm: 58., meter: 3, bars: 54, key: "G major",
        description: "A slow three-beat piano line climbs and settles above wide G-major pads, with distant bell echoes only at phrase endings.",
        chords: &[G, C, EM, AM, G, C, GSUS, G],
        phrases: [
            &[(1.,62,1.5),(4.5,67,1.8),(8.,69,1.6),(12.,71,2.),(16.,74,1.8),(20.,67,2.6)],
            &[(0.5,74,2.),(5.,72,1.5),(8.5,71,2.),(13.,69,1.8),(17.,67,2.),(21.,62,2.)],
            &[(1.,67,2.5),(7.,62,2.3),(13.,64,2.),(18.,67,3.)]],
        form: [0,1,2,0,1,2], lead: Voice::Felt, companion: Voice::Glass, texture: 7 },
    Song { id: "rain-on-leaves", title: "Rain on Leaves", bpm: 62., meter: 4, bars: 40, key: "D Dorian",
        description: "Irregularly spaced glass droplets and damped wooden keys hover over a D-Dorian progression; there is no rain recording or percussion.",
        chords: &[DORIAN, G, C, DM, EM, AM, G, CSUS],
        phrases: [
            &[(0.7,74,0.9),(3.2,76,1.3),(7.4,69,1.5),(11.7,72,1.1),(16.3,71,1.8),(21.6,69,1.2),(25.2,67,1.7),(30.,65,1.3)],
            &[(1.2,77,1.3),(5.6,76,1.1),(9.8,74,1.8),(15.4,72,1.1),(19.7,76,1.5),(24.8,71,1.4),(29.3,69,1.6)],
            &[(2.1,62,2.1),(7.7,65,1.8),(13.2,67,2.),(19.6,69,2.2),(26.5,64,2.4)]],
        form: [0,2,1,0,2,1], lead: Voice::Glass, companion: Voice::Pluck, texture: 8 },
    Song { id: "homeward-light", title: "Homeward Light", bpm: 60., meter: 4, bars: 42, key: "C major",
        description: "An answering piano theme with falling sixths resolves into a spacious C-major coda, warm pads and a final distant glass response.",
        chords: &[C, AM, F, G, EM, F, DM, C],
        phrases: [
            &[(1.,76,2.),(5.,67,2.3),(10.,72,2.),(15.,64,2.5),(20.,69,1.8),(24.,67,2.),(29.,60,2.)],
            &[(1.,79,2.3),(6.,76,2.),(11.,72,2.),(16.,74,2.),(21.,69,2.),(26.,67,3.)],
            &[(2.,64,2.),(8.,67,2.),(13.,65,2.4),(19.,62,2.2),(25.,60,4.)]],
        form: [0,1,2,1,0,2], lead: Voice::Felt, companion: Voice::Glass, texture: 9 },
]
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

struct Mix {
    left: Vec<f32>,
    right: Vec<f32>,
    notes: usize,
    melodic: usize,
}
impl Mix {
    fn new(seconds: f64) -> Self {
        let n = (seconds * RATE as f64).round() as usize;
        Self { left: vec![0.; n], right: vec![0.; n], notes: 0, melodic: 0 }
    }
    fn note(&mut self, at: f64, hold: f64, midi: u8, gain: f64, pan: f64, voice: Voice) {
        if gain <= 0. {
            return;
        }
        self.notes += 1;
        let frequency = 440. * 2_f64.powf((midi as f64 - 69.) / 12.);
        let (attack, release, decay, partials): (f64, f64, f64, [(f64, f64); 4]) = match voice {
            Voice::Felt => (0.014, 4.2, 2.7, [(1., 1.), (2.001, 0.15), (3.004, 0.026), (4.01, 0.008)]),
            Voice::Glass => (0.022, 5.1, 2.3, [(1., 1.), (2.003, 0.13), (4.01, 0.042), (6.019, 0.009)]),
            Voice::Dulcimer => (0.011, 3.3, 1.8, [(1., 1.), (2.002, 0.23), (3.008, 0.055), (4.016, 0.018)]),
            Voice::Pluck => (0.010, 2.8, 1.25, [(1., 1.), (2., 0.24), (3., 0.07), (4., 0.018)]),
            Voice::Pad => (1.5, 3.8, 9999., [(1., 1.), (2., 0.045), (3., 0.012), (4., 0.)]),
            Voice::Air => (2.1, 4.5, 9999., [(1., 1.), (2., 0.019), (3., 0.004), (4., 0.)]),
            Voice::Bass => (0.12, 1.6, 5.4, [(1., 1.), (2., 0.095), (3., 0.), (4., 0.)]),
        };
        let start = (at * RATE as f64).round() as usize;
        let count = ((hold + release) * RATE as f64).round() as usize;
        let angle = (pan.clamp(-1., 1.) + 1.) * 0.5 * FRAC_PI_2;
        let (lg, rg) = (gain * angle.cos(), gain * angle.sin());
        let mut phase = [(0f64, 1f64); 4];
        let rotation = partials.map(|(ratio, _)| {
            let p = TAU * frequency * ratio / RATE as f64;
            (p.sin(), p.cos())
        });
        let damp = partials.map(|(ratio, _)| (-ratio.sqrt() / (decay * RATE as f64)).exp());
        let mut levels = partials.map(|(_, level)| level);
        for index in 0..count.min(self.left.len().saturating_sub(start)) {
            let t = index as f64 / RATE as f64;
            let envelope = smooth(t / attack) * smooth((hold + release - t) / release);
            let mut value = 0.;
            for h in 0..4 {
                value += phase[h].0 * levels[h];
                let (s, c) = phase[h];
                let (rs, rc) = rotation[h];
                phase[h] = (s * rc + c * rs, c * rc - s * rs);
                levels[h] *= damp[h];
            }
            let value = value * envelope;
            self.left[start + index] += (value * lg) as f32;
            self.right[start + index] += (value * rg) as f32;
        }
    }
    fn finish(&mut self) {
        // Feed-forward stereo diffusion. No loop wrapping or truncated release at the boundary.
        let dry_l = self.left.clone();
        let dry_r = self.right.clone();
        for (seconds, gain) in [(0.137, 0.16), (0.283, 0.10), (0.431, 0.065), (0.719, 0.035)] {
            let d = (seconds * RATE as f64) as usize;
            for i in d..self.left.len() {
                self.left[i] += dry_r[i - d] * gain;
                self.right[i] += dry_l[i - d] * gain;
            }
        }
        // One album-wide loudness target, peak-safe. No per-note normalization or compression.
        let energy = self.left.iter().chain(&self.right).map(|v| (*v as f64).powi(2)).sum::<f64>();
        let rms = (energy / (self.left.len() * 2) as f64).sqrt();
        let peak = self.left.iter().chain(&self.right).fold(0f64, |p, v| p.max((*v as f64).abs()));
        let gain = (0.052 / rms.max(0.001)).min(0.31 / peak.max(0.001));
        let seconds = self.left.len() as f64 / RATE as f64;
        for (i, (left, right)) in self.left.iter_mut().zip(&mut self.right).enumerate() {
            let t = i as f64 / RATE as f64;
            let fade = smooth(t / 2.5) * smooth((seconds - t - 1. / RATE as f64) / 4.);
            *left *= (gain * fade) as f32;
            *right *= (gain * fade) as f32;
        }
    }
}

fn render(song: &Song) -> Mix {
    let beat = 60. / song.bpm;
    let bar = song.meter as f64 * beat;
    let body_end = INTRO + song.bars as f64 * bar;
    let seconds = body_end + TAIL;
    let mut mix = Mix::new(seconds);
    // All songs enter and leave on the same quiet C/G/D bridge. Twelve seconds of overlap can
    // therefore join every album neighbour without an unrelated bass or melody collision.
    for (at, hold, gain) in [(0., 10.5, 0.030), (body_end - 3., 14., 0.030)] {
        for (midi, pan) in [(48, -0.3), (55, 0.25), (62, -0.05)] {
            mix.note(at, hold, midi, gain, pan, Voice::Air);
        }
    }
    for b in 0..song.bars {
        let at = INTRO + b as f64 * bar;
        let chord = song.chords[b % song.chords.len()];
        let section = b / 8;
        let arc = [0.77, 0.92, 1.0, 0.88, 0.72, 0.59][section.min(5)];
        let coda = if b + 4 >= song.bars { 0.70 } else { 1. };
        // Alternating sustained and moving inner voices differentiate the accompaniment itself.
        let pad = if song.texture == 5 { Voice::Air } else { Voice::Pad };
        let hold = if song.texture == 1 || song.texture == 7 { bar * 0.90 } else { bar * 0.95 };
        for (i, midi) in chord.notes.iter().enumerate() {
            let inversion = if song.texture == 3 && section == 2 && i == 3 { 12 } else { 0 };
            mix.note(at, hold, *midi + inversion, 0.0145 * arc * coda, (i as f64 - 1.5) * 0.27, pad);
        }
        if b + 2 < song.bars && !(song.texture == 5 && b % 2 == 1) {
            mix.note(at + 0.08, bar * 0.72, chord.bass, 0.026 * arc, 0., Voice::Bass);
        }
        match song.texture {
            0 if (section == 1 || section == 2) && b % 3 == 0 => {
                mix.note(at + 1.37, 2.4, chord.notes[1], 0.035, -0.35, Voice::Felt)
            }
            1 if b % 2 == 1 => mix.note(at + 1.5 * beat, 1.2 * beat, chord.notes[1], 0.032, -0.30, Voice::Felt),
            2 if b % 3 == 2 => mix.note(at + 2.6 * beat, 1.2 * beat, chord.notes[2], 0.027, 0.32, Voice::Pluck),
            3 if section == 2 && b % 2 == 0 => {
                mix.note(at + 2.3 * beat, 1.4 * beat, chord.notes[3] + 12, 0.024, 0.33, Voice::Glass)
            }
            4 if section != 2 && b % 2 == 0 => {
                mix.note(at + 1.5 * beat, beat, chord.notes[1], 0.029, -0.35, Voice::Pluck);
                mix.note(at + 3. * beat, beat, chord.notes[2], 0.024, 0.32, Voice::Pluck);
            }
            5 if b % 4 == 2 => mix.note(at + 1.3 * beat, 3. * beat, chord.notes[0], 0.039, -0.22, Voice::Felt),
            6 if section != 3 && b % 2 == 1 => {
                mix.note(at + 2.3 * beat, 1.5 * beat, chord.notes[1], 0.032, -0.32, Voice::Felt)
            }
            7 if b % 8 == 7 => mix.note(at + 1.8 * beat, 1.5 * beat, chord.notes[3] + 12, 0.024, 0.28, Voice::Glass),
            8 if b % 3 == 1 => mix.note(at + 2.7 * beat, beat, chord.notes[2], 0.029, -0.31, Voice::Pluck),
            9 if section >= 2 && b % 4 == 3 => {
                mix.note(at + 2.5 * beat, 1.5 * beat, chord.notes[3] + 12, 0.022, 0.31, Voice::Glass)
            }
            _ => {}
        }
    }
    let phrase_beats = (song.meter * 8) as f64;
    for section in 0..song.bars.div_ceil(8) {
        let phrase = song.phrases[song.form[section.min(5)]];
        let section_start = INTRO + section as f64 * phrase_beats * beat;
        for (n, &(at, midi, hold)) in phrase.iter().enumerate() {
            let start = section_start + at * beat;
            if start + hold * beat > body_end - 2. {
                continue;
            }
            let contour = [0.95, 0.82, 1.0, 0.87, 0.91][n % 5];
            let dynamic = [0.82, 0.94, 1.0, 0.86, 0.72, 0.58][section.min(5)];
            let lead_gain = match song.lead {
                Voice::Glass => 0.083,
                Voice::Pluck => 0.119,
                Voice::Dulcimer => 0.092,
                _ => 0.105,
            };
            let pan = ((n * 3 + section) % 7) as f64 * 0.055 - 0.165;
            mix.note(start, hold * beat, midi, lead_gain * contour * dynamic, pan, song.lead);
            mix.melodic += 1;
            if section == 2 && n % 3 == 1 && song.texture != 5 {
                // Authored middle-section responses use the chord underneath that time.
                let chord = song.chords[((section * 8) + (at / song.meter as f64) as usize) % song.chords.len()];
                mix.note(start + 1.35 * beat, 1.4 * beat, chord.notes[1], 0.027, -pan - 0.15, song.companion);
            }
        }
    }
    mix.finish();
    mix
}

fn wav(path: &Path, mix: &Mix) {
    let mut f = BufWriter::new(File::create(path).expect("create WAV"));
    let bytes = (mix.left.len() * 4) as u32;
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + bytes).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&(RATE as u32).to_le_bytes()).unwrap();
    f.write_all(&(RATE as u32 * 4).to_le_bytes()).unwrap();
    f.write_all(&4u16.to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&bytes.to_le_bytes()).unwrap();
    for (&l, &r) in mix.left.iter().zip(&mix.right) {
        for v in [l, r] {
            f.write_all(&((v.clamp(-1., 1.) * 32767.).round() as i16).to_le_bytes()).unwrap();
        }
    }
}

fn main() {
    let directory = std::env::args().nth(1).expect("output directory argument required");
    let directory = Path::new(&directory);
    fs::create_dir_all(directory).unwrap();
    let mut catalog = String::from(
        "{\n  \"version\": 1,\n  \"sample_rate\": 44100,\n  \"crossfade_seconds\": 12,\n  \"tracks\": [\n",
    );
    for (index, song) in songs().iter().enumerate() {
        eprintln!("Rendering {:02} {}", index + 1, song.title);
        let mix = render(song);
        let basename = format!("{:02}-{}", index + 1, song.id);
        wav(&directory.join(format!("{basename}.wav")), &mix);
        if index > 0 {
            catalog.push_str(",\n");
        }
        catalog.push_str(&format!("    {{\"id\":\"{}\",\"title\":\"{}\",\"file\":\"{}.ogg\",\"seconds\":{:.6},\"bpm\":{},\"meter\":{},\"key\":\"{}\",\"description\":\"{}\",\"notes\":{},\"melody_notes\":{},\"instrument\":\"{:?}\"}}", song.id,song.title,basename,mix.left.len() as f64/RATE as f64,song.bpm,song.meter,song.key,song.description,mix.notes,mix.melodic,song.lead));
    }
    catalog.push_str("\n  ]\n}\n");
    fs::write(directory.join("catalog.json"), catalog).unwrap();
}
