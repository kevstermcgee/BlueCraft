//! BlueCraft's original album. Only the current song and its successor reach the audio backend.
//!
//! Ogg validation and decoding run on workers. Macroquad only receives prepared PCM WAV data on
//! its required window thread. Playback clocks begin when play is submitted, rather than while a
//! file is loading. The backend exposes no device acknowledgement or sample-position query.
use macroquad::audio::{load_sound_from_bytes, play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Instant,
};
use vesper3d::{
    runtime::hash::sha256,
    viewer::devkit::{downloads_dir, runtime_assets, sanitize_filename},
};

pub const TRACK_COUNT: usize = 10;
const RATE: u32 = 44_100;
const MAX_ENCODED_BYTES: u64 = 16 * 1024 * 1024;
const PRELOAD_SECONDS: f64 = 45.;
const FIRST_FADE_SECONDS: f64 = 4.;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub file: String,
    pub seconds: f64,
    pub bpm: f64,
    pub key: String,
    #[serde(default)]
    pub description: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Catalog {
    pub version: u32,
    pub sample_rate: u32,
    pub crossfade_seconds: f64,
    pub tracks: Vec<Track>,
}

pub fn load_catalog() -> Result<(PathBuf, Catalog), String> {
    let root = runtime_assets("assets/music", Path::new(env!("CARGO_MANIFEST_DIR")))?;
    let catalog = Catalog::load(&root)?;
    Ok((root, catalog))
}

impl Catalog {
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("catalog.json");
        if fs::metadata(&path).map_err(|error| format!("Music catalog: {error}"))?.len() > 1_000_000 {
            return Err("Music catalog is too large".into());
        }
        let bytes = fs::read(&path).map_err(|error| format!("Music catalog: {error}"))?;
        let catalog: Self = serde_json::from_slice(&bytes).map_err(|error| format!("Music catalog: {error}"))?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.sample_rate != RATE || self.crossfade_seconds != 12. {
            return Err("Unsupported music catalog version, sample rate or crossfade".into());
        }
        if self.tracks.len() != TRACK_COUNT {
            return Err(format!("Music catalog must contain exactly {TRACK_COUNT} songs"));
        }
        let (mut ids, mut titles, mut files) = (HashSet::new(), HashSet::new(), HashSet::new());
        for track in &self.tracks {
            let safe_id = !track.id.is_empty()
                && track.id.len() <= 64
                && track.id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
            let safe_file = track.file.len() <= 100
                && track.file.ends_with(".ogg")
                && !track.file.contains(['/', '\\'])
                && track.file.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.')
                && matches!(Path::new(&track.file).components().collect::<Vec<_>>().as_slice(), [Component::Normal(_)]);
            if !safe_id
                || !safe_file
                || track.title.trim().is_empty()
                || track.title.len() > 96
                || track.title.chars().any(char::is_control)
                || !track.seconds.is_finite()
                || !(120. ..=240.).contains(&track.seconds)
                || !track.bpm.is_finite()
                || !(30. ..=120.).contains(&track.bpm)
                || track.sha256.len() != 64
                || !track.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(format!("Invalid music metadata for {}", track.title));
            }
            if !ids.insert(track.id.to_lowercase())
                || !titles.insert(track.title.to_lowercase())
                || !files.insert(track.file.to_lowercase())
            {
                return Err("Music song IDs, titles and filenames must be unique".into());
            }
        }
        Ok(())
    }
}

fn digest_hex(bytes: &[u8]) -> String {
    sha256(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn verified_bytes(root: &Path, track: &Track) -> Result<Vec<u8>, String> {
    let path = root.join(&track.file);
    let metadata = fs::symlink_metadata(&path).map_err(|error| format!("{}: {error}", track.title))?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_ENCODED_BYTES {
        return Err(format!("{} is not a bounded regular music file", track.title));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(path)
        .and_then(|file| file.take(MAX_ENCODED_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("{}: {error}", track.title))?;
    if bytes.len() as u64 > MAX_ENCODED_BYTES {
        return Err(format!("{} exceeds the music file size limit", track.title));
    }
    if digest_hex(&bytes) != track.sha256.to_ascii_lowercase() {
        return Err(format!("{} failed its music checksum", track.title));
    }
    Ok(bytes)
}

struct Prepared {
    index: usize,
    wav: Vec<u8>,
    seconds: f64,
}

fn decode(root: &Path, index: usize, track: &Track) -> Result<Prepared, String> {
    let bytes = verified_bytes(root, track)?;
    let mut reader = audrey::Reader::new(Cursor::new(bytes)).map_err(|error| format!("{}: {error}", track.title))?;
    let description = reader.description();
    if reader.format() != audrey::Format::OggVorbis
        || description.channel_count() != 2
        || description.sample_rate() != RATE
    {
        return Err(format!("{} must be stereo 44.1 kHz Ogg Vorbis", track.title));
    }
    let expected = (track.seconds * f64::from(RATE) * 2.).round() as usize;
    let limit = expected + RATE as usize * 2;
    let mut wav = Vec::with_capacity(44 + expected * 2);
    wav.resize(44, 0);
    let mut samples = 0;
    for sample in reader.samples::<i16>() {
        if samples >= limit {
            return Err(format!("{} exceeds its declared music duration", track.title));
        }
        wav.extend_from_slice(&sample.map_err(|error| format!("{}: {error}", track.title))?.to_le_bytes());
        samples += 1;
    }
    let seconds = samples as f64 / (f64::from(RATE) * 2.);
    if samples % 2 != 0 || (seconds - track.seconds).abs() > 0.05 {
        return Err(format!("{} decoded duration {seconds:.5}s differs from its catalog", track.title));
    }
    let data_bytes = (samples * 2) as u32;
    wav[..4].copy_from_slice(b"RIFF");
    wav[4..8].copy_from_slice(&(36 + data_bytes).to_le_bytes());
    wav[8..16].copy_from_slice(b"WAVEfmt ");
    wav[16..20].copy_from_slice(&16u32.to_le_bytes());
    wav[20..22].copy_from_slice(&1u16.to_le_bytes());
    wav[22..24].copy_from_slice(&2u16.to_le_bytes());
    wav[24..28].copy_from_slice(&RATE.to_le_bytes());
    wav[28..32].copy_from_slice(&(RATE * 4).to_le_bytes());
    wav[32..34].copy_from_slice(&4u16.to_le_bytes());
    wav[34..36].copy_from_slice(&16u16.to_le_bytes());
    wav[36..40].copy_from_slice(b"data");
    wav[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    Ok(Prepared { index, wav, seconds })
}

#[derive(Clone, Copy, Debug)]
struct Playing {
    index: usize,
    seconds: f64,
    started: f64,
}

/// Presentation-only time. A transition is recorded after its actual playback submission.
#[derive(Default)]
struct PlaylistClock {
    current: Option<Playing>,
    incoming: Option<Playing>,
    crossfade: f64,
    starts: u64,
    transitions: u64,
}

impl PlaylistClock {
    fn new(crossfade: f64) -> Self {
        Self { crossfade, ..Self::default() }
    }

    fn start(&mut self, index: usize, seconds: f64, now: f64) {
        self.current = Some(Playing { index, seconds, started: now });
        self.starts += 1;
    }

    fn preload_due(&self, now: f64) -> bool {
        self.current.is_some_and(|current| now - current.started >= current.seconds - self.crossfade - PRELOAD_SECONDS)
    }

    fn transition_due(&self, now: f64) -> bool {
        self.incoming.is_none()
            && self.current.is_some_and(|current| now - current.started >= current.seconds - self.crossfade)
    }

    fn start_incoming(&mut self, index: usize, seconds: f64, now: f64) {
        self.incoming = Some(Playing { index, seconds, started: now });
        self.starts += 1;
    }

    fn phase(&self, now: f64) -> f32 {
        self.incoming.map_or(0., |incoming| ((now - incoming.started) / self.crossfade).clamp(0., 1.) as f32)
    }

    fn gains(&self, now: f64) -> (f32, f32) {
        if self.incoming.is_some() {
            let angle = self.phase(now) * std::f32::consts::FRAC_PI_2;
            (angle.cos().max(0.), angle.sin())
        } else if self.starts == 1 {
            let fade =
                self.current.map_or(0., |current| ((now - current.started) / FIRST_FADE_SECONDS).clamp(0., 1.) as f32);
            ((fade * std::f32::consts::FRAC_PI_2).sin(), 0.)
        } else {
            (1., 0.)
        }
    }

    fn finish(&mut self, now: f64) -> bool {
        if self.incoming.is_some() && self.phase(now) >= 1. {
            self.current = self.incoming.take();
            self.transitions += 1;
            true
        } else {
            false
        }
    }

    fn dominant(&self, now: f64) -> Option<usize> {
        if self.incoming.is_some() && self.phase(now) >= 0.5 {
            self.incoming.map(|track| track.index)
        } else {
            self.current.map(|track| track.index)
        }
    }
}

#[derive(Default)]
struct Level {
    now: f32,
    updated: f64,
}

impl Level {
    fn update(&mut self, volume: f32, paused: bool, time: f64) -> f32 {
        let target = if volume.is_finite() { volume.clamp(0., 1.) } else { 0. } * if paused { 0.35 } else { 1. };
        let elapsed = (time - self.updated).clamp(0., 1.) as f32;
        self.now += (target - self.now) * (1. - (-elapsed / 0.35).exp());
        self.updated = time;
        self.now
    }
}

struct Loaded {
    index: usize,
    sound: Sound,
    seconds: f64,
}

pub struct Soundtrack {
    pub catalog: Catalog,
    root: PathBuf,
    muted: bool,
    origin: Instant,
    clock: PlaylistClock,
    level: Level,
    current: Option<Loaded>,
    next: Option<Loaded>,
    loading: Option<(usize, Receiver<Result<Prepared, String>>)>,
    exporting: Option<Receiver<Result<PathBuf, String>>>,
    failed: HashSet<usize>,
    errors: Vec<String>,
    longest_submission_ms: f64,
}

impl Soundtrack {
    pub fn new(root: PathBuf, catalog: Catalog, muted: bool) -> Self {
        let crossfade = catalog.crossfade_seconds;
        let mut soundtrack = Self {
            catalog,
            root,
            muted,
            origin: Instant::now(),
            clock: PlaylistClock::new(crossfade),
            level: Level::default(),
            current: None,
            next: None,
            loading: None,
            exporting: None,
            failed: HashSet::new(),
            errors: Vec::new(),
            longest_submission_ms: 0.,
        };
        if let Err(error) = soundtrack.catalog.validate() {
            soundtrack.errors.push(error);
            soundtrack.failed.extend(0..soundtrack.catalog.tracks.len());
        } else if !muted {
            soundtrack.prepare(0);
        }
        soundtrack
    }

    fn now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64()
    }

    fn successor(&self, current: usize) -> Option<usize> {
        (1..self.catalog.tracks.len())
            .map(|offset| (current + offset) % self.catalog.tracks.len())
            .find(|index| !self.failed.contains(index))
    }

    fn prepare(&mut self, index: usize) {
        if self.muted || self.loading.is_some() || self.failed.contains(&index) {
            return;
        }
        let root = self.root.clone();
        let track = self.catalog.tracks[index].clone();
        let (tx, rx) = mpsc::channel();
        self.loading = Some((index, rx));
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| decode(&root, index, &track))
                .unwrap_or_else(|_| Err(format!("{} music decoder failed", track.title)));
            let _ = tx.send(result);
        });
    }

    pub async fn poll(&mut self) {
        let result = match self.loading.as_ref() {
            Some((index, receiver)) => match receiver.try_recv() {
                Ok(result) => Some((*index, result)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some((*index, Err("Music preparation worker disconnected".into()))),
            },
            None => None,
        };
        let Some((index, result)) = result else { return };
        self.loading = None;
        let result = match result {
            Ok(prepared) => {
                let began = Instant::now();
                let sound = load_sound_from_bytes(&prepared.wav).await;
                self.longest_submission_ms = self.longest_submission_ms.max(began.elapsed().as_secs_f64() * 1000.);
                sound
                    .map(|sound| Loaded { index: prepared.index, sound, seconds: prepared.seconds })
                    .map_err(|error| format!("Music backend submission: {error}"))
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(loaded) => {
                if self.current.is_none() {
                    self.current = Some(loaded);
                } else {
                    self.next = Some(loaded);
                }
            }
            Err(error) => {
                self.failed.insert(index);
                self.errors.push(error);
            }
        }
    }

    pub fn update(&mut self, volume: f32, paused: bool) {
        if self.muted {
            return;
        }
        let mut now = self.now();
        if self.clock.current.is_none() {
            if let Some(current) = &self.current {
                play_sound(&current.sound, PlaySoundParams { looped: true, volume: 0. });
                now = self.now();
                self.clock.start(current.index, current.seconds, now);
            } else if self.loading.is_none() {
                if let Some(index) = (0..self.catalog.tracks.len()).find(|index| !self.failed.contains(index)) {
                    self.prepare(index);
                }
            }
        }
        if self.clock.finish(now) {
            if let Some(outgoing) = self.current.take() {
                stop_sound(&outgoing.sound);
            }
            self.current = self.next.take();
        }
        if self.clock.transition_due(now) {
            if let Some(incoming) = &self.next {
                // Start from this real submission time even after a hitch or late preparation.
                // The outgoing voice loops while waiting, so a delayed load cannot create a gap.
                play_sound(&incoming.sound, PlaySoundParams { looped: true, volume: 0. });
                now = self.now();
                self.clock.start_incoming(incoming.index, incoming.seconds, now);
            }
        }
        if self.next.is_none() && self.loading.is_none() && self.clock.preload_due(now) {
            if let Some(current) = &self.current {
                if let Some(index) = self.successor(current.index) {
                    self.prepare(index);
                }
            }
        }
        let level = self.level.update(volume, paused, now);
        let (outgoing, incoming) = self.clock.gains(now);
        if let Some(current) = &self.current {
            set_sound_volume(&current.sound, level * outgoing);
        }
        if let Some(next) = &self.next {
            set_sound_volume(&next.sound, level * incoming);
        }
    }

    pub fn current_track(&self) -> Option<usize> {
        self.clock.dominant(self.now())
    }

    pub fn status(&self) -> String {
        if self.muted {
            "Soundtrack intentionally muted".into()
        } else if let Some(error) = self.errors.last() {
            format!("Music: {error}")
        } else {
            let now = self.now();
            let dominant = if self.clock.phase(now) >= 0.5 { self.clock.incoming } else { self.clock.current };
            let Some(track) = dominant else { return "Preparing soundtrack".into() };
            let timestamp = |seconds: f64| {
                let seconds = seconds.max(0.).floor() as u64;
                format!("{}:{:02}", seconds / 60, seconds % 60)
            };
            let position = (now - track.started).max(0.).rem_euclid(track.seconds);
            let progress = format!("{} / {}", timestamp(position), timestamp(track.seconds.round()));
            if let Some(incoming) = self.clock.incoming {
                let other =
                    if incoming.index == track.index { self.clock.current.unwrap().index } else { incoming.index };
                format!("{progress} - blending with {}", self.catalog.tracks[other].title)
            } else {
                progress
            }
        }
    }

    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "muted": self.muted, "catalog_tracks": self.catalog.tracks.len(),
            "loaded_tracks": usize::from(self.current.is_some()) + usize::from(self.next.is_some()),
            "preparing": self.loading.is_some(), "playing_voices": usize::from(self.clock.current.is_some()) + usize::from(self.clock.incoming.is_some()),
            "track_starts": self.clock.starts, "transitions": self.clock.transitions,
            "current_track": self.current_track(), "load_failures": self.errors.len(), "errors": self.errors,
            "longest_pcm_submission_ms": self.longest_submission_ms,
            "timing": "monotonic clock after playback submission; backend position and device audibility unavailable",
        })
    }

    pub fn verify_playback(&self) -> Result<(), String> {
        if self.muted {
            return Ok(());
        }
        if !self.errors.is_empty() {
            return Err(self.errors.join("; "));
        }
        if self.current.is_none() || self.clock.starts == 0 {
            return Err("Soundtrack has not finished preparing and starting a song".into());
        }
        Ok(())
    }

    pub fn export_busy(&self) -> bool {
        self.exporting.is_some()
    }

    pub fn export(&mut self, index: usize) -> Result<(), String> {
        let directory = downloads_dir().ok_or_else(|| "Could not find your Downloads folder".to_string())?;
        self.export_to(index, directory)
    }

    fn export_to(&mut self, index: usize, directory: PathBuf) -> Result<(), String> {
        if self.export_busy() {
            return Err("A song is already being saved".into());
        }
        let track = self.catalog.tracks.get(index).ok_or_else(|| "Unknown song".to_string())?.clone();
        let root = self.root.clone();
        let (tx, rx) = mpsc::channel();
        self.exporting = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(export_song(&root, &track, &directory));
        });
        Ok(())
    }

    pub fn poll_export(&mut self) -> Option<Result<PathBuf, String>> {
        let result = match self.exporting.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("Music download worker disconnected".into()),
        };
        self.exporting = None;
        Some(result)
    }
}

impl Drop for Soundtrack {
    fn drop(&mut self) {
        if let Some(current) = &self.current {
            stop_sound(&current.sound);
        }
        if let Some(next) = &self.next {
            stop_sound(&next.sound);
        }
    }
}

fn export_song(root: &Path, track: &Track, directory: &Path) -> Result<PathBuf, String> {
    let bytes = verified_bytes(root, track)?;
    fs::create_dir_all(directory).map_err(|error| format!("Could not create Downloads folder: {error}"))?;
    let stem = format!("BlueCraft - {}", sanitize_filename(&track.title));
    for number in 1..=1000 {
        let filename = if number == 1 { format!("{stem}.ogg") } else { format!("{stem} ({number}).ogg") };
        let path = directory.join(filename);
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Could not save {}: {error}", track.title)),
        };
        let result = file.write_all(&bytes).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = result {
            let _ = fs::remove_file(&path);
            return Err(format!("Could not finish saving {}: {error}", track.title));
        }
        return Ok(path);
    }
    Err(format!("Too many existing downloads of {}", track.title))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static SERIAL: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "bluecraft-music-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn source() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/music")
    }

    #[test]
    fn catalog_rejects_unsafe_duplicate_and_unbounded_metadata() {
        let catalog = Catalog::load(&source()).unwrap();
        for file in [
            "../escape.ogg",
            "..\\escape.ogg",
            "/absolute.ogg",
            "C:drive.ogg",
            "sub/song.ogg",
            "song.wav",
            "song.ogg\0",
        ] {
            let mut invalid = catalog.clone();
            invalid.tracks[0].file = file.into();
            assert!(invalid.validate().is_err(), "accepted {file}");
        }
        let mut invalid = catalog.clone();
        invalid.tracks[0].seconds = f64::NAN;
        assert!(invalid.validate().is_err());
        invalid = catalog.clone();
        invalid.tracks[0].seconds = 1000.;
        assert!(invalid.validate().is_err());
        invalid = catalog.clone();
        invalid.tracks[1].title = invalid.tracks[0].title.to_uppercase();
        assert!(invalid.validate().is_err());
        invalid = catalog.clone();
        invalid.tracks[1].file = invalid.tracks[0].file.clone();
        assert!(invalid.validate().is_err());
        invalid = catalog.clone();
        invalid.tracks[0].sha256 = "bad".into();
        assert!(invalid.validate().is_err());
        invalid = catalog.clone();
        invalid.tracks.pop();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn all_ten_transitions_and_the_wrap_have_equal_power_and_actual_submission_clocks() {
        let mut clock = PlaylistClock::new(12.);
        clock.start(0, 160., 5.);
        assert_eq!(clock.dominant(4.), Some(0));
        assert_eq!(clock.gains(5.), (0., 0.));
        assert!((clock.gains(9.).0 - 1.).abs() < 0.00001);
        for index in 0..TRACK_COUNT {
            let current = clock.current.unwrap();
            let next = (index + 1) % TRACK_COUNT;
            let edge = current.started + current.seconds - 12.;
            assert!(!clock.preload_due(edge - PRELOAD_SECONDS - 0.01));
            assert!(clock.preload_due(edge - PRELOAD_SECONDS));
            assert!(!clock.transition_due(edge - 0.01));
            assert!(clock.transition_due(edge));
            // Loading being late does not advance the current title or invent an incoming voice.
            assert_eq!(clock.dominant(edge + 30.), Some(index));
            clock.start_incoming(next, 160. + next as f64, edge + 30.);
            assert_eq!(clock.dominant(edge + 30.), Some(index));
            for step in 0..=120 {
                let now = edge + 30. + f64::from(step) / 10.;
                let (a, b) = clock.gains(now);
                assert!((a * a + b * b - 1.).abs() < 0.00001);
                assert!((0. ..=1.).contains(&a) && (0. ..=1.).contains(&b));
            }
            assert_eq!(clock.dominant(edge + 36.), Some(next));
            assert!(!clock.finish(edge + 41.999));
            assert!(clock.finish(edge + 42.));
            assert_eq!(clock.current.unwrap().started, edge + 30.);
            assert_eq!(clock.dominant(edge + 42.), Some(next));
        }
        assert_eq!(clock.transitions, 10);
        assert_eq!(clock.starts, 11);
        assert_eq!(clock.current.unwrap().index, 0);
    }

    #[test]
    fn pause_mute_and_large_hitches_keep_the_running_song_clock() {
        let mut clock = PlaylistClock::new(12.);
        let mut level = Level::default();
        clock.start(0, 180., 0.);
        level.update(0.6, false, 1.);
        for second in 2..=40 {
            level.update(0.6, true, f64::from(second));
        }
        assert!((level.now - 0.21).abs() < 0.0001);
        for second in 41..=80 {
            level.update(0., true, f64::from(second));
        }
        assert!(level.now < 0.0001);
        assert_eq!(clock.current.unwrap().started, 0.);
        assert!(clock.transition_due(500.));
        clock.start_incoming(1, 180., 500.);
        assert_eq!(clock.phase(500.), 0.);
        level.update(0.6, false, 500.);
        assert!(level.now > 0.5);
        assert_eq!(clock.dominant(505.), Some(0));
        assert_eq!(clock.dominant(507.), Some(1));
        assert!(clock.finish(520.));
        assert_eq!(clock.current.unwrap().started, 500.);
        assert!(!clock.transition_due(520.));
    }

    #[test]
    fn actual_album_has_ten_valid_stereo_ogg_scores_and_precise_pcm_durations() {
        let root = source();
        let catalog = Catalog::load(&root).unwrap();
        let mut hashes = HashSet::new();
        for (index, track) in catalog.tracks.iter().enumerate() {
            assert!(hashes.insert(track.sha256.clone()), "songs must have distinct audio bytes");
            let prepared = decode(&root, index, track).unwrap();
            assert_eq!(prepared.index, index);
            assert!((prepared.seconds - track.seconds).abs() <= 0.05);
            let (rate, channels, samples) = vesper3d::viewer::devkit::synth::parse_wav(&prepared.wav).unwrap();
            assert_eq!((rate, channels), (RATE, 2));
            assert!(samples.iter().all(|sample| sample.is_finite()));
            let peak = samples.iter().fold(0f32, |peak, sample| peak.max(sample.abs()));
            assert!((0.02..0.75).contains(&peak), "{} peak {peak}", track.title);
        }
    }

    #[test]
    fn every_download_matches_source_and_repeated_downloads_never_overwrite() {
        let root = source();
        let catalog = Catalog::load(&root).unwrap();
        let directory = Temp::new();
        for track in &catalog.tracks {
            let first = export_song(&root, track, &directory.0).unwrap();
            let second = export_song(&root, track, &directory.0).unwrap();
            assert_ne!(first, second);
            assert_eq!(fs::read(&first).unwrap(), fs::read(root.join(&track.file)).unwrap());
            assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
            assert!(second.file_name().unwrap().to_string_lossy().ends_with(" (2).ogg"));
        }
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 20);
        let blocked = directory.0.join("a-file-not-a-directory");
        fs::write(&blocked, b"keep").unwrap();
        assert!(export_song(&root, &catalog.tracks[0], &blocked).is_err());
        assert_eq!(fs::read(blocked).unwrap(), b"keep");
    }

    #[test]
    fn corrupt_source_is_refused_before_decode_or_download_creates_a_file() {
        let root = Temp::new();
        let catalog = Catalog::load(&source()).unwrap();
        let track = &catalog.tracks[0];
        fs::write(root.0.join(&track.file), b"damaged audio").unwrap();
        assert!(verified_bytes(&root.0, track).unwrap_err().contains("checksum"));
        assert!(decode(&root.0, 0, track).is_err());
        let downloads = root.0.join("downloads");
        assert!(export_song(&root.0, track, &downloads).is_err());
        assert!(!downloads.exists());
    }

    #[test]
    fn intentional_global_mute_prepares_no_audio_but_still_exports_every_song() {
        let root = source();
        let catalog = Catalog::load(&root).unwrap();
        let mut soundtrack = Soundtrack::new(root, catalog, true);
        soundtrack.update(1., false);
        assert!(soundtrack.loading.is_none() && soundtrack.current.is_none());
        assert_eq!(soundtrack.current_track(), None);
        assert!(soundtrack.verify_playback().is_ok());
        let directory = Temp::new();
        for index in 0..TRACK_COUNT {
            soundtrack.export_to(index, directory.0.clone()).unwrap();
            assert!(soundtrack.export_to(index, directory.0.clone()).is_err());
            let deadline = Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if let Some(result) = soundtrack.poll_export() {
                    assert!(result.is_ok());
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
        }
        assert!(!soundtrack.export_busy());
    }
}
