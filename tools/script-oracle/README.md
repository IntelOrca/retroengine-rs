# Script compiler oracle

Runs the reference RSDKv4 text compiler on the shipped `Data/Scripts` sources and writes the same
canonical `_Bytecode` container as `retro_script::write_bytecode`, so the output of
`retro_script::compiler` can be compared byte-for-byte.

This is a development tool for validating the Rust port; it is not part of the Cargo workspace and
is not built by `cargo test`. The upstream `Script.cpp` is **fetched at run time** from a pinned
commit and is never vendored into this repository.

## How it works

`run.sh`:

1. Downloads `RSDKv4/Script.cpp` from
   `RSDKModding/RSDKv4-Decompilation` at commit `a7f5195e21fdad7b75e4587e249013feeea9e6f3` (the
   same revision the opcode/variable tables were generated from) into `.build/`.
2. Extracts the compiler half (everything before `LoadBytecode`) into `.build/compiler_section.cpp`.
3. Applies two patches:
   * `#include "RetroEngine.hpp"` becomes the local `stubs.hpp`, which provides the minimal engine
     surface (`ScriptFunction`, `objectScriptList`, file I/O, `Engine.releaseType`, ...).
   * The decomp-only `USE_DECOMP` platform tag is disabled so `#platform` selection matches the
     original tools that produced the shipped `_Bytecode` (the decomp build otherwise compiles
     those blocks in).
4. Compiles `main.cpp` + the extracted section with `g++ -std=c++17` and forwards the arguments.

`main.cpp` parses `GameConfig.bin`/`StageConfig.bin`, registers symbol tables in the same order as
`Scene.cpp` (globals compiled before stage names are registered), runs `ParseScriptFile` for each
file, and serialises code, jump table, object scripts and functions in the canonical block format.
`--all` compiles `GlobalCode` plus every stage folder.

The reference's fixed-size string buffers trip glibc fortify checks (level >= 2) in some paths, so
the harness builds with `-U_FORTIFY_SOURCE -D_FORTIFY_SOURCE=0`; its output was verified under
AddressSanitizer and is byte-exact.

## Requirements

* `g++` with C++17 (tested with GCC 15)
* `curl` or `wget`
* A local Sonic 1/2 asset tree (`assets/S1`, `assets/S2`)

## Usage

```sh
# One group:
tools/script-oracle/run.sh /home/ted/projects/assets/S1 GLOBAL /tmp/GlobalCode.bin origins
tools/script-oracle/run.sh /home/ted/projects/assets/S2 Zone01 /tmp/Zone01.bin standalone

# Every group (GlobalCode + all stages) in a directory:
tools/script-oracle/run.sh /home/ted/projects/assets/S1 --all /tmp/oracle/S1 origins
tools/script-oracle/run.sh /home/ted/projects/assets/S2 --all /tmp/oracle/S2 origins
```

The build directory defaults to `tools/script-oracle/.build` and can be overridden with
`SCRIPT_ORACLE_BUILD`. `origins` (default) selects `USE_ORIGINS`, `standalone` selects
`USE_STANDALONE`.

## Reproducing the parity claim

```sh
export PATH="$HOME/.local/bin:$PATH"

# 1. Write the Rust compiler output for every group.
RETRO_DUMP_DIR=/tmp/rust-groups cargo test -p retro-script --test compiler_assets \
    -- --ignored --nocapture dump_compiled_groups

# 2. Write the reference compiler output for every group.
tools/script-oracle/run.sh /home/ted/projects/assets/S1 --all /tmp/oracle/S1 origins
tools/script-oracle/run.sh /home/ted/projects/assets/S2 --all /tmp/oracle/S2 origins

# 3. Compare (71 of 72 groups are byte-identical).
diff -r /tmp/rust-groups/S1 /tmp/oracle/S1
diff -r /tmp/rust-groups/S2 /tmp/oracle/S2
```

Expected result: every file matches except `S2/Mission_Zone02.bin`. That group differs by one jump
word because the reference stores alias names in `char name[0x20]`; the 32-character
`EGGMANSIGNPOST_SPAWNFALLSIGNPOST` leaves no room for a NUL terminator, so the reference's
`StrComp` reads into the adjacent `value` field and fails to resolve the alias (its switch case
keeps the default position 921). This port resolves it like the original tools (case body 364), and
the shipped `_Bytecode/Mission_Zone02.bin` agrees with this port. The same 71/72 result holds for
`standalone`.

The committed blake3 goldens in `crates/retro-script/tests/compiler_assets.rs`
(`ORACLE_PARITY_HASHES`, `PLATFORM_MODE_HASHES`) are checked by the `oracle_parity_hashes` and
`platform_modes_compile_the_expected_title_and_hud_paths` tests, so a compiler change that alters
the output fails CI until the hashes are refreshed.
