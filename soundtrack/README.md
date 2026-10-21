# BlueCraft — Quiet Worlds

Ten original ambient compositions, written for building, wandering and returning home.
The album uses felt-like piano, muted wooden keys, glass keys, soft dulcimer and broad,
quiet pads. All instrument sounds are synthesized from the committed score; no
recordings, melodies or samples from Minecraft or any other game are included.

| Title | Duration | Harmony / pace | Character |
| --- | --- | --- | --- |
| Open Meadow | 3:12 | F major, 60 BPM, 4/4 | The original meadow motifs, expanded with a contrasting low-register passage. |
| Glasswater | 2:48 | C major, 68 BPM, 3/4 | Lilting glass-key reflections and low piano answers. |
| Below the Ferns | 2:54 | D minor, 56 BPM, 4/4 | Descending shaded piano phrases and damped wooden replies. |
| Clouds in Pieces | 3:05 | G major, 64 BPM, 4/4 | Wide rising intervals, changing pad inversions and a central high-glass passage. |
| Lantern Current | 2:47 | A minor, 72 BPM, 4/4 | Offbeat wooden plucks and interlocking figures. |
| A Place to Rest | 3:00 | F major, 54 BPM, 4/4 | Long low piano tones, a nearly still pad and generous rests. |
| Copper Dawn | 3:00 | C major, 66 BPM, 4/4 | Paired dulcimer figures, a rising melody and piano responses. |
| Far Hills | 3:08 | G major, 58 BPM, 3/4 | A spacious climbing and falling piano line, with distant bell endings. |
| Rain on Leaves | 2:55 | D Dorian, 62 BPM, 4/4 | Irregular glass droplets, damped keys and modal harmony. |
| Homeward Light | 3:08 | C major, 60 BPM, 4/4 | Falling piano intervals and a broad resolving coda. |

Each composition has three explicitly written melodic phrases, its own progression,
section order and accompaniment. They are not transposed or seeded versions of one
template. The variations in note spacing, register, meter, density and instrumentation
give each piece an individual identity while preserving a common palette.

The album lasts about 30 minutes before overlaps. Its ordered sequence uses 12-second
equal-power crossfades, including the return from Homeward Light to Open Meadow. Every
piece has a gentle entry and a compatible C/G/D pad at its ending, so there is no hard
file boundary or abruptly changing bass underneath the overlap. These pads retain quiet
energy through the join; there is no inserted silent interval. Standalone downloads
have their own short opening and ending fades.

## Rebuilding and checking

```sh
python3 tools/render_soundtrack.py
python3 tools/render_soundtrack.py --check
```

The first command compiles the dependency-free offline Rust renderer, renders temporary
44.1-kHz stereo PCM16 masters, encodes Vorbis quality 5 files with FFmpeg, writes the
runtime catalog and verifies the resulting compressed files. Rust and FFmpeg must be
available; no Python packages or network access are required. Temporary PCM masters
are removed automatically. The game ships the encoded files and performs no score
synthesis at startup. Downloads copy those exact original Ogg files, with embedded
title, artist and album tags.

Authoritative compositions and instrument envelopes live in
[`tools/render_soundtrack.rs`](../tools/render_soundtrack.rs). The encoded tracks and
runtime metadata live in [`assets/music`](../assets/music). `catalog.json` records their
SHA-256 digests, exact durations, tempo, meter, key, lead instrument and authored note
counts. Re-render when changing score or instrument data. The WAV synthesis is
deterministic on the same platform; encoding bytes can vary between FFmpeg versions.

[`analysis.json`](analysis.json) records checks against the actual Vorbis-decoded files:
format, checksum, duration, peak headroom, RMS balance, DC, sample discontinuities and
all ten adjacent transitions. Transition measurements include the 12 one-second RMS
windows, avoiding a test that checks only the single endpoint sample. This analysis
checks signal integrity and consistent energy; perceived musical quality, speaker
playback and controller interaction require separate human review.

## Native playback limits

Vorbis decoding happens off the game thread. The current native backend still
converts a complete prepared PCM file synchronously when it is submitted, so a
new song can briefly stall rendering while preloading. Preloading begins 45 seconds
before the 12-second transition, and the already-playing audio continues on its
own thread. The playlist uses real elapsed time after playback submissions to
keep its timing through those stalls. The full-length software run verified the
first transition and release of the outgoing voice; physical listening and output
device behavior remain unverified.
