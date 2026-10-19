# BlueCraft status

The playable Linux development package is [dist/bluecraft](dist/bluecraft)
(about 3.8 MiB). Run `./dist/bluecraft` from this directory, or launch the
executable by its full path from another working directory.

BlueCraft includes a seeded voxel world, first-person movement and jumping,
block mining and placement, nine materials, target highlighting, a hotbar,
help/pause menus and engine-owned save states. Normal play resumes automatic
progress and saves every 15 seconds and on clean exit. F5/F9 use a separate
manual checkpoint. `--fresh` and `--seed N` keep automatic progress untouched.

The pinned engine is upstream revision
`e7ba8467fe9043527579fc8fac3de7818dddea3a`, fetched October 19, 2026. Linux is
declared for local development/package checks; Windows is the release target.

Completed evidence:

- All 22 tests passed: 12 rules tests, eight determinism/snapshot tests, one
  identity test and one mesh-winding test.
- Headless and client all-target Clippy checks passed.
- `scripts/blue ship --no-install` passed Linux identity/icon, asset closure,
  package hash and isolated declared-file launch checks. The package includes
  BlueCraft's own icons. [dist/ship.json](dist/ship.json) records its SHA-256
  file inventory and shipping evidence.
- The isolated smoke captures at
  [.blue-check/smoke-20261019T085510192036](.blue-check/smoke-20261019T085510192036)
  (frames 30 and 90) were visually inspected.
- X11 XTest events exercised the native keyboard/mouse path without scripted
  gameplay input. The
  [input report](.blue-check/real-device-input-v3.json) records 24 movement frames,
  mouse look, jump, selection, one mined block, one placed block, one successful
  F5 save and one successful F9 load. Captures also checked help and a paused
  simulation tick.

Windows executable resources, native package launch and the x64 Inno Setup
installer remain unverified and require a Windows host. Physical keyboard/mouse,
hardware graphics and human-audible output have not been verified. XTest events
and software captures provide native input/rendering evidence only. Review
criteria are declared in [assets/native-review.json](assets/native-review.json).
