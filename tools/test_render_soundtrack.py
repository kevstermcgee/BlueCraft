"""Fault probes for preserving a completed album during an offline rebuild."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("render_soundtrack", Path(__file__).with_name("render_soundtrack.py"))
soundtrack = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(soundtrack)


class PreserveCompletedAlbum(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.music = self.root / "music"
        self.music.mkdir()
        self.original = {"catalog.json": b'{"previous":"verified"}\n',
                         "01-first.ogg": b"previous completed first song",
                         "02-second.ogg": b"previous completed second song"}
        for filename, content in self.original.items():
            (self.music / filename).write_bytes(content)
        self.tracks = [{"file": "01-first.ogg", "title": "First"},
                       {"file": "02-second.ogg", "title": "Second"}]

    def fake_renderer(self, command, **_kwargs):
        directory = Path(command[1])
        (directory / "catalog.json").write_text(json.dumps({"tracks": self.tracks}))

    def assert_previous_album(self):
        self.assertEqual({p.name: p.read_bytes() for p in self.music.iterdir()}, self.original)

    def test_encoder_failure_after_one_new_song_preserves_all_previous_outputs(self):
        encoded = []

        def fake_run(command, **_kwargs):
            if command[0] == "ffmpeg":
                encoded.append(command[-1])
                Path(command[-1]).write_bytes(b"new encoded data")
                if len(encoded) == 2:
                    raise subprocess.CalledProcessError(1, command, stderr="injected encoder failure")

        with patch.object(soundtrack, "MUSIC", self.music), \
             patch.object(soundtrack, "run", side_effect=fake_run), \
             patch.object(soundtrack.subprocess, "run", side_effect=self.fake_renderer), \
             patch.object(soundtrack, "verify") as verify:
            with self.assertRaises(subprocess.CalledProcessError):
                soundtrack.encode()
        self.assertEqual(len(encoded), 2)
        verify.assert_not_called()
        self.assert_previous_album()
        self.assertEqual(list(self.root.iterdir()), [self.music])

    def test_validation_failure_preserves_previous_album(self):
        def fake_run(command, **_kwargs):
            if command[0] == "ffmpeg":
                Path(command[-1]).write_bytes(b"new encoded data")

        with patch.object(soundtrack, "MUSIC", self.music), \
             patch.object(soundtrack, "run", side_effect=fake_run), \
             patch.object(soundtrack.subprocess, "run", side_effect=self.fake_renderer), \
             patch.object(soundtrack, "verify", side_effect=ValueError("injected decode failure")) as verify:
            with self.assertRaisesRegex(ValueError, "injected decode failure"):
                soundtrack.encode()
        verify.assert_called_once()
        self.assertNotEqual(verify.call_args.args[1], self.music)
        self.assert_previous_album()

    def test_publish_failure_restores_previous_directory(self):
        staged = self.root / "staged"
        staged.mkdir()
        (staged / "catalog.json").write_bytes(b"new catalog")
        rename = Path.rename

        def fault(source, target):
            if source == staged:
                raise OSError("injected publish failure")
            return rename(source, target)

        with patch.object(Path, "rename", fault):
            with self.assertRaisesRegex(OSError, "injected publish failure"):
                soundtrack.publish_music(staged, self.music)
        self.assert_previous_album()
        self.assertEqual(set(self.root.iterdir()), {self.music, staged})

    def test_successful_publish_replaces_album_as_one_complete_directory(self):
        staged = self.root / "staged"
        staged.mkdir()
        (staged / "catalog.json").write_bytes(b"complete new catalog")
        (staged / "new.ogg").write_bytes(b"complete new song")
        soundtrack.publish_music(staged, self.music)
        self.assertEqual({p.name: p.read_bytes() for p in self.music.iterdir()},
                         {"catalog.json": b"complete new catalog", "new.ogg": b"complete new song"})
        self.assertEqual(list(self.root.iterdir()), [self.music])


if __name__ == "__main__":
    unittest.main()
