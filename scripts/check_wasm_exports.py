#!/usr/bin/env python3
"""Fail if a wasm binary exports a function whose name starts with ``test_``.

Issue #1308: the test-only helpers ``test_insert_period`` and
``test_set_last_claimed_idx`` mutate accounting state without authorization, so
they must not exist in the deployed contract. ``scripts/check_wasm_exports.sh``
builds the release wasm and runs this check in CI.

The check parses the wasm export section directly (no external tooling), so it
works with plain ``python3``.

Usage: check_wasm_exports.py <path-to-wasm>
Exit code 0 = no ``test_*`` export; 1 = leak found; 2 = bad input.
"""

from __future__ import annotations

import sys

WASM_MAGIC = b"\x00asm"
EXPORT_SECTION_ID = 7
FUNC_EXPORT_KIND = 0x00


def read_u32_leb128(data: bytes, pos: int) -> tuple[int, int]:
    """Read an unsigned LEB128 integer, returning (value, new_pos)."""
    result = 0
    shift = 0
    while True:
        if pos >= len(data):
            raise ValueError("truncated LEB128 integer")
        byte = data[pos]
        pos += 1
        result |= (byte & 0x7F) << shift
        if byte & 0x80 == 0:
            return result, pos
        shift += 7
        if shift > 35:
            raise ValueError("LEB128 integer too long")


def read_name(data: bytes, pos: int) -> tuple[str, int]:
    length, pos = read_u32_leb128(data, pos)
    end = pos + length
    if end > len(data):
        raise ValueError("truncated name")
    return data[pos:end].decode("utf-8"), end


def exports(data: bytes) -> list[tuple[str, int]]:
    """Return the (name, kind) pairs from the wasm export section."""
    if data[:4] != WASM_MAGIC:
        raise ValueError("not a wasm binary (bad magic)")
    pos = 8  # skip magic + version
    found: list[tuple[str, int]] = []
    while pos < len(data):
        section_id = data[pos]
        pos += 1
        size, pos = read_u32_leb128(data, pos)
        end = pos + size
        if end > len(data):
            raise ValueError("section extends past end of file")
        if section_id == EXPORT_SECTION_ID:
            count, pos = read_u32_leb128(data, pos)
            for _ in range(count):
                name, pos = read_name(data, pos)
                kind = data[pos]
                pos += 1
                _index, pos = read_u32_leb128(data, pos)
                found.append((name, kind))
        pos = end
    return found


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: check_wasm_exports.py <path-to-wasm>", file=sys.stderr)
        return 2

    with open(argv[1], "rb") as handle:
        data = handle.read()

    try:
        export_list = exports(data)
    except ValueError as error:
        print(f"error: could not parse {argv[1]}: {error}", file=sys.stderr)
        return 2

    if not export_list:
        print(f"error: {argv[1]} has no exports; unexpected wasm layout", file=sys.stderr)
        return 2

    leaked = sorted(name for name, kind in export_list if kind == FUNC_EXPORT_KIND and name.startswith("test_"))
    if leaked:
        print(
            "error: test-only functions exported by the deployed contract: "
            + ", ".join(leaked),
            file=sys.stderr,
        )
        return 1

    print(f"ok: {len(export_list)} exports, no test_* functions")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
