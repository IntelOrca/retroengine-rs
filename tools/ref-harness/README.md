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
| S2 `Special` act 1-8, 600 frames `--scene3d` (2026-10-04) | 600 records/act, all `scene3D` blocks present; act 1 rerun byte-identical (`diff_records.py`: no divergence) |

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
re-runnable: it resets the engine checkout, re-applies `patches/*.patch` and rebuilds. On hosts
with little RAM, export `CMAKE_BUILD_PARALLEL_LEVEL=2`; `build.sh` maps it onto `ninja -j` (ninja
itself only honours the variable through `cmake --build`).

## Run a scene

```sh
tools/ref-harness/run.sh \
    --game S1 --scene Zone01 --act 1 --frames 600 \
    --input tools/ref-harness/testdata/zone01_right.input \
    --out /tmp/opencode/refrun/zone01-right \
    --ppm 155,240            # optional keyframe PPM dumps
```

Without `--scene`/`--act`, `--boot` starts the reference with no `stage=`/`scene=` argument so it boots
the real `GameConfig` list (Title at list position 0) for attract/Continue/game-over/credits windows:

```sh
tools/ref-harness/run.sh --game S1 --boot --frames 1600 \
    --out /tmp/opencode/refrun/s1-boot
```

`--record-input` adds the parsed and actual controller button masks to every record (see below);
`--input-trace` logs the per-player masks to stderr each tick. `--text-menus` adds the legacy
`gameMenu` state to each record, and `--screens` adds per-screen framebuffer hashes when
`videoSettings.screenCount > 1` (dormant for standalone S1/S2, which use one screen).
`--scene3d` adds the legacy v4 3D state (matrices, projection/fog and full vertex/face buffer
digests) to every record; it is off by default and the record stream is byte-identical without
it.

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
`crates/retro-input/src/scripted.rs`), so the same file drives both engines. The four per-player
button columns are injected; axes/touches are parsed but ignored:

```text
retro-input 1
seed 1592594996
0 - 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -
60 RIGHT 0 0 -  LEFT|B 0 0 -  - 0 0 -  - 0 0 -
```

Upstream maps `entity.controlMode + 1` onto the controller array, so player 1 reads
`controller[CONT_P1]` (index 1), player 2 reads `controller[2]`, etc. Column 1 (Rust player 0) is
therefore applied to `controller[1]`, column 2 (Rust player 1) to `controller[2]`, and columns 3/4
to `controller[3]`/`controller[4]`. `controller[CONT_ANY]` (index 0) — what unassigned devices feed
and what the pause/frame-step checks read — receives the OR of all players. For the M7 1P files
(only column 1 ever non-neutral) this is byte-identical to the old behaviour of applying column 1
to `controller[0]` and `controller[1]`.

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
* `zone01_pause.input` – idle through frame 199, then START held (pause)
* `players_split.input` – column 1 RIGHT from line 10, column 2 `LEFT|B` from line 50 (injection
  verification; use with `--record-input`)
* `zone01_spindash.input` – S1 `Zone01`: DOWN (crouch) from line 220, `DOWN|A` charge presses on
  270/273/276 and release on line 290 (needs the save override below)

### Spindash save seeding

`PlayerObject`'s startup reads `saveRAM[35]` (`options.spindash`), which the shipped
`S1/SGame.bin` leaves `0` (with the option off, a charge press jumps instead). The reference
window and `zone01_spindash_framebuffer_matches_reference` in
`crates/retro-engine/tests/assets.rs` (via `spindash_storage()`) both seed word 35 to `1`; the
test patches the little-endian `i32` at byte offset `35 * 4`. Point `run.sh` at a shadow assets
root that symlinks the real `Data/` and serves only the patched save (the real assets are never
written):

```sh
SHADOW=/tmp/opencode/refrun/s1-spindash-assets
mkdir -p "$SHADOW/S1"
ln -s /home/ted/projects/assets/S1/Data "$SHADOW/S1/Data"
cp /home/ted/projects/assets/S1/SGame.bin "$SHADOW/S1/SGame.bin"
python3 - "$SHADOW/S1/SGame.bin" <<'PY'
import struct, sys
path = sys.argv[1]
data = bytearray(open(path, "rb").read())
struct.pack_into("<i", data, 35 * 4, 1)  # options.spindash = 1
open(path, "wb").write(data)
PY

tools/ref-harness/run.sh --game S1 --scene Zone01 --act 1 --frames 360 \
    --input tools/ref-harness/testdata/zone01_spindash.input \
    --assets-root "$SHADOW" \
    --out /tmp/opencode/refrun/s1-spindash
```

`run.sh` copies `SGame.bin` into its scratch dir and symlinks only `Data/`, so the shadow root and
the real asset tree both stay read-only. The resulting records reproduce every
`S1_ZONE01_SPINDASH` pin (frames 0, 269, 290, 291, 295, 300, 304, 305, 320, 359).

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
* With `REF_HARNESS_RECORD_INPUT=1` each record also carries
  `"input":{"p1":N,"p2":N,"p3":N,"p4":N,"ctrl":[N,N,N,N,N]}`: the four parsed button masks
  (`p1` = first column) and the held-button masks reconstructed from `controller[0..4]` after the
  tick. This is how per-player injection is verified (`run.sh --record-input`).
* With `REF_HARNESS_TEXT_MENUS=1` each record also carries
  `"textMenus":[{"rowCount":..,"visibleRowCount":..,"visibleRowOffset":..,"selection1":..,
  "selection2":..,"selectionCount":..,"alignment":..,"timer":..,"textDataPos":..,
  "entryStart":[..],"entrySize":[..],"entryHighlight":[..]}, ...]` (two legacy menus, arrays
  truncated to `rowCount`); `run.sh --text-menus`.
* With `REF_HARNESS_SCREENS=1` and `videoSettings.screenCount > 1`, each record also carries
  `"screens":[{"w":..,"h":..,"blake3":".."}, ...]`, one hashed visible framebuffer per active
  screen (each with its own `size`/`pitch`); `fb` remains `screens[0]`. `run.sh --screens`.
* With `REF_HARNESS_SCENE3D=1` each v4 record also carries a `scene3D` object with the legacy 3D
  state (`run.sh --scene3d`):

  ```json
  "scene3D":{"vertexCount":1400,"faceCount":740,"projectionX":216,"projectionY":216,
    "fogColor":0,"fogStrength":80,
    "matWorld":[16 ints],"matView":[16 ints],"matTemp":[16 ints],
    "vertexHash":"<blake3>","faceHash":"<blake3>"}
  ```

  Matrices are row-major `int32` (`values[4][4]`). `vertexHash` is BLAKE3 of the full
  `Legacy::v4::vertexBuffer` (`0x1000` vertices) and `faceHash` of the full `faceBuffer`
  (`0x400` faces), each serialized as tightly packed little-endian `i32` fields
  (`x,y,z,u,v` per vertex; `a,b,c,d,color,flag` per face — `color` is its `u32` bit pattern).
  The digests cover the whole buffers, not just the `vertexCount`/`faceCount` used range, because
  the shipped scripts read and write scratch slots past the cursors.
* `cameras` are `Legacy::cameras[0..1]`; `scroll`/`shake`/`lag` are the legacy camera globals.
* Names/strings are JSON-escaped.

Environment variables honoured by the binary (normally set by `run.sh`):
`REF_HARNESS_DUMP`, `REF_HARNESS_INPUT`, `REF_HARNESS_FRAMES`, `REF_HARNESS_SEED`,
`REF_HARNESS_PPM_DIR`, `REF_HARNESS_PPM_FRAMES`, `REF_HARNESS_PPM_EVERY`, `REF_HARNESS_ENT_VALUES`,
`REF_HARNESS_RECORD_INPUT` (adds the `input` object; `0`/unset = off), `REF_HARNESS_INPUT_TRACE`
(stderr line per tick with the injected `p1..p4`/`any` masks), `REF_HARNESS_TEXT_MENUS` (adds the
`textMenus` array), `REF_HARNESS_SCREENS` (adds the `screens` array when
`videoSettings.screenCount > 1`) and `REF_HARNESS_SCENE3D` (adds the `scene3D` object; `0`/unset =
off).

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
| `0005-per-player-input.patch` | Parses all four per-player button columns and injects them into `controller[CONT_P1..CONT_P4]` (with `CONT_ANY` = OR of all players); adds the optional `input`, `textMenus` and `screens` record blocks plus the per-tick mask trace (`REF_HARNESS_RECORD_INPUT`, `REF_HARNESS_TEXT_MENUS`, `REF_HARNESS_SCREENS`, `REF_HARNESS_INPUT_TRACE`). |
| `0006-scene3d-record.patch` | Adds the optional `scene3D` record block (`REF_HARNESS_SCENE3D`): the legacy v4 `vertexCount`/`faceCount`/`projectionX`/`projectionY`/`fogColor`/`fogStrength` scalars, the persisted `matWorld`/`matView`/`matTemp` matrices and BLAKE3 digests of the full `0x1000` vertex / `0x400` face buffers as little-endian `i32`. Purely additive: without the env var the records are byte-identical. |

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

## M8 reference windows (patch 0005)

Generated with the rebuilt harness (patches 0001–0005) and `zone01_idle.input`/`title_idle.input`
(seed 1592594996); records under `/tmp/opencode/m8-harness/records/` (scratch, not committed).
`fb.blake3` is the final record's framebuffer hash (frame index in parentheses):

| Window | Frames | Final `fb.blake3` |
|---|---|---|
| S1 `LSelect` act 1 idle | 600 | `129bd878c03d9c295f920416949a59d57576da5c4fea5eff5be8067ba6b02d42` (599) |
| S2 `LSelect` act 1 idle | 600 | `5fcbd9dbe431d6033343857ba737623357e00cf344108aca3fea5e3f11c00b88` (599) |
| S1 `Special` act 1 idle | 600 | `cfede829d9a0e12f6cb2fff5c35962e83949c47ac458ce0865a3743fc892c976` (599) |
| S1 `Continue` idle | 600 | `185bf124b602857645f274754e6f7162886fcf743c101ccec3dda08c1e9833f8` (599) |
| S1 `Credits` idle | 600 | `14579eb008f57f0b54dced061dc19d2edbd0552a741c0d606d57c1880eddffea` (599; loads `Zone01` at 211) |
| S1 `Title` idle (`--scene Title`) | 1600 | `f69be45a040048f841e49fb14d64227478cec862bc8ec41ace67873f048b1e5f` (1599; attract loads `Zone01` at 1049) |
| S1 boot idle (`--boot`) | 1600 | `f69be45a040048f841e49fb14d64227478cec862bc8ec41ace67873f048b1e5f` (1599; real list pos 0, `TITLE SCREEN`) |
| S2 `Zone03` (ARZ) act 1 idle | 600 | `fa6bc0fe6891c2bab3eb78f30840de66cc1faaf291ac7cb122a92f989e60d74d` (599) |

`--scene Title` records the synthetic `_RSDK_SCENE` entry at `list=60`; `--boot` records the real
`GameConfig` entry (`cat=0,list=0,scenename="TITLE SCREEN"`) and is the closer match for the Rust
port's real-list boot. Both reach the same attract state by frame 1600.

Per-player injection is verified with `players_split.input` (column 1 RIGHT from line 10, column 2
`LEFT|B` from line 50) and `--record-input`: record 10 has `input.ctrl=[8,8,0,0,0]` (P1 RIGHT;
`CONT_ANY` = OR) and record 50 has `input.ctrl=[44,8,36,0,0]` (P2 `LEFT|B` = 36, `CONT_ANY` = 44).

## M9 reference windows (patch 0006)

The S2 `Special` halfpipe acts are the M9 3D windows. Captured with patches 0001–0006 and
`zone01_idle.input` (seed `1592594996`), 600 frames each, `--scene3d`; records and PPM keyframes
under `/tmp/opencode/m9-harness/{records,ppm}/special-{1..8}/` (scratch, not committed):

```sh
export REF_HARNESS_BUILD=/tmp/opencode/m9-harness/refbuild
export CMAKE_BUILD_PARALLEL_LEVEL=2          # honoured by build.sh for the SDL2/engine builds
tools/ref-harness/build.sh
for a in 1 2 3 4 5 6 7 8; do
  tools/ref-harness/run.sh --game S2 --scene Special --act $a --frames 600 \
    --input tools/ref-harness/testdata/zone01_idle.input --scene3d \
    --out /tmp/opencode/m9-harness/records/special-$a --ppm 100,300,599
done
# determinism: same input twice, byte-identical records (the scene3D block is compared too)
tools/ref-harness/run.sh --game S2 --scene Special --act 1 --frames 600 \
  --input tools/ref-harness/testdata/zone01_idle.input --scene3d \
  --out /tmp/opencode/m9-harness/records/special-1-rerun
python3 tools/ref-harness/diff_records.py \
  /tmp/opencode/m9-harness/records/special-1/records.jsonl \
  /tmp/opencode/m9-harness/records/special-1-rerun/records.jsonl
```

At `STAGEMODE_LOAD` (record 0) every act reports `vertexCount=1400`, `faceCount=740`,
`projectionX=projectionY=216`, `fogColor=0`, `fogStrength=0x50` (the `Halfpipe` tube mesh);
`matWorld`/`matView`/`matTemp` start zeroed and persist across frames. Pin frames for the Rust
port are `0, 100, 300, 321, 599` (`321` is the old divide-by-zero boundary). The `--scene3d`
digests cover the full buffers, so a divergence localizes to the matrices (`matWorld`/`matView`/
`matTemp`), the cursors/scalars or a specific buffer before falling back to PPM pixel diffs.

## Limitations

* Only the RSDKv5U rev-3 **legacy v4** path is exercised; the v3/v5 scene paths are untested.
* Video playback, mod loader and shader paths are disabled in the harness build. S1/S2 have no
  video assets, so the stub matches the shipped data.
* Camera styles 0-6 and the `camera.enabled != 1` locked branch are ported (M8,
  `crates/retro-engine/src/camera.rs`). Shipped S1/S2 data only exercises `CAMERASTYLE_FOLLOW`
  (0; every M7 window) and `CAMERASTYLE_HLOCKED` (4; S1 PlayerObject spindash, pinned by
  `S1_ZONE01_SPINDASH`). `CAMERASTYLE_STATIC` (6) is assigned only under `USE_ORIGINS`/vs mode,
  and `CAMERASTYLE_EXTENDED`/`EXTENDED_OFFSET_L`/`EXTENDED_OFFSET_R` (1-3) and
  `CAMERASTYLE_FIXED` (5) by no shipped S1/S2 script, so styles 1-3/5 are unit-tested only, while
  the locked branch is also reference-pinned through the S1 death/respawn window
  (`S1_ZONE01_RIGHT` frames 400/500).
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
* The M9 rasterizer intentionally diverges from upstream where upstream reads out of bounds:
  texture samples outside the sheet return index 0 (transparent) instead of reading the shared
  `graphicData` pool (`retro-render/src/faces.rs:14`), and out-of-range `vertexCount`/
  `faceCount`/vertex/face indices are skipped instead of corrupting memory
  (`retro-render/src/scene3d.rs`, `retro-engine/src/draw3d.rs`). Neither produced a divergence in
  the S2 `Special` act 1-8 600-frame windows (8 x 600 frames framebuffer-identical); the
  out-of-sheet path is not instrumented, so "never sampled" is not proven.
