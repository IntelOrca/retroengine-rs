# Project history

Last updated: 2026-10-24  
Baseline: `main` @ `5fd1383` (after tag `m9`)

This is the condensed history of the Retro Engine Rust port. The original
per-milestone notes were development records; this document keeps the durable
decisions, outcomes and verification evidence. Current unfinished work is in
[`progress.md`](progress.md).

## Scope and architecture

The project is a from-scratch Rust port of the Retro Engine, targeting RSDKv4
legacy data for Sonic 1 and Sonic 2. It loads unpacked asset folders, runs
headless on Linux CI, and builds a self-contained Windows executable.

Key decisions made at the start and kept throughout:

- **RSDKv4 / S1 / S2 first.** RSDKv2/v3/v5, Mania and S3K were deferred. The
  v2/v3 script tables are placeholders that currently fall back to v4.
- **Runtime profiles, not conditional compilation.** Engine version and platform
  behaviour are selected through runtime profiles and data tables.
- **SDL3 plus a headless backend.** Only `retro-platform` touches SDL3/OS APIs.
  The platform backend is selectable at runtime.
- **Software RGB565 framebuffer is the parity artifact.** SDL3 only presents it.
  Pixel-exact framebuffer comparison against the C++ reference is the acceptance
  signal.
- **No native game objects.** All objects, including the 6,070-line player
  object, run as RetroScript. Game logic is not duplicated in Rust crates.
- **No game data in the repository.** Assets are supplied separately and must
  never be committed. Packs (`Data.rsdk`) were not supported; only unpacked
  asset folders were targeted.
- **Decomp-style licence.** Attribution, no asset redistribution, and DLC off by
  default. GPL decompilation/export tools were used only as black-box oracles.

## Milestones

| Tag | Date | Result |
|---|---|---|
| `m0` | 2026-10-02 | 15-crate workspace, platform traits, SDL3/headless backends, CLI skeleton, licences, Linux+Windows CI. 54 tests. |
| `m1` | 2026-10-02 | RSDKv4 formats: GameConfig, stages, scenes, tiles, collision, backgrounds, animations, palettes, GIF decoding, saves. 186 tests; 0 corpus mismatches and 46,035 truncation/mutation probes with no panics. |
| `m2` | 2026-10-02 | RetroScript v4 text compiler, bytecode loader/writer, disassembler and VM. 278 tests; 72/72 bytecode files byte-identical; 756 scripts compile. |
| `m3` | 2026-10-02 | Scene/entity engine, object registry, collision base, script host, deterministic frame loop and state hash. 341 tests; all 171 S1/S2 acts boot headless. |
| `m4` | 2026-10-02 | Software renderer: RGB565 framebuffer, palettes/ink LUTs, sprites, tiles, backgrounds, HUD, PNG dumps, SDL3 presentation. 401 tests; first 600-frame framebuffer hashes. |
| `m5` | 2026-10-02 | Audio (WAV/Vorbis, mixer, device submit), input mappings/scripted input, save RAM/achievements format, CLI wiring. 524 tests. |
| `m6` | 2026-10-02 | Friendly scene resolution, `--list`, act normalisation, mouse-as-touch, windowed run-until-close, static Windows CRT + SDL3 zip. 549 tests; ~1000 fps headless. |
| `m6.1` | 2026-10-03 | First Windows feedback round: platform/audio robustness, standalone scripts by default, title camera offsets, idle spawn/floor probe. 571 tests. |
| `m7` | 2026-10-03 | Deterministic C++ reference harness and major parity wave: deferred `LoadStage`, paused title stage, full `ProcessObjectMovement`, `ForEachAll`, live draw lists. S1/S2 Zone01 and Title→Zone windows framebuffer-identical. |
| `m7.1` | 2026-10-04 | Streaming Ogg, bounded audio backlog, clock rebase after stalls, unbounded runs, pause-state semantics. EHZ monitor/chunk report traced to edited asset data, not the engine. |
| `m8` | 2026-10-04 | Front-end completion: camera styles 0–6 and locked branch, text menus, `engine.state` end/reset flows, `BoxCollision2`, attract/Continue/Credits verification. LSelect/S1 Special/S2 OOZ parity pinned. |
| `m8.1` | 2026-10-04 | Logic-rate audio pacing (`frames_due`), boss-music volume reset, HPZ Zone08 infinite draw-list fix. 0 underruns/gaps after the fix. |
| `m9` | 2026-10-04 | S2 3D special stages: scene3D matrices/transforms, vertex/face buffers, textured/faded/blended scanline rasteriser. All 8 acts × 600 frames framebuffer-identical to the reference. |

The whole M0–M9 sequence was completed over 2–4 October 2026.

## How parity was verified

M7 introduced the approach used for every later milestone:

- A pinned C++ reference harness based on RSDKv5-Decompilation (`43d426f`),
  SDL2 (`5d24957`) and BLAKE3 (`81f772a`), with portability, video-stub,
  frame-record, scripted-input and fixed-seed patches.
- Deterministic scripted input where input line *N* is applied on record *N*,
  including the load tick.
- Per-frame JSONL records containing framebuffer hashes, camera/player/entity
  state and optional text-menu/scene3D fields.
- Golden framebuffer pins stored next to the asset-gated tests, plus PNG/PPM
  keyframe comparison when a hash diverged.
- An all-acts sweep over 171 S1/S2 acts to catch late stubs and crashes.
- Full fmt/clippy/test gates on Linux, and a Windows build with static CRT and
  SDL3, checked for unwanted DLL imports.

The harness was extended as the port grew: per-player input and `--boot` in M8,
then `scene3D` records in M9. The Rust side still relies on framebuffer hashes;
the `retro-parity` JSONL emitter remains a tooling stub.

## Notable bugs and lessons

- **Input edges and skipped ticks.** Scripted input had to preserve the previous
  held state across load ticks, otherwise press edges shifted by one frame.
- **`LoadStage` was only a flag.** Title, act transitions and attract mode stalled
  until the deferred load tick was modelled explicitly.
- **`ForEachAll`/frozen-state bugs.** Incorrect entity write-back and a completely
  frozen death state broke HUD updates and death animations.
- **Audio was paced by presentation.** A slow `SDL_RenderPresent` could starve the
  mixer; the clock now counts due 60 Hz logic frames and catches up within a
  bounded limit. Audio streaming was also switched from whole-track decode to a
  packet-streamed ring.
- **Draw lists could grow while being drawn.** HPZ appended to the list it was
  iterating and hung with unbounded memory; the fix snapshots the list length per
  pass.
- **Some reported visual bugs were data, not engine.** The EHZ stray chunk and
  monitors were caused by an edited `Zone01/Act1.bin`, byte-identical to the
  shipped archive. Rust matched the reference; the recommendation was to restore
  an official asset file.
- **Randomness had to match glibc.** The engine's RNG was replaced with a
  validated `GlibcRand` implementation and its state is included in hashes.

## Current state

Single-player Sonic 1 and Sonic 2 are essentially feature-complete for the
RSDKv4 target. The code and every tag through `m9` are on GitHub; there are no
local-only branches. The remaining major feature is **M10 — 2P VS**, followed by
optional polish, deferred edge cases and deliberately out-of-scope versions.
See [`progress.md`](progress.md) for the current backlog.

The original development notes under `notes/M0.md`–`M9.md` remain useful as
detailed evidence records, but their older “remaining gaps” sections are
historical. In particular, the M7 gap list and M8 items G1–G5, G8 and G10 were
closed by M8/M8.1/M9.
