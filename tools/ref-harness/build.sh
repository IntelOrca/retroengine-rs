#!/usr/bin/env bash
# Build the deterministic C++ reference harness (RSDKv5U, RETRO_REVISION=3) plus its helpers.
#
# Everything is fetched at run time from pinned upstream revisions and built with user-local
# tooling (no sudo, no system SDL2/pkg-config required). The build directory defaults to
# ~/.cache/ref-harness and can be overridden with REF_HARNESS_BUILD (use a scratch dir such as
# /tmp/opencode/refbuild on machines with a small home).
#
# Requirements: bash, git, cmake >= 3.10, ninja, a C/C++ compiler, curl (git uses it for HTTPS),
# network access to github.com.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD="${REF_HARNESS_BUILD:-$HOME/.cache/ref-harness}"
PREFIX="$BUILD/prefix"

SDL2_REPO="https://github.com/libsdl-org/SDL.git"
# `release-2.32.10`
SDL2_REF="5d249570393f7a37e037abf22cd6012a4cc56a71"
RSDK5_REPO="https://github.com/RSDKModding/RSDKv5-Decompilation.git"
RSDK5_REF="43d426f8427c5553dab72afc02354e275f9ace48"
BLAKE3_REPO="https://github.com/BLAKE3-team/BLAKE3.git"
# `1.5.5`
BLAKE3_REF="81f772a4cd70dc0325047a6a737d2f6f4b92180e"

# cmake/ninja may live in ~/.local/bin on the harness machine.
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"

log() { printf '[ref-harness] %s\n' "$*" >&2; }

# ninja ignores CMAKE_BUILD_PARALLEL_LEVEL (it is only honoured by `cmake --build`), so map it
# onto `ninja -j` explicitly to keep memory bounded on small hosts.
ninja_jobs() {
    if [ -n "${CMAKE_BUILD_PARALLEL_LEVEL:-}" ]; then
        ninja -j "$CMAKE_BUILD_PARALLEL_LEVEL" "$@"
    else
        ninja "$@"
    fi
}

need() {
    command -v "$1" >/dev/null 2>&1 || {
        log "missing required tool: $1"
        exit 1
    }
}
need git
need cmake
need ninja
need cc
need c++

fetch_repo() { # dir url ref
    local dir="$1" url="$2" ref="$3"
    if [ ! -d "$dir/.git" ]; then
        log "cloning $url"
        git init -q "$dir"
        git -C "$dir" remote add origin "$url" 2>/dev/null || true
    fi
    log "fetching $url @ $ref"
    git -C "$dir" fetch -q --depth 1 origin "$ref"
    git -C "$dir" checkout -q --force FETCH_HEAD
    if [ -f "$dir/.gitmodules" ]; then
        git -C "$dir" submodule update -q --init --depth 1 --recursive
    fi
}

mkdir -p "$BUILD"

# ---------------------------------------------------------------------------
# 1. SDL2 (video/audio/input backend; offscreen video + dummy audio at run time)
# ---------------------------------------------------------------------------
fetch_repo "$BUILD/sdl2" "$SDL2_REPO" "$SDL2_REF"
if [ ! -f "$PREFIX/lib/libSDL2.so" ]; then
    log "building SDL2 -> $PREFIX"
    cmake -S "$BUILD/sdl2" -B "$BUILD/sdl2-build" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$PREFIX" \
        -DSDL_SHARED=ON -DSDL_STATIC=OFF -DSDL_TEST=OFF -DSDL_TESTS=OFF \
        -DCMAKE_POLICY_VERSION_MINIMUM=3.5
    ninja_jobs -C "$BUILD/sdl2-build"
    ninja_jobs -C "$BUILD/sdl2-build" install
else
    log "SDL2 already installed in $PREFIX"
fi

# ---------------------------------------------------------------------------
# 2. BLAKE3 (framebuffer/state hashes; C sources pinned at 1.5.5)
# ---------------------------------------------------------------------------
fetch_repo "$BUILD/blake3" "$BLAKE3_REPO" "$BLAKE3_REF"
BLAKE3_DIR="$BUILD/blake3/c"

# ---------------------------------------------------------------------------
# 3. RSDKv5-Decompilation + parity patches
# ---------------------------------------------------------------------------
fetch_repo "$BUILD/rsdkv5" "$RSDK5_REPO" "$RSDK5_REF"
log "applying $(ls "$ROOT"/patches/*.patch | wc -l) parity patches"
git -C "$BUILD/rsdkv5" reset -q --hard FETCH_HEAD
git -C "$BUILD/rsdkv5" clean -qfd
for patch in "$ROOT"/patches/*.patch; do
    git -C "$BUILD/rsdkv5" apply --whitespace=nowarn "$patch"
done

# ---------------------------------------------------------------------------
# 4. Configure + build RSDKv5U with the harness enabled
# ---------------------------------------------------------------------------
CMAKE_ARGS=(
    -S "$BUILD/rsdkv5"
    -B "$BUILD/rsdkv5-build"
    -G Ninja
    -DCMAKE_BUILD_TYPE=Release
    -DCMAKE_PREFIX_PATH="$PREFIX"
    -DRETRO_SUBSYSTEM=SDL2
    -DRETRO_MOD_LOADER=OFF
    -DRETRO_HARNESS=ON
    -DBLAKE3_DIR="$BLAKE3_DIR"
    -DCMAKE_POLICY_VERSION_MINIMUM=3.5
)
# Prefer pkg-config when present; the portability patch handles its absence via SDL2Config.cmake.
if command -v pkg-config >/dev/null 2>&1; then
    PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}" cmake "${CMAKE_ARGS[@]}"
else
    cmake "${CMAKE_ARGS[@]}"
fi
ninja_jobs -C "$BUILD/rsdkv5-build"

# ---------------------------------------------------------------------------
# 5. hash565 helper (hashes Rust --dump-frames PNGs in the same format)
# ---------------------------------------------------------------------------
log "building hash565"
cc -O2 -I "$BLAKE3_DIR" \
    -DBLAKE3_NO_SSE2 -DBLAKE3_NO_SSE41 -DBLAKE3_NO_AVX2 -DBLAKE3_NO_AVX512 -DBLAKE3_NO_NEON \
    "$ROOT/hash565.c" "$BLAKE3_DIR/blake3.c" "$BLAKE3_DIR/blake3_dispatch.c" "$BLAKE3_DIR/blake3_portable.c" \
    -o "$BUILD/hash565"

cat > "$BUILD/env.sh" <<EOF
REF_HARNESS_BUILD="$BUILD"
REF_HARNESS_PREFIX="$PREFIX"
REF_HARNESS_BIN="$BUILD/rsdkv5-build/RSDKv5U"
REF_HARNESS_HASH565="$BUILD/hash565"
EOF

log "built: $BUILD/rsdkv5-build/RSDKv5U"
log "environment written to $BUILD/env.sh; use run.sh (it sources this automatically)"
