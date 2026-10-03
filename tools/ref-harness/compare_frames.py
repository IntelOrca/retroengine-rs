#!/usr/bin/env python3
"""Compare reference-harness JSONL framebuffer hashes against Rust `--dump-frames` PNGs.

The Rust engine has no framebuffer-only hash output, but `retroengine ... --dump-frames DIR`
writes lossless RGB888 PNGs of the visible RGB565 framebuffer. This tool decodes those PNGs,
packs them back to RGB565 exactly, and hashes them with the `hash565` helper built by build.sh
(the same BLAKE3 the C reference writes into each record's `fb.blake3`).

Usage:
    compare_frames.py RECORDS.jsonl FRAMES_DIR [--offset N] [--hash565 PATH] [--frame N]
                      [--image A.ppm B.png]

A record stream frame R[i] is compared against `FRAMES_DIR/frame_{i+offset:04d}.png`.
Exit status is 1 on the first mismatch, 0 when all compared frames match.
"""

from __future__ import annotations

import argparse
import json
import os
import struct
import subprocess
import sys
import zlib
from pathlib import Path


def decode_png(path: Path) -> tuple[int, int, bytes]:
    """Decodes a non-interlaced 8-bit RGB PNG to packed RGB888 bytes."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: not a PNG")
    pos = 8
    idat = b""
    width = height = bit_depth = color_type = interlace = None
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        chunk_type = data[pos + 4 : pos + 8]
        chunk = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if chunk_type == b"IHDR":
            width, height, bit_depth, color_type, _comp, _filt, interlace = struct.unpack(">IIBBBBB", chunk)
        elif chunk_type == b"IDAT":
            idat += chunk
        elif chunk_type == b"IEND":
            break
    if bit_depth != 8 or color_type != 2 or interlace != 0:
        raise ValueError(f"{path}: expected 8-bit RGB, non-interlaced PNG")

    raw = zlib.decompress(idat)
    stride = width * 3
    out = bytearray()
    previous = bytearray(stride)
    cursor = 0
    for _ in range(height):
        filter_type = raw[cursor]
        cursor += 1
        line = bytearray(raw[cursor : cursor + stride])
        cursor += stride
        if filter_type == 1:
            for i in range(3, stride):
                line[i] = (line[i] + line[i - 3]) & 0xFF
        elif filter_type == 2:
            for i in range(stride):
                line[i] = (line[i] + previous[i]) & 0xFF
        elif filter_type == 3:
            for i in range(stride):
                left = line[i - 3] if i >= 3 else 0
                line[i] = (line[i] + ((left + previous[i]) >> 1)) & 0xFF
        elif filter_type == 4:
            for i in range(stride):
                left = line[i - 3] if i >= 3 else 0
                upper_left = previous[i - 3] if i >= 3 else 0
                up = previous[i]
                estimate = left + up - upper_left
                dist_left = abs(estimate - left)
                dist_up = abs(estimate - up)
                dist_corner = abs(estimate - upper_left)
                if dist_left <= dist_up and dist_left <= dist_corner:
                    predictor = left
                elif dist_up <= dist_corner:
                    predictor = up
                else:
                    predictor = upper_left
                line[i] = (line[i] + predictor) & 0xFF
        elif filter_type != 0:
            raise ValueError(f"{path}: unknown PNG filter {filter_type}")
        out += line
        previous = line
    return width, height, bytes(out)


def pack_rgb565(rgb: bytes) -> bytes:
    out = bytearray()
    for i in range(0, len(rgb), 3):
        value = ((rgb[i] >> 3) << 11) | ((rgb[i + 1] >> 2) << 5) | (rgb[i + 2] >> 3)
        out += struct.pack("<H", value)
    return bytes(out)


def hash565(hash565_bin: str, rgb565: bytes) -> str:
    result = subprocess.run([hash565_bin], input=rgb565, capture_output=True, check=True)
    return result.stdout.decode().strip()


def ppm_to_rgb565(path: Path) -> bytes:
    data = path.read_bytes()
    newline1 = data.find(b"\n")
    newline2 = data.find(b"\n", newline1 + 1)
    newline3 = data.find(b"\n", newline2 + 1)
    return pack_rgb565(data[newline3 + 1 :])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("records", nargs="?", help="reference record stream (JSONL)")
    parser.add_argument("frames_dir", nargs="?")
    parser.add_argument("--offset", type=int, default=0)
    parser.add_argument("--hash565", default=None, help="path to hash565 (defaults to $REF_HARNESS_HASH565)")
    parser.add_argument("--frame", type=int, default=None, help="only report this record frame")
    parser.add_argument("--image", nargs=2, metavar=("PPM", "PNG"), help="compare two images directly")
    args = parser.parse_args()

    if args.image:
        ppm_path, png_path = Path(args.image[0]), Path(args.image[1])
        ppm = ppm_to_rgb565(ppm_path)
        width, height, rgb = decode_png(png_path)
        png = pack_rgb565(rgb)
        if ppm == png:
            print(f"identical: {ppm_path.name} == {png_path.name}")
            return 0
        diffs = 0
        min_x, min_y, max_x, max_y = width, height, -1, -1
        for i in range(0, min(len(ppm), len(png)), 2):
            if ppm[i : i + 2] != png[i : i + 2]:
                diffs += 1
                pixel = i // 2
                x, y = pixel % width, pixel // width
                min_x, max_x = min(min_x, x), max(max_x, x)
                min_y, max_y = min(min_y, y), max(max_y, y)
        print(f"{width}x{height}: {diffs} differing pixels, bbox=({min_x},{min_y})-({max_x},{max_y})")
        return 1

    if not args.records or not args.frames_dir:
        parser.error("records and frames_dir are required unless --image is used")

    hash565_bin = args.hash565 or os.environ.get("REF_HARNESS_HASH565")
    if not hash565_bin:
        print("--hash565 or $REF_HARNESS_HASH565 is required", file=sys.stderr)
        return 2

    rows = []
    with open(args.records, "r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if line:
                rows.append(json.loads(line))

    frames_dir = Path(args.frames_dir)
    compared = 0
    first_divergence = None
    for index, record in enumerate(rows):
        png = frames_dir / f"frame_{index + args.offset:04d}.png"
        if not png.exists():
            break
        width, height, rgb = decode_png(png)
        digest = hash565(hash565_bin, pack_rgb565(rgb))
        compared += 1
        if digest != record["fb"]["blake3"]:
            if first_divergence is None:
                first_divergence = (index, record["f"], record["fb"]["blake3"], digest)
            if args.frame is None or args.frame == record["f"]:
                print(f"frame {record['f']}: A={record['fb']['blake3'][:16]} B={digest[:16]} DIFF")
    if first_divergence:
        index, frame, hash_a, hash_b = first_divergence
        print(f"first divergent frame: record {frame} (index {index}), A={hash_a[:16]} B={hash_b[:16]}")
        print(f"compared {compared} frames")
        return 1
    print(f"no divergence in {compared} compared frames")
    return 0


if __name__ == "__main__":
    sys.exit(main())
