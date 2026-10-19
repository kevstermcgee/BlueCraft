# BlueCraft release status

[BlueCraft 0.1.0 is published](https://github.com/kevstermcgee/BlueCraft/releases/tag/v0.1.0)
as a Windows x64 EXE installer. Its download page and complete game guide are at
https://kevstermcgee.github.io/BlueEngineGames/games/bluecraft/.

The release was built from game source `9d1294c5dd39af3fecddcd3b2f2b5220722d1dd2`
and BlueEngine `e7ba8467fe9043527579fc8fac3de7818dddea3a`.
The published installer is 3,175,937 bytes with SHA-256
`ecd0736331e6a6f8b748bc8a2a9e3302cd48675e31e1e8aaf2da6f08e14fdb49`.
The public download was retrieved anonymously and its exact bytes verified.

[Windows verification run](https://github.com/kevstermcgee/BlueCraft/actions/runs/39300126196)
passed formatting, all 22 game tests, Clippy, content checks, executable resources,
and isolated declared-file package smoke. The exact installer passed installation,
upgrade, shortcut target and working-directory checks, save/settings retention,
and uninstallation. Both Windows smoke captures were visually inspected.
Checksums and build provenance accompany the public release.

Linux development checks also passed: the game tests and Clippy, full project
checks, release package hashes and isolated launch. X11 input events exercised
movement, mouse look, jump, material selection, mining, placement, manual save/load,
help and pause/resume. A separate run of the release executable verified periodic
autosave, save on clean exit and automatic continuation of position and selection.

The published guide has three distinct, visually inspected gameplay captures,
verified controls, actual sandbox rules and the original creation request.
There are no visible player models or NPCs in this first-person sandbox.

Hardware graphics, physical input devices and human-audible output remain outside
these automated checks. Review criteria are in `assets/native-review.json`.
