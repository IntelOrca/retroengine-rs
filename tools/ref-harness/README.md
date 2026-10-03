# Reference harness (RSDKv5U, v4-legacy data)

This directory builds and runs a deterministic C++ reference of the engine behaviour the Rust
port imitates, so fidelity bugs (collision, tile rendering, draw order, transitions) can be found
by frame/state comparison instead of guesswork.

The reference is [RSDKv5-Decompilation](https://github.com/RSDKModding/RSDKv5-Decompilation) at
commit `43d426f8427c5553dab72afc02354e275f9ace48`, built with `RETRO_REVISION=3` (v5U) and the
SDL2 subsystem. It detects the unpacked RSDKv4 asset folders (`Data/Game/GameConfig.bin` with a
non-`CFG` signature) and runs them through its **Legacy v4** path, which is the same code path the
Rust port targets (`retroengine <assets> --scene ... --headless`).

Everything upstream (engine, SDL2, BLAKE3) is fetched at build time from pinned revisions; nothing
upstream is vendored in this repository, and no game assets are stored here.

## Status (verified on this machine)

Verified end-to-end (2026-10-03, Linux x86_64, GCC 15.2, CMake 4.4.3, Ninja 1.13.2, no sudo, no
system SDL2, no `pkg-config`):

| Run | Result |
|---|---|
| `build.sh` from a clean build dir | builds SDL2 2.32.10 and BLAKE3 1.5.5 (pinned by commit SHA), patched RSDKv5U + `hash565` |
| S1 `Title` act 1, 600 frames | 600 records, 236 unique framebuffers, exit 0 |
| S1 `Zone01` act 1, 600 frames (idle / hold RIGHT) | 600 records each, 495 / 380 unique framebuffers |
| Two identical runs (`cmp records.jsonl`) | byte-identical (deterministic) |
| S2 `Title` and `Zone01` act 1, 120 frames | boot and render (S2 covered by the same loader) |
| C reference vs Rust `--dump-frames` PNGs, S1/S2 `Zone01` idle | **600/600 identical** |
| C reference vs Rust, S1/S2 `Zone01` hold-RIGHT (`zone01_right.input`) | **600/600 identical** |
| C reference vs Rust, S1/S2 `Zone01` RIGHT held from frame 400 (`zone01_right400.input`) | **600/600 identical** |
| C reference vs Rust, S1/S2 `Title` -> `Zone01` START (A held from frame 800, `title_start.input`) | **1200/1200 identical** |

## Requirements

* Linux (the patches target `platforms/Linux.cmake`; other platforms could be added the same way)
* `git`, `cmake >= 3.10`, `ninja`, `cc`, `c++` (GCC/Clang), network access to github.com
* `python3` (diff tooling only)
* No sudo, no system SDL2, no `pkg-config`, no audio/video device required

## Build

```sh
# default build dir: ~/.cache/ref-harness
tools/ref-harness/build.sh

# or keep heavy builds on a scratch volume:
REF_HARNESS_BUILD=/tmp/opencode/refbuild tools/ref-harness/build.sh
```

The script writes `$REF_HARNESS_BUILD/env.sh` (binary + helper paths) used by `run.sh`. It is
re-runnable: it resets the engine checkout, re-applies `patches/*.patch` and rebuilds.

## Run a scene

```sh
tools/ref-harness/run.sh \
    --game S1 --scene Zone01 --act 1 --frames 600 \
    --input tools/ref-harness/testdata/zone01_right.input \
    --out /tmp/opencode/refrun/zone01-right \
    --ppm 155,240            # optional keyframe PPM dumps
```

Outputs under `--out` (default `tools/ref-harness/out/<game>-<scene>-<act>`, gitignored):

* `records.jsonl` – one record per processed frame (format below)
* `ppm/frame_%04d.ppm` – optional binary PPM keyframes (RGB888)
* `run/` – scratch working directory (symlink to the game's `Data/`, per-run copies of save files,
  generated `Settings.ini`, engine `log.txt`). The asset folder is never written to.
* `run.log` – engine stdout/stderr

The reference is launched as `RSDKv5U stage=<scene> scene=<act>` from the scratch directory, with
`SDL_VIDEODRIVER=offscreen SDL_AUDIODRIVER=dummy`, so it runs fully headless.

`--game-type` defaults to `0` (standalone/`USE_STANDALONE` scripts), matching the Rust port's
default; pass `--game-type 1` for Origins (`USE_ORIGINS`) scripts.

## Scripted input

`--input` accepts exactly the Rust `retro-input 1` format (see
`crates/retro-input/src/scripted.rs`), so the same file drives both engines. Only player 1's
button field is used (axes/touches are parsed and ignored):

```text
retro-input 1
seed 1592594996
0 - 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -
60 RIGHT 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -
```

* Frame numbers must be sequential from 0; the header and `seed` line are optional only in that
  `seed` may be omitted.
* Polling past the end of the file repeats the last frame, so a 61-line file holds RIGHT forever.
* Button names: `UP DOWN LEFT RIGHT A B C X Y Z START SELECT`, joined with `|`, or `-`, a
  decimal mask, or a `0x` mask. `L`/`R` are rejected (`bad buttons`): rev03 `ControllerState`
  has no L/R fields and the harness never injects them, so accepting the tokens would silently
  do nothing.
* `seed` (or `--seed`/`REF_HARNESS_SEED`, which take precedence) seeds both libc `rand()` and
  `Engine.randSeed`, so script `Rand()` calls are reproducible.

### Tick alignment

Input lines are indexed by **absolute engine tick**: line `N` is applied on record `N`. The
reference's `InjectInput` reads `inputMasks[frame]`, and record 0 is the `STAGEMODE_LOAD` tick,
which never calls `ProcessInput`; line 0 is therefore unused. The Rust port performs that load
tick in `Engine::load` and its first `run_frame` (record 1) consumes line 1. Scene switches
trigger further `STAGEMODE_LOAD` ticks, which likewise do not call `ProcessInput` and do not
consume their line.

Press edges are derived against the previously *processed* line (upstream keeps `down` in
`controller[]` and clears only `press`), so a button that becomes held on a skipped load-tick
line still reports a press on the next processed line. This is what made A-at-800 START and
RIGHT-at-400 replays line up exactly; an off-by-one here is invisible to inputs that hold a
button continuously (idle/hold-RIGHT) but shifts every press edge by one tick.

`testdata/` contains text-only inputs used by the verified runs:

* `title_idle.input` – header-only, idle (repeat)
* `title_start.input` – Title screen, A held from line 800 (START -> Zone01)
* `zone01_idle.input` – header-only, idle (repeat)
* `zone01_right.input` – idle through frame 59, then RIGHT held
* `zone01_right400.input` – idle through frame 399, then RIGHT held

## Record stream format (`records.jsonl`)

One JSON object per line, one line per `ProcessEngine` tick (frame 0 is the scene-load tick):

```json
{"f":0,"ver":4,
 "engine":{"state":0,"cat":0,"list":60},
 "catname":"Presentation","folder":"Zone01","sceneid":"1","scenename":"_RSDK_SCENE",
 "fb":{"w":424,"h":240,"blake3":"df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b"},
 "v4":{"gmode":1,"smode":2,"plr":0,"scroll":[0,0],"shake":[0,0],"lag":0,
   "cameras":[{"x":80,"y":944,"target":0,"adjustY":0,"enabled":1,"style":0},{...}],
   "player":[slot,type,name,propertyValue,groupID,xpos,ypos,xvel,yvel,speed,state,angle,scale,rotation,alpha,
             animationTimer,animationSpeed,priority,drawOrder,direction,inkEffect,animation,prevAnimation,frame,
             collisionMode,collisionPlane,gravity,controlMode,controlLock,values[0..47]],
   "ents":[[...same layout...],...]}}
```

When a scene is selected directly (`run.sh` passes `stage=<folder> scene=<act>`), the engine appends
a synthetic `_RSDK_SCENE` entry to the scene list, which is why `list` is one past the shipped
scenes and `scenename` is `_RSDK_SCENE`. The reference seeds `stage.listPos` with that synthetic
entry (`listPos = totalSceneCount`), while the Rust port points `stage.activeList`/`listPos` at
the real `GameConfig` entry for the requested scene. Scripts that read those variables
(`ActFinish`, `SignPost`, `TitleCard`, `DeathEvent`, `Start`, `CheckCurrentStageFolder` and the
`LoadStage` reload path) would observe a different list position if they compared it against a
shipped index. The verified windows do not exercise that comparison: idle and RIGHT-from-400
never leave the scene, the S1 hold-RIGHT death/game-over reloads the same entry, and the Title
START path overwrites `stage.activeList`/`listPos` in `Start.txt` before calling `LoadStage`.

* `fb.blake3` – BLAKE3 of the **visible** `w × h` RGB565 framebuffer, rows contiguous,
  little-endian u16. The C reference hashes `screens[0].frameBuffer` after `ProcessEngine()`.
  A Rust-side emitter only needs the visible pixels; padding (`pitch > width`) is excluded.
* Positions/velocities are 16.16 fixed point (raw ints), matching the engine's internal format.
* `v4.player` is the entity at `playerListPos`, always with its 48 `values`.
* `v4.ents` lists every entity with `type != 0` (plus the player slot). Set
  `REF_HARNESS_ENT_VALUES=1` to also dump all 48 `values` per entity (large).
* `cameras` are `Legacy::cameras[0..1]`; `scroll`/`shake`/`lag` are the legacy camera globals.
* Names/strings are JSON-escaped.

Environment variables honoured by the binary (normally set by `run.sh`):
`REF_HARNESS_DUMP`, `REF_HARNESS_INPUT`, `REF_HARNESS_FRAMES`, `REF_HARNESS_SEED`,
`REF_HARNESS_PPM_DIR`, `REF_HARNESS_PPM_FRAMES`, `REF_HARNESS_PPM_EVERY`, `REF_HARNESS_ENT_VALUES`.

## Diffing

### Reference vs reference

```sh
python3 tools/ref-harness/diff_records.py A.jsonl B.jsonl [--offset N]
```

Reports the first divergent frame and field (framebuffer hash, engine/scene fields, v4 camera /
player / entity slot fields). Exit status 1 when the streams differ. Example (idle vs
hold-RIGHT, S1 Zone01): first difference at frame 187, `fb.blake3`.

### Reference vs Rust port

The Rust engine's `--hash-every-frame` prints a composite state hash, not a framebuffer hash, so
compare against `--dump-frames` PNGs:

```sh
cargo build --release -p retro-engine
target/release/retroengine /home/ted/projects/assets/S1 \
    --scene Zone01 --act 1 --headless --frames 600 \
    --input tools/ref-harness/testdata/zone01_right.input \
    --dump-frames /tmp/rust-frames --dump-frame-every 1 \
    --hash-every-frame --seed 1592594996 --mute

python3 tools/ref-harness/compare_frames.py \
    /tmp/opencode/refrun/zone01-right/records.jsonl /tmp/rust-frames \
    --hash565 "$REF_HARNESS_BUILD/hash565"
```

`compare_frames.py` decodes each PNG, packs it back to RGB565 exactly, and hashes it with the
same BLAKE3 as `fb.blake3`. It reports the first divergent frame, and
`--image A.ppm B.png` reports the differing-pixel count and bounding box for two images.

## Patches (`patches/`, applied in order)

| Patch | Purpose |
|---|---|
| `0001-build-portability.patch` | Makes `pkg-config` optional (SDL2 via `SDL2Config.cmake`) and adds the `RETRO_HARNESS` CMake option wiring `refharness.cpp` + pinned BLAKE3 sources. |
| `0002-disable-video.patch` | Compiles out libogg/libtheora video playback under `RETRO_HARNESS` (S1/S2 ship no `Data/Video`; avoids a dependency that has no user-local build here). |
| `0003-reference-harness.patch` | Adds `RSDKv5/refharness.{hpp,cpp}` and hooks: deterministic RNG seed (`Math.cpp`), scripted input injection (`Input.cpp`), no frame-skip/no wall clock + software-renderer fallback + presentation skip (`SDL2RenderDevice.cpp`), frame recording and frame-limit exit (`RetroEngine.cpp`). |
| `0004-reject-lr-buttons.patch` | Rejects `L`/`R` button names and mask bits in the input script parser; rev03 `ControllerState` has no L/R fields, so they could only be silently ignored. |

The harness only activates when a `REF_HARNESS_*` variable is set; without it the binary behaves
like the upstream decompilation.

## Determinism notes

* Frame pacing is disabled (`CheckFPSCap` always true) and no frame skipping happens; frame N is
  always the Nth `ProcessEngine` tick.
* Input comes only from the script; SDL keyboard/gamepad polling is bypassed while active.
* `srand`/`randSeed` are re-seeded from the fixed seed on every `InitEngine` (scene reload).
* Audio is opened with `SDL_AUDIODRIVER=dummy` and never influences simulation.
* Wall-clock time is not read anywhere on the simulation path (scene timers tick per frame).
* Verified: two full 600-frame runs with identical input produced byte-identical
  `records.jsonl`.

## Verified parity (`m7-fidelity` @ `c8c3da1` + input alignment)

Commands: the `run.sh` invocation above, plus the Rust command from "Reference vs Rust port"
(`Rust`: build `m7-fidelity`, seed 1592594996). All framebuffer comparisons use
`compare_frames.py --offset 0`:

| Input | Window | First divergence |
|---|---|---|
| `zone01_idle.input` (S1/S2) | 600 frames | none (600/600) |
| `zone01_right.input` (S1/S2) | 600 frames | none (600/600) |
| `zone01_right400.input` (S1/S2) | 600 frames | before the tick-alignment fix: record 400; now none |
| `title_start.input` (S1/S2, Title -> Zone01) | 1200 frames | before: record 801; after the alignment fix: record 1028 (S1) / 1010 (S2), caused by scripts writing `keyDown`/`keyPress`; with those writes ported: none (1200/1200) |

Earlier revisions of the port diverged in the title-card fade (frame 155) and at the first
death (`STAGEMODE_FROZEN` handling, record 377); both are fixed and covered by pinned tests in
`crates/retro-engine/tests/{collision_parity,assets}.rs`.

## Limitations

* Only the RSDKv5U rev-3 **legacy v4** path is exercised; the v3/v5 scene paths are untested.
* Video playback, mod loader and shader paths are disabled in the harness build. S1/S2 have no
  video assets, so the stub matches the shipped data.
* Camera styles other than `CAMERASTYLE_FOLLOW` (0) are dormant in the Rust port: upstream's
  `HandleCameras` also dispatches `EXTENDED`/`EXTENDED_OFFSET_L`/`EXTENDED_OFFSET_R`/`HLOCKED`/
  `FIXED`/`STATIC` and falls back to `SetPlayerLockedScreenPosition` when `cameraEnabled != 1`.
  The verified scenes use style 0 only; a scene that switches styles will diverge.
* Audio is the dummy SDL driver; the harness deliberately compares graphics + simulation state,
  not audio.
* `run.sh` always regenerates `Settings.ini` with `devMenu=n`, `gameType=0`, `pixWidth=424`; use
  `--game-type 1` for Origins scripts. Fullscreen/window settings are irrelevant headlessly.
* Entity `values` are omitted for non-player entities by default (`REF_HARNESS_ENT_VALUES=1` to
  include them); framebuffer hashes still cover every drawn pixel.
* The Rust side currently emits a composite `state_hash`, so only framebuffer-image comparison
  (`compare_frames.py`) and reference-to-reference `diff_records.py` are field-level today. If
  `crates/retro-parity` gains a JSONL emitter matching this format, `diff_records.py` will work
  C↔Rust unchanged: scripted input is now indexed by absolute tick, so record `N` consumes line
  `N` and `--offset 0` aligns the streams.
