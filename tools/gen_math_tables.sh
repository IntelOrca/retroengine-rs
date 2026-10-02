#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
output="${TMPDIR:-/tmp}/gen_math_tables"

cc "$root/tools/gen_math_tables.c" -o "$output" -lm
"$output" "$root/crates/retro-core/src/math_tables.rs"
rm -f "$output"
(cd "$root" && cargo fmt)
