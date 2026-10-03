// hash565: BLAKE3 of a raw little-endian RGB565 stream (stdin -> hex on stdout).
//
// Used by tools/ref-harness/compare_frames.py to hash Rust `--dump-frames` PNGs with the same
// algorithm the C reference harness uses for its framebuffer records.

#include <stdio.h>
#include <stdlib.h>

#include "blake3.h"

int main(void)
{
    blake3_hasher hasher;
    blake3_hasher_init(&hasher);

    unsigned char buffer[65536];
    size_t read = 0;
    while ((read = fread(buffer, 1, sizeof(buffer), stdin)) > 0) blake3_hasher_update(&hasher, buffer, read);

    unsigned char digest[32];
    blake3_hasher_finalize(&hasher, digest, sizeof(digest));
    for (int i = 0; i < 32; ++i) printf("%02x", digest[i]);
    putchar('\n');
    return 0;
}
