# Remaining work

Last verified: 2026-10-24  
Baseline: `main` @ `c76af28` (tag `m9`, pushed 2026-10-04)

Single-player Sonic 1 and Sonic 2 are essentially feature-complete for the
current RSDKv4 target. The code, tags `m0`–`m9`, and all branches are on GitHub;
there are no local-only commits and no open GitHub issues. This document is the
remaining feature and polish backlog, not a list of missing source.

## M10 — 2P VS (the only major unfinished feature)

Not started.

- [ ] Per-player input — `keyDown[1]`/`keyPress[1]` are not yet distinct from player 1.
- [ ] Implement `STAGEMODE_2P` and `Process2PObjects`.
- [ ] Wire the `options.vsMode` flow.
- [ ] Per-player camera, HUD, and screen handling.
- [ ] Implement `SetScreenCount` / `SetScreenVertices` (currently stubs).
- [ ] Wire the 2P VS menus and results screens. The text-menu system itself is done.
- [ ] Decide single shared screen vs split-screen. Standalone data likely uses one
      shared screen; Origins split-screen is a stretch/validation item.
- [ ] Verify `SendEntity` / `ReceiveEntity` / `TransmitGlobal` are harmless no-ops
      for local single-device 2P.
- [ ] Capture a reference 2P run (`--scene LSelect --act 2` with 2P input) before
      designing the screen model.

Everything else in S1/S2 single-player — menus, level select, game-over/credits
flows, camera styles, `BoxCollision2`, S1 special stages, S2 3D special stages,
and the HPZ hang — is done.

## Optional polish (M11 in the M8 plan)

- [ ] Achievements persistence. `Achievements.bin` load/save exists, but the
      script `SetAchievement` native callback is not wired up, so in-game unlocks
      are not persisted.
- [ ] Native options / time attack. Time attack has no scripted entry in the
      standalone data; supporting it requires a native UI. Decide whether to
      implement it or document it as out of engine scope.
- [ ] macOS CI. Only Ubuntu and Windows workflows exist.
- [ ] Windows audio validation. The Windows audio path is still reasoned/mock-tested
      on this Linux machine and needs a real Windows audio-device run.
- [ ] Triage the low-priority residuals listed below.

## Deferred engine/tooling edge cases

These are real gaps but are unused by the shipped S1/S2 stages:

- [ ] `Draw3DFloorLayer` / `Draw3DSkyLayer` / `Init3DFloorBuffer`.
- [ ] 3D face flags 0–3 (unit-tested only, no reference pin).
- [ ] `FX_ROTOZOOM` 3D sprites (unreachable in shipped S2 Special).
- [ ] Rust `--emit-records` JSONL emitter — `retro-parity` is still a stub. This is
      tooling only and is not needed for framebuffer parity.
- [ ] Matrix/buffer carry-over across a real select→Special `LoadStage`. The current
      same-`EngineState` preservation is probably correct; verify only if that
      window is added.
- [ ] Origins platform path. `CallNativeFunction2/4` are no-ops and some Origins
      callbacks/title flow remain stubbed. `--origins` exists but is not fully
      validated.
- [ ] RSDK v2/v3 script VMs — they currently fall back to the v4 tables.
- [ ] `Data.rsdk` pack loaders — only unpacked asset folders are supported.
- [ ] Native objects/game-logic crates — intentionally not ported; all objects run
      through RetroScript by design.
- [ ] Mods and DLC — out of scope for the current target.

## Low-priority residuals

- [ ] Out-of-range `PlayMusic` track IDs are not wrapped with `trackID & 0xF`;
      no shipped script hits it.
- [ ] `game_mode` is not included in `state_hash`; the framebuffer catches pause
      divergences on the next frame.
- [ ] `DirSource` case-collision resolution order is filesystem-dependent.
- [ ] `SData.bin` / `UData.bin` handling is unit-only (the files are not present in
      the shipped assets).
- [ ] `next_in(min > max)` returns `min` (documented deviation).
- [ ] Letterbox-bar touch clamping differs subtly from upstream.
- [ ] Duplicate S1 mission folders without `--act` default by id ranking.
- [ ] `exclusive_fullscreen` is recorded but not consumed.
- [ ] `audio: unavailable` prints to stdout.

## Historical gaps — now closed

Do not treat these older note sections as current:

- `notes/M7.md` “Remaining known gaps” — superseded by M8/M9.
- `notes/M8.md` gap inventory G1–G5, G8, G10 — completed in M8/M8.1 (camera
  styles, text menus, `engine.state` flows, `BoxCollision2`, HPZ Zone08 hang,
  attract-mode verification).
- `notes/M9.md` deferrables 1–2 — unused by shipped stages; tooling only.

## Verification

- `git status` is clean and `main` matches `origin/main` at `c76af28`.
- All tags `m0`–`m9` exist on `origin`.
- `git log --branches --not --remotes` is empty.
- `gh issue list --state open` returns no open issues.
