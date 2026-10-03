# retroengine-rs

A from-scratch Rust port of the Retro Engine (RSDK v4 legacy data, Sonic 1 & Sonic 2).

The engine loads unpacked RSDK asset folders, runs headless on Linux CI, and builds a Windows executable. Version-specific behaviour is selected through runtime profiles; the platform backend (SDL3 or headless) is chosen at runtime with both compiled in. A software-rendered u16 RGB565 framebuffer is the exact-parity artifact and SDL3 only presents it.

## Workspace layout

| Crate | Responsibility |
| --- | --- |
| `retro-core` | Fixed-point math, RNG, colour packing and trig lookup primitives |
| `retro-platform` | Runtime-selectable headless and SDL3 backends (the only SDL3/OS boundary) |
| `retro-io` | Asset folder discovery and byte-level access |
| `retro-format` | Version-agnostic container dispatch |
| `retro-format-v4` | RSDK v4 legacy binary parsers |
| `retro-image` | Image container decoding |
| `retro-script` | Bytecode loader and VM |
| `retro-scene` | Scene graph and entity storage |
| `retro-render` | Software RGB565 renderer |
| `retro-audio` | Audio mixing and streaming |
| `retro-input` | Input mapping |
| `retro-parity` | Frame capture and comparison tooling |
| `retro-engine` | Main engine binary (`retroengine`) |
| `retro-export` | Asset export/inspection binary |
| `retro-dev` | Developer utility binary |

## Building

Requirements: Rust 1.98.0 (pinned by `rust-toolchain.toml`), `cmake`, and a C/C++ toolchain. SDL3 is built from source by the `sdl3-sys` crate, so no system SDL3 and no `pkg-config` are required.

```sh
cargo build --release
```

Build artifacts go to `../target` (configured in `.cargo/config.toml`). The engine executable is `retroengine` (`retroengine.exe` on Windows).

`crates/retro-core/src/math_tables.rs` is generated; run `tools/gen_math_tables.sh` to regenerate it after changing the table formulas.

On this server, put the local cmake/ninja first on `PATH`:

```sh
export PATH="$HOME/.local/bin:$PATH"
cargo build --release
```

## Running

```sh
../target/release/retroengine /path/to/assets/S1                # windowed until the window closes
../target/release/retroengine /path/to/assets/S1 --headless     # 600 deterministic frames
```

`<ASSETS_DIR>` must be an unpacked RSDK asset folder containing `Data/Game/GameConfig.bin`
(a Sonic 1 `S1` or Sonic 2 `S2` folder). Without `--headless` the game opens in an SDL3 window
and runs until the window closes; `--frames N` caps either mode, and headless runs without
`--frames` default to 600 frames. `--list` prints the scene table without loading a stage.

On this Linux server the source-built SDL3 only has the dummy/offscreen video drivers (no
X11/Wayland development packages are installed), so windowed mode cannot show a window here.
Use `--headless`, or set `SDL_VIDEO_DRIVER=dummy` to exercise the window lifecycle (close,
resize, quit) in tests. Windows, macOS and Linux builds with X11/Wayland enabled get the full
SDL video drivers.

## Selecting a scene

```sh
retroengine /path/to/assets/S1 --list              # categories, scenes and available acts
retroengine /path/to/assets/S1 --scene GHZ --act 1 # Green Hill Zone act 1
retroengine /path/to/assets/S2 --scene GHZ         # Emerald Hill Zone act 1
retroengine /path/to/assets/S2 --scene Zone02 --act B
```

`--scene` is case-insensitive and ignores spaces and punctuation. It accepts:

| Form | Example | Picks |
| --- | --- | --- |
| `--list` index | `--scene 7` | the seventh listed scene |
| stage folder | `--scene Zone01` | the GameConfig entry for that folder |
| full scene name | `--scene "GREEN HILL ZONE 1"` | that scene |
| name without act | `--scene "GREEN HILL ZONE"`, `--scene MarbleZone2` | the scene, act 2 from the suffix |
| name prefix | `--scene GreenHill` | the unique prefix match |
| acronym | `--scene GHZ`, `--scene GHZ2` | the unique acronym match |

`GHZ` is the canonical first-zone shorthand and also resolves on Sonic 2 (Emerald Hill).
`--act` accepts numeric and short ids case-insensitively (`1`, `2`, `b`); an explicit `--act`
wins over a trailing number in `--scene`. An unknown scene lists close candidates, a missing
act file lists the `Act*.bin` files that do exist, and `--list-json` emits the same table as
JSON.

Saves live under `--user-dir`, or `%APPDATA%\retroengine-rs\retroengine` on Windows
(`SDL_GetPrefPath`), seeded from the shipped `SData.bin`/`SGame.bin` on first run.

Scripts compile with the standalone platform (`#platform: USE_STANDALONE`) by default, even when
`Settings.ini` has `gameType=1`; pass `--origins` to compile the `#platform: USE_ORIGINS` blocks
instead. Windowed startup prints one `platform:`, `video:` and `audio:` line, so a missing audio
device or an unusual driver is visible in the log without aborting the run.

Full verification:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
../target/release/retroengine /path/to/assets/S1 --headless --frames 3
../target/release/retroengine /path/to/assets/S1 --scene GHZ --act 1 --headless --frames 60
```

## Performance

Release headless throughput for S1 Zone01 (600 frames, single-threaded) is roughly 1000 fps
wall clock including startup and asset load, with a marginal cost of about 0.6 ms/frame
(~1700 fps) once warm. Windowed runs are locked to the engine's 60 Hz clock.

## Windows CI

`.github/workflows/ci.yml` builds the workspace on `windows-latest` with
`cargo build --workspace --release` and uploads two artifacts:

* `retroengine-windows-exe`: the raw `retroengine.exe`.
* `retroengine-windows-zip`: `retroengine-windows.zip` containing `retroengine.exe` and
  `README.txt` (usage, flags and the save location).

SDL3 is compiled from source via `sdl3-sys`'s `build-from-source-static` feature and linked
statically. The Windows job also links the MSVC C runtime statically
(`RUSTFLAGS=-C target-feature=+crt-static` plus a generated CMake toolchain file that sets
`CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded` for SDL3's own C build, since sdl3-sys does not
forward Rust's `crt-static` feature to CMake). `dumpbin /dependents` fails the build if
`SDL3.dll`, `VCRUNTIME140.dll` or `MSVCP140.dll` appears; the only runtime requirement left is
the OS-provided Universal CRT (`api-ms-win-crt-*`, shipped with Windows 10+). The Linux release
binary has no `libSDL3.so` dependency either. `--version`/`--help` print the CLI examples
including Windows paths.

## Assets and licensing

No game data is included in this repository and game data must never be committed. See `LICENSE.md`, `NOTICE.md`, and `LICENSES/`.
