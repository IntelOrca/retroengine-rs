#!/usr/bin/env python3
"""Compare two reference-harness JSONL frame record streams and report the first divergence.

Usage:
    diff_records.py A.jsonl B.jsonl [--offset N] [--all] [--entities N]

A frame record A[i] is compared against B[i + offset] (default offset 0). The tool walks both
streams frame by frame and stops at the first divergence, reporting the frame index, the field
(or entity slot/field) that differs, and the two values. Exit status is 1 when any difference is
found, 0 when the streams match for the overlapping length.

Record format: see tools/ref-harness/README.md.
"""

from __future__ import annotations

import argparse
import json
import sys
from typing import Any

ENTITY_FIELDS = [
    "slot",
    "type",
    "name",
    "propertyValue",
    "groupID",
    "xpos",
    "ypos",
    "xvel",
    "yvel",
    "speed",
    "state",
    "angle",
    "scale",
    "rotation",
    "alpha",
    "animationTimer",
    "animationSpeed",
    "priority",
    "drawOrder",
    "direction",
    "inkEffect",
    "animation",
    "prevAnimation",
    "frame",
    "collisionMode",
    "collisionPlane",
    "gravity",
    "controlMode",
    "controlLock",
]

SCALAR_FIELDS = ["state", "cat", "list", "catname", "folder", "sceneid", "scenename"]


def load(path: str) -> list[dict[str, Any]]:
    rows = []
    with open(path, "r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def first_diff(a: Any, b: Any, path: str = "") -> tuple[str, Any, Any] | None:
    """Returns (field path, a value, b value) for the first mismatch, or None."""
    if isinstance(a, dict) and isinstance(b, dict):
        for key in a:
            if key not in b:
                return (f"{path}.{key}", a[key], "<missing>")
            found = first_diff(a[key], b[key], f"{path}.{key}")
            if found:
                return found
        for key in b:
            if key not in a:
                return (f"{path}.{key}", "<missing>", b[key])
        return None
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            return (f"{path}.length", len(a), len(b))
        for index, (item_a, item_b) in enumerate(zip(a, b)):
            found = first_diff(item_a, item_b, f"{path}[{index}]")
            if found:
                return found
        return None
    if a != b:
        return (path, a, b)
    return None


def entity_map(record: dict[str, Any]) -> dict[int, list[Any]]:
    entities = record.get("v4", {}).get("ents", [])
    return {entity[0]: entity for entity in entities}


def compare_entities(
    frame: int, a: dict[str, Any], b: dict[str, Any], max_report: int, report_all: bool
) -> bool:
    entities_a = entity_map(a)
    entities_b = entity_map(b)
    slots = sorted(set(entities_a) | set(entities_b))
    differences = 0
    for slot in slots:
        entity_a = entities_a.get(slot)
        entity_b = entities_b.get(slot)
        if entity_a is None or entity_b is None:
            side = "A" if entity_a else "B"
            print(f"frame {frame}: entity slot {slot} only in {side}")
            differences += 1
        else:
            found = first_diff(entity_a, entity_b)
            if found:
                field, value_a, value_b = found
                print(f"frame {frame}: entity slot {slot} {field}: A={value_a!r} B={value_b!r}")
                differences += 1
        if differences >= max_report and not report_all:
            print(f"frame {frame}: ... {len(slots) - differences} more entity differences suppressed (--entities)")
            break
    return differences > 0


def compare_frame(
    index_a: int,
    a: dict[str, Any],
    index_b: int,
    b: dict[str, Any],
    max_entities: int,
    report_all: bool,
) -> bool:
    header_a = {key: a.get(key) for key in SCALAR_FIELDS}
    header_b = {key: b.get(key) for key in SCALAR_FIELDS}
    header_a["fb"] = a.get("fb", {}).get("blake3")
    header_b["fb"] = b.get("fb", {}).get("blake3")
    found = first_diff(header_a, header_b)
    if found:
        field, value_a, value_b = found
        print(f"frame {index_a} (B frame {index_b}): {field}: A={value_a!r} B={value_b!r}")
        return True

    if a.get("ver") != b.get("ver"):
        print(f"frame {index_a} (B frame {index_b}): ver: A={a.get('ver')} B={b.get('ver')}")
        return True

    found = first_diff(a.get("v4"), b.get("v4", {}), "v4")
    if found:
        field, value_a, value_b = found
        # Keep the report focused: the player path is the common culprit.
        print(f"frame {index_a} (B frame {index_b}): {field}: A={value_a!r} B={value_b!r}")
        return True

    found = first_diff(a.get("scene3D"), b.get("scene3D"), "scene3D")
    if found:
        field, value_a, value_b = found
        print(f"frame {index_a} (B frame {index_b}): {field}: A={value_a!r} B={value_b!r}")
        return True

    return compare_entities(index_a, a, b, max_entities, report_all)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("a", help="baseline record stream")
    parser.add_argument("b", help="candidate record stream")
    parser.add_argument("--offset", type=int, default=0, help="candidate frame offset (default 0)")
    parser.add_argument("--all", action="store_true", help="report every divergent frame, not just the first")
    parser.add_argument("--entities", type=int, default=8, help="max entity differences reported per frame")
    args = parser.parse_args()

    rows_a = load(args.a)
    rows_b = load(args.b)
    common = min(len(rows_a), max(0, len(rows_b) - args.offset))
    divergent = 0
    for index in range(common):
        if compare_frame(index, rows_a[index], index + args.offset, rows_b[index + args.offset], args.entities, args.all):
            divergent += 1
            if not args.all:
                break
    if not divergent:
        print(f"no divergence in {common} compared frames (A={args.a}, B={args.b}, offset={args.offset})")
        return 0
    print(f"divergent frames: {divergent} of {common} compared")
    return 1


if __name__ == "__main__":
    sys.exit(main())
