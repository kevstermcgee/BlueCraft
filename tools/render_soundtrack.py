#!/usr/bin/env python3
"""Render/encode the original BlueCraft album, or verify its shipped audio without a device.

Requires Rust and FFmpeg only. No Python packages, network, or realtime synthesis.
    python3 tools/render_soundtrack.py
    python3 tools/render_soundtrack.py --check
"""
from __future__ import annotations

import argparse
import array
import hashlib
import json
import math
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
MUSIC = ROOT / "assets" / "music"
SOURCE = ROOT / "tools" / "render_soundtrack.rs"
REPORT = ROOT / "soundtrack" / "analysis.json"
RATE = 44_100
FADE = 12


def run(command: list[str], *, binary: bool = False) -> subprocess.CompletedProcess:
    return subprocess.run(command, cwd=ROOT, check=True, capture_output=True, text=not binary)


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def publish_music(staged: Path, destination: Path) -> None:
    """Publish a complete directory, restoring the previous album if the swap fails.

    Both rename targets are absent when used, which also works on Windows. Keep the
    backup outside the disposable render workspace so a failed rollback cannot delete
    the only remaining copy of the previous completed album during workspace cleanup.
    """
    backup = Path(tempfile.mkdtemp(prefix=".bluecraft-music-backup-", dir=destination.parent))
    backup.rmdir()
    previous = destination.exists()
    try:
        if previous:
            destination.rename(backup)
        staged.rename(destination)
    except BaseException:
        if backup.exists():
            try:
                backup.rename(destination)
            except BaseException as restore_error:
                raise RuntimeError(f"Music publish and rollback failed; previous album preserved at {backup}") from restore_error
        raise
    if previous:
        try:
            shutil.rmtree(backup)
        except OSError:
            print(f"Published verified music; previous album backup remains at {backup}", file=sys.stderr)


def encode() -> tuple[dict, dict]:
    MUSIC.parent.mkdir(parents=True, exist_ok=True)
    # A workspace on the destination filesystem permits directory renames after every
    # encoded track, the catalog and all transitions have passed verification.
    with tempfile.TemporaryDirectory(prefix=".bluecraft-score-", dir=MUSIC.parent) as temp:
        temp = Path(temp)
        staged = temp / "music"
        staged.mkdir()
        renderer = temp / ("renderer.exe" if sys.platform == "win32" else "renderer")
        run(["rustc", "-O", "--edition=2021", str(SOURCE), "-o", str(renderer)])
        print("Rendering ten authored scores offline...", flush=True)
        subprocess.run([str(renderer), str(temp)], cwd=ROOT, check=True)
        catalog = json.loads((temp / "catalog.json").read_text())
        for index, track in enumerate(catalog["tracks"], 1):
            target = staged / track["file"]
            run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i",
                 str(temp / Path(track["file"]).with_suffix(".wav")),
                 "-fflags", "+bitexact", "-flags:a", "+bitexact", "-map_metadata", "-1",
                 "-c:a", "libvorbis", "-q:a", "5", "-ar", str(RATE), "-ac", "2",
                 "-metadata", f"title={track['title']}", "-metadata", "artist=BlueEngine Games",
                 "-metadata", "album=BlueCraft - Quiet Worlds", "-metadata", f"track={index}/10",
                 "-metadata", "comment=Original BlueCraft composition; no imported audio samples.",
                 str(target)])
            track["sha256"] = sha(target)
            track["bytes"] = target.stat().st_size
        (staged / "catalog.json").write_text(json.dumps(catalog, indent=2) + "\n")
        report = verify(catalog, staged)
        publish_music(staged, MUSIC)
    return catalog, report


def probe(path: Path) -> dict:
    return json.loads(run(["ffprobe", "-v", "error", "-show_streams", "-show_format",
                          "-of", "json", str(path)]).stdout)


def measurements(path: Path) -> dict:
    result = run(["ffmpeg", "-hide_banner", "-nostats", "-i", str(path), "-af",
                  "astats=metadata=0:reset=0", "-f", "null", "-"])
    overall = result.stderr.rsplit("Overall", 1)[-1]

    def metric(name: str) -> float:
        match = re.search(re.escape(name) + r":\s+([-+\deE.]+)", overall)
        if not match:
            raise ValueError(f"Missing {name} for {path.name}")
        return float(match.group(1))

    return {"peak": 10 ** (metric("Peak level dB") / 20),
            "rms": 10 ** (metric("RMS level dB") / 20),
            "dc": metric("DC offset"), "max_sample_step": metric("Max difference")}


def transition(current: Path, following: Path, seconds: float) -> dict:
    # FFmpeg qsin is sin(pi*t/2), matching the player's equal-power envelopes. Analyze the
    # actual Vorbis-decoded overlap, including codec delay and file edges, for every neighbour.
    result = run(["ffmpeg", "-v", "error", "-ss", f"{seconds - FADE:.9f}", "-t", str(FADE),
                  "-i", str(current), "-t", str(FADE), "-i", str(following), "-filter_complex",
                  f"[0:a]asetpts=PTS-STARTPTS[a];[1:a]asetpts=PTS-STARTPTS[b];"
                  f"[a][b]acrossfade=d={FADE}:c1=qsin:c2=qsin",
                  "-f", "f32le", "-acodec", "pcm_f32le", "-"], binary=True)
    samples = array.array("f")
    samples.frombytes(result.stdout)
    if sys.byteorder != "little":
        samples.byteswap()
    assert abs(len(samples) / 2 / RATE - FADE) < 0.03, "Truncated transition"
    windows = []
    for at in range(0, len(samples) - RATE, RATE * 2):
        window = samples[at:at + RATE * 2]
        windows.append(math.sqrt(sum(v * v for v in window) / len(window)))
    peak = max(abs(v) for v in samples)
    max_step = max(abs(samples[i] - samples[i - 2]) for i in range(2, len(samples)))
    rms = math.sqrt(sum(v * v for v in samples) / len(samples))
    assert peak < 0.46, f"Transition peak too high: {current.name}"
    assert min(windows) > 0.005, f"Transition contains an energy gap: {current.name}"
    assert max(windows) / min(windows) < 5.0, f"Abrupt transition dynamics: {current.name}"
    assert max_step < 0.06, f"Large transition sample step: {current.name}"
    return {"from": current.stem, "to": following.stem, "seconds": len(samples) / 2 / RATE,
            "peak": round(peak, 7), "rms": round(rms, 7),
            "max_sample_step": round(max_step, 7), "one_second_rms": [round(v, 7) for v in windows],
            "loudest_to_quietest_window_ratio": round(max(windows) / min(windows), 5)}


def verify(catalog: dict, music: Path | None = None) -> dict:
    music = MUSIC if music is None else music
    tracks = catalog["tracks"]
    assert catalog["version"] == 1 and catalog["sample_rate"] == RATE
    assert catalog["crossfade_seconds"] == FADE and len(tracks) == 10
    for field in ["id", "title", "file", "sha256"]:
        assert len({t[field] for t in tracks}) == 10, f"Duplicate {field}"
    analysis = []
    for track in tracks:
        path = music / track["file"]
        assert path.parent == music and path.suffix == ".ogg"
        assert sha(path) == track["sha256"], f"Checksum mismatch: {path.name}"
        metadata = probe(path)
        streams = metadata["streams"]
        assert len(streams) == 1 and streams[0]["codec_name"] == "vorbis"
        stream = streams[0]
        assert int(stream["sample_rate"]) == RATE and stream["channels"] == 2
        seconds = float(stream["duration"])
        assert 150 < seconds <= 195 and abs(seconds - track["seconds"]) < 0.001
        assert stream["tags"]["title"] == track["title"]
        stats = measurements(path)
        assert 0.08 < stats["peak"] < 0.34, f"Peak: {path.name} {stats}"
        assert 0.025 < stats["rms"] < 0.065, f"Loudness: {path.name} {stats}"
        assert abs(stats["dc"]) < 0.0001, f"DC bias: {path.name}"
        assert stats["max_sample_step"] < 0.06, f"Sample discontinuity: {path.name}"
        analysis.append({"id": track["id"], "seconds": seconds, "bytes": path.stat().st_size,
                         "sha256": track["sha256"], **{k: round(v, 8) for k, v in stats.items()}})
        print(f"Verified {track['title']}: {seconds:.2f}s, peak {stats['peak']:.3f}, RMS {stats['rms']:.4f}", flush=True)
    loudness_ratio = max(t["rms"] for t in analysis) / min(t["rms"] for t in analysis)
    assert loudness_ratio < 1.35, "Album loudness is inconsistent"
    transitions = []
    for index, track in enumerate(tracks):
        following = tracks[(index + 1) % len(tracks)]
        transitions.append(transition(music / track["file"], music / following["file"], track["seconds"]))
    return {"version": 1, "method": "Actual Vorbis decode; 12-second equal-power adjacent crossfades, including last to first.",
            "limitations": "Numeric signal and transition checks do not establish musical quality or physical audibility.",
            "renderer_sha256": sha(SOURCE), "encoder": run(["ffmpeg", "-version"]).stdout.splitlines()[0],
            "total_seconds": round(sum(t["seconds"] for t in tracks), 6),
            "total_bytes": sum(t["bytes"] for t in analysis), "album_rms_ratio": round(loudness_ratio, 6),
            "tracks": analysis, "transitions": transitions}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Verify shipped files without rerendering.")
    args = parser.parse_args()
    if args.check:
        catalog = json.loads((MUSIC / "catalog.json").read_text())
        report = verify(catalog)
    else:
        catalog, report = encode()
    REPORT.parent.mkdir(exist_ok=True)
    REPORT.write_text(json.dumps(report, indent=2) + "\n")
    print(f"All 10 tracks and all 10 transitions passed; {report['total_seconds'] / 60:.2f} minutes, "
          f"{report['total_bytes'] / 1024 / 1024:.2f} MiB.")


if __name__ == "__main__":
    main()
