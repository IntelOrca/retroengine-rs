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
| `retro-engine` | Main engine binary |
| `retro-export` | Asset export/inspection binary |
| `retro-dev` | Developer utility binary |

## Building

Requirements: Rust 1.98.0 (pinned by `rust-toolchain.toml`), `cmake`, and a C/C++ toolchain. SDL3 is built from source by the `sdl3-sys` crate, so no system SDL3 and no `pkg-config` are required.

```sh
cargo build --release
```

Build artifacts go to `../target` (configured in `.cargo/config.toml`).

On this server, put the local cmake/ninja first on `PATH`:

```sh
export PATH="$HOME/.local/bin:$PATH"
cargo build --release
```

## Running headless

```sh
../target/release/retro-engine /path/to/assets/S1 --headless --frames 3
```

`<ASSETS_DIR>` must be an unpacked RSDK asset folder containing `Data/Game/GameConfig.bin`. At M0 the binary parses and validates arguments, prints the resolved configuration, and exits.

Full verification:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
../target/release/retro-engine /path/to/assets/S1 --headless --frames 3
```

## Windows CI

`.github/workflows/ci.yml` builds the workspace on `windows-latest` with `cargo build --workspace --release` and uploads `retroengine.exe` as an artifact. SDL3 is compiled from source via `sdl3-sys`'s `build-from-source-static` feature using the runner's CMake and MSVC toolchain; no system SDL3 installation is needed.

## Assets and licensing

No game data is included in this repository and game data must never be committed. See `LICENSE.md`, `NOTICE.md`, and `LICENSES/`.
