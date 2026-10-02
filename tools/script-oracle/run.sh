#!/bin/sh
# Builds and runs the reference RSDKv4 script compiler oracle.
#
# Fetches `Script.cpp` from RSDKv4-Decompilation at a pinned commit, extracts the compiler half
# (everything before `LoadBytecode`), applies the two harness patches, links it against the local
# stubs, and forwards all arguments to the resulting binary. Upstream code is fetched at run time
# and is never vendored into this repository.
#
# Usage:
#   tools/script-oracle/run.sh <gameRoot> <GLOBAL|stageFolder|--all> <out.bin|outDir> [origins|standalone]
#
# Example:
#   tools/script-oracle/run.sh /home/ted/projects/assets/S1 GLOBAL /tmp/GlobalCode.bin origins
#   tools/script-oracle/run.sh /home/ted/projects/assets/S2 --all /tmp/oracle-s2 standalone
#
# Environment:
#   SCRIPT_ORACLE_BUILD  build directory (default: <tool dir>/.build)

set -eu

here=$(cd "$(dirname "$0")" && pwd)
build=${SCRIPT_ORACLE_BUILD:-"$here/.build"}
commit=a7f5195e21fdad7b75e4587e249013feeea9e6f3
url="https://raw.githubusercontent.com/RSDKModding/RSDKv4-Decompilation/$commit/RSDKv4/Script.cpp"

mkdir -p "$build"

if [ ! -f "$build/Script.cpp" ]; then
    echo "fetching $url" >&2
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$build/Script.cpp"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$build/Script.cpp" "$url"
    else
        echo "error: curl or wget is required" >&2
        exit 1
    fi
fi

# Extract the compiler half: everything up to (but excluding) LoadBytecode.
awk '/^void LoadBytecode\(int stageListID, int scriptID\)/ { exit } { print }' \
    "$build/Script.cpp" > "$build/compiler_section.cpp"

# Patch 1: use the local stubs instead of the engine header.
sed -i 's|#include "RetroEngine.hpp"|#include "stubs.hpp"|' "$build/compiler_section.cpp"

# Patch 2: disable the decomp-only USE_DECOMP platform tag so `#platform` selection matches the
# original tools that produced the shipped `_Bytecode` files.
sed -i 's|FindStringToken(scriptText, "USE_DECOMP", 1) == -1|true|' "$build/compiler_section.cpp"

# The reference has fixed-size string buffers whose libc fortify checks (level >= 2) abort even
# though the compiler output is correct (verified under AddressSanitizer with fortify disabled).
# Build with fortify off so the oracle runs deterministically.
g++ -std=c++17 -O2 -U_FORTIFY_SOURCE -D_FORTIFY_SOURCE=0 -I"$build" -I"$here" \
    -o "$build/script-oracle" "$here/main.cpp"

exec "$build/script-oracle" "$@"
