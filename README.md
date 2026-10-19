# BlueCraft

Build a little world.

A small Minecraft-style creative voxel sandbox made with [BlueEngine](../bluecraft-engine).
Explore a block world in first person, break blocks and build with grass, dirt, stone,
sand, wood, leaves, planks, brick and glass. The simulation runs at a fixed 60 Hz,
and BlueEngine owns save and load storage.

The Linux package is ready to run from this project directory:

```sh
./dist/bluecraft
```

The executable also works from another working directory; use its full path,
`/home/kevin/bluecraft/dist/bluecraft`. The package includes the game's icons and
assets. Windows executable and installer verification remain pending.

| Control | Action |
| --- | --- |
| WASD | Move |
| Mouse | Look |
| Space | Jump |
| Shift | Sprint |
| Left click | Break targeted block |
| Right click | Place selected block |
| Mouse wheel / 1–9 | Select material |
| Middle click | Pick targeted material |
| F5 / F9 | Save / load |
| E | Show help |
| Esc | Pause / resume |
| F | Toggle fullscreen |

The palette order is grass, dirt, stone, sand, wood, leaves, planks, brick, glass.

Normal play automatically resumes your previous world. BlueCraft saves automatic
progress every 15 seconds and when you exit cleanly. F5 and F9 use a separate
manual checkpoint, so automatic saves preserve that checkpoint.

Start with `--fresh` for a new world that leaves automatic progress untouched, or
`--seed N` for a deterministic world with the same protection. These runs do not
save automatically; F5 still saves a manual checkpoint when requested.

## Develop

Keep the pinned BlueEngine checkout beside this project at `../bluecraft-engine`
(upstream revision `e7ba8467fe9043527579fc8fac3de7818dddea3a`, fetched October 19, 2026).
Rust and the native platform's
build dependencies are required. From this directory:

```sh
cargo test --no-default-features
scripts/blue dev
# Start a fresh world, or explore a repeatable seed:
scripts/blue dev -- --fresh
scripts/blue dev -- --seed 42
```

Linux is declared as a local development target alongside the Windows release
target. For a capture using the engine's shared lifecycle:

```sh
scripts/blue dev -- --capture .blue-check/captures --frames 30,120 --exit-after 130
```

Verification status and remaining target checks are recorded in [STATUS.md](STATUS.md).

## Package and Windows installer

BlueEngine's release distribution target is a native Windows x64 EXE installer.
This project also declares Linux for local development and package verification.
On either native host, `scripts/blue ship --no-install` builds and checks that
host's package in `dist/` without installing a shortcut. On Windows, the equivalent
Python entry point and installer commands are:

```text
python scripts/ship.py ship --no-install
python scripts/ship.py installer prepare
python scripts/ship.py installer build --iscc "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
python scripts/ship.py installer test --stage "ABSOLUTE PREPARED STAGE PATH"
```

Run the last command on an isolated Windows account; it exercises installation,
upgrade and uninstall. Keep `scripts/installer.json`'s AppId stable across releases.
Building a package does not install the game; `scripts/blue ship` additionally
installs and checks its desktop shortcut when installation is wanted.

The Linux package is for local development. Linux checks cannot certify a Windows
executable or installer. Windows packaging needs the Windows toolchain and SDK resources;
installer compilation needs Inno Setup 6. `installer prepare` can run on Linux only
after a Windows package exists. Physical controls, graphics and audible output
require native device review using [assets/native-review.json](assets/native-review.json).
