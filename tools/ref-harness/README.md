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
| `build.sh` from a clean build dir | builds SDL2 2.32.10, BLAKE3 1.5.5, patched RSDKv5U + `hash565` |
| S1 `Title` act 1, 600 frames | 600 records, 236 unique framebuffers, exit 0 |
| S1 `Zone01` act 1, 600 frames (idle / hold RIGHT) | 600 records each, 495 / 380 unique framebuffers |
| Two identical runs (`cmp records.jsonl`) | byte-identical (deterministic) |
| S2 `Title` and `Zone01` act 1, 120 frames | boot and render (S2 covered by the same loader) |
| C reference vs Rust `--dump-frames` PNGs, S1 Zone01 hold-RIGHT | first divergent framebuffer at **frame 155** |

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
* Button names: `UP DOWN LEFT RIGHT A B C X Y Z L R START SELECT`, joined with `|`, or `-`,
  a decimal mask, or a `0x` mask.
* `seed` (or `--seed`/`REF_HARNESS_SEED`, which take precedence) seeds both libc `rand()` and
  `Engine.randSeed`, so script `Rand()` calls are reproducible.

`testdata/` contains text-only inputs used by the verified runs:

* `title_idle.input` – header-only, idle (repeat)
* `zone01_idle.input` – header-only, idle (repeat)
* `zone01_right.input` – idle through frame 59, then RIGHT held

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
scenes and `scenename` is `_RSDK_SCENE`.

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

## First verified divergence (S1 Zone01, hold RIGHT)

Commands: the `run.sh` invocation above, plus the Rust command from "Reference vs Rust port"
(`Rust`: build `m7-fidelity`, seed 1592594996). Observed:

* Frames 0–154 match pixel-for-pixel. Frame 0 is the scene load; frames ~0–200 are the title-card
  pause, so this divergence happens during the title-card fade before gameplay starts.
* **First divergent frame: 155** (`A=0ad5666c…`, `B=bcc1a1a6…`); at frame 155 the difference is
  42,838 pixels, all near-black values (`0x0000/0x0001/0x0040/0x0800`), i.e. a background/fade
  difference rather than the player.
* By frame 240 the images differ massively (66k pixels) as the runs move through the level
  differently; frame 599 differs in only 1,235 pixels (y 80–130), consistent with a UI/title-text
  difference after the Rust run's death/respawn timing diverged.
* Reference keyframes for inspection: `/tmp/opencode/refrun/final-zone01-right/ppm/frame_0155.ppm`
  and `frame_0240.ppm` (regenerate with `--ppm 155,240`). These are asset-derived and stay out of
  the repository.

## Limitations

* Only the RSDKv5U rev-3 **legacy v4** path is exercised; the v3/v5 scene paths are untested.
* Video playback, mod loader and shader paths are disabled in the harness build. S1/S2 have no
  video assets, so the stub matches the shipped data.
* Audio is the dummy SDL driver; the harness deliberately compares graphics + simulation state,
  not audio.
* `run.sh` always regenerates `Settings.ini` with `devMenu=n`, `gameType=0`, `pixWidth=424`; use
  `--game-type 1` for Origins scripts. Fullscreen/window settings are irrelevant headlessly.
* Entity `values` are omitted for non-player entities by default (`REF_HARNESS_ENT_VALUES=1` to
  include them); framebuffer hashes still cover every drawn pixel.
* The Rust side currently emits a composite `state_hash`, so only framebuffer-image comparison
  (`compare_frames.py`) and reference-to-reference `diff_records.py` are field-level today. If
  `crates/retro-parity` gains a JSONL emitter matching this format, `diff_records.py` will work
  C↔Rust unchanged (mind the Rust `state.frame` starts at 1 for the first executed frame; use
  `--offset`).
