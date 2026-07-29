#!/usr/bin/env python3
"""
generate_event_sunset_json.py
─────────────────────────────
Reads docs/EVENT_SUNSET.yaml (the single source of truth) and writes
docs/EVENT_SUNSET.json for machine consumption by indexers and CI tooling.

Usage:
    python3 scripts/generate_event_sunset_json.py [--check]

Options:
    --check   Validate only – exit 1 if the on-disk JSON is stale or invalid.
              Used by CI to enforce that the JSON is always in sync with the YAML.
    --help    Show this message and exit.

Exit codes:
    0  Success (or --check passed with no drift)
    1  Validation error (missing sunset_epoch, duplicate topic, …)
    2  Drift detected (--check mode: JSON does not match what would be generated)

Security notes:
    - Input is loaded with yaml.safe_load() to prevent arbitrary code execution
      from untrusted YAML payloads.
    - Output JSON is written atomically (temp-file + rename) to avoid partially-
      written files being read by concurrent CI jobs.
    - No shell interpolation; all paths are built with pathlib.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
from pathlib import Path

try:
    import yaml  # PyYAML
except ImportError:
    print(
        "ERROR: PyYAML is not installed.  Run: pip install pyyaml",
        file=sys.stderr,
    )
    sys.exit(1)

# ── Paths ──────────────────────────────────────────────────────────────────

REPO_ROOT = Path(__file__).resolve().parent.parent
YAML_PATH = REPO_ROOT / "docs" / "EVENT_SUNSET.yaml"
JSON_PATH = REPO_ROOT / "docs" / "EVENT_SUNSET.json"

# ── Schema / validation ────────────────────────────────────────────────────

REQUIRED_ENTRY_FIELDS = {"topic", "deprecated_in", "sunset_epoch", "replacement"}
OPTIONAL_ENTRY_FIELDS = {"note"}
ALLOWED_ENTRY_FIELDS = REQUIRED_ENTRY_FIELDS | OPTIONAL_ENTRY_FIELDS


def validate(data: dict) -> list[str]:
    """
    Validate the parsed YAML structure.

    Returns a list of human-readable error strings.  An empty list means the
    data is valid.
    """
    errors: list[str] = []

    if not isinstance(data, dict):
        return ["Root document must be a YAML mapping."]

    schema_version = data.get("schema_version")
    if schema_version != 1:
        errors.append(
            f"schema_version must be 1, got {schema_version!r}."
        )

    deprecated = data.get("deprecated_events")
    if deprecated is None:
        errors.append("Missing required key 'deprecated_events'.")
        return errors

    if not isinstance(deprecated, list):
        errors.append("'deprecated_events' must be a list.")
        return errors

    seen_topics: set[str] = set()

    for i, entry in enumerate(deprecated):
        prefix = f"deprecated_events[{i}]"

        if not isinstance(entry, dict):
            errors.append(f"{prefix}: entry must be a mapping, got {type(entry).__name__}.")
            continue

        # Unknown fields
        unknown = set(entry.keys()) - ALLOWED_ENTRY_FIELDS
        if unknown:
            errors.append(
                f"{prefix}: unknown field(s) {sorted(unknown)!r}. "
                f"Allowed: {sorted(ALLOWED_ENTRY_FIELDS)!r}."
            )

        # Required fields present
        for field in sorted(REQUIRED_ENTRY_FIELDS):
            if field not in entry:
                errors.append(f"{prefix}: missing required field '{field}'.")

        # topic must be a non-empty string
        topic = entry.get("topic")
        if topic is not None:
            if not isinstance(topic, str) or not topic.strip():
                errors.append(f"{prefix}.topic: must be a non-empty string.")
            else:
                # Duplicate check
                if topic in seen_topics:
                    errors.append(
                        f"{prefix}.topic: duplicate topic '{topic}' – each topic "
                        f"must appear at most once."
                    )
                seen_topics.add(topic)

        # sunset_epoch must be a positive integer
        sunset = entry.get("sunset_epoch")
        if sunset is not None:
            if not isinstance(sunset, int) or isinstance(sunset, bool):
                errors.append(
                    f"{prefix}.sunset_epoch: must be an integer, got {type(sunset).__name__}."
                )
            elif sunset <= 0:
                errors.append(
                    f"{prefix}.sunset_epoch: must be > 0, got {sunset}."
                )
        else:
            # null / absent already caught by required-fields check above,
            # but add a more descriptive message if the key exists but is null.
            if "sunset_epoch" in entry:
                errors.append(
                    f"{prefix}.sunset_epoch: must not be null – every deprecated "
                    f"topic must have a scheduled sunset ledger sequence."
                )

        # replacement must be a non-empty string
        replacement = entry.get("replacement")
        if replacement is not None and (
            not isinstance(replacement, str) or not replacement.strip()
        ):
            errors.append(
                f"{prefix}.replacement: must be a non-empty string."
            )

        # deprecated_in must be a non-empty string
        dep_in = entry.get("deprecated_in")
        if dep_in is not None and (
            not isinstance(dep_in, str) or not dep_in.strip()
        ):
            errors.append(
                f"{prefix}.deprecated_in: must be a non-empty string."
            )

    return errors


# ── JSON generation ────────────────────────────────────────────────────────

def build_output(data: dict) -> dict:
    """
    Convert the validated YAML dict into the canonical JSON structure.

    The JSON format is intentionally minimal and stable:
    - `schema_version` is forwarded as-is.
    - Each deprecated entry is output with a fixed key ordering for stable diffs.
    - The `note` field is included only when present (no null padding).
    - `removed_events` is forwarded unchanged (may be an empty list).
    """
    output_entries = []
    for entry in data.get("deprecated_events", []):
        out: dict = {
            "topic": entry["topic"],
            "deprecated_in": entry["deprecated_in"],
            "sunset_epoch": entry["sunset_epoch"],
            "replacement": entry["replacement"],
        }
        # Include `note` only when explicitly set (cleaner diff when absent).
        note = entry.get("note")
        if note is not None:
            # YAML block scalars include a trailing newline; strip it for JSON.
            out["note"] = note.strip()
        output_entries.append(out)

    removed = data.get("removed_events", [])
    if removed is None:
        removed = []

    return {
        "schema_version": data["schema_version"],
        "deprecated_events": output_entries,
        "removed_events": removed,
    }


def serialise(output: dict) -> str:
    """Return the canonical JSON string (UTF-8, 2-space indent, sorted keys)."""
    return json.dumps(output, indent=2, ensure_ascii=False, sort_keys=False) + "\n"


# ── I/O helpers ───────────────────────────────────────────────────────────

def atomic_write(path: Path, content: str) -> None:
    """
    Write *content* to *path* atomically using a sibling temp file + os.replace().

    This avoids leaving a partially-written JSON file if the process is
    interrupted mid-write (which could break concurrent CI jobs that read the
    file).
    """
    dir_ = path.parent
    with tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        dir=dir_,
        delete=False,
        suffix=".tmp",
    ) as tmp:
        tmp.write(content)
        tmp_path = tmp.name
    try:
        os.replace(tmp_path, path)
    except Exception:
        os.unlink(tmp_path)
        raise


# ── Main ──────────────────────────────────────────────────────────────────

def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="generate_event_sunset_json",
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help=(
            "Validate only – exit 2 if the on-disk JSON is stale "
            "compared to what would be generated from the YAML."
        ),
    )
    args = parser.parse_args(argv)

    # ── Load YAML ──────────────────────────────────────────────────────────
    if not YAML_PATH.exists():
        print(f"ERROR: YAML source not found: {YAML_PATH}", file=sys.stderr)
        return 1

    try:
        with YAML_PATH.open(encoding="utf-8") as fh:
            data = yaml.safe_load(fh)
    except yaml.YAMLError as exc:
        print(f"ERROR: Failed to parse YAML: {exc}", file=sys.stderr)
        return 1

    # ── Validate ───────────────────────────────────────────────────────────
    errors = validate(data)
    if errors:
        print("VALIDATION ERRORS in docs/EVENT_SUNSET.yaml:", file=sys.stderr)
        for err in errors:
            print(f"  • {err}", file=sys.stderr)
        return 1

    # ── Build output ───────────────────────────────────────────────────────
    output = build_output(data)
    json_str = serialise(output)

    if args.check:
        # Compare against what is already on disk.
        if not JSON_PATH.exists():
            print(
                "ERROR: docs/EVENT_SUNSET.json does not exist. "
                "Run 'python3 scripts/generate_event_sunset_json.py' to generate it.",
                file=sys.stderr,
            )
            return 2

        existing = JSON_PATH.read_text(encoding="utf-8")
        if existing == json_str:
            print("OK: docs/EVENT_SUNSET.json is up-to-date.")
            return 0
        else:
            print(
                "ERROR: docs/EVENT_SUNSET.json is stale. "
                "Run 'python3 scripts/generate_event_sunset_json.py' to regenerate it.",
                file=sys.stderr,
            )
            return 2

    # ── Write JSON ─────────────────────────────────────────────────────────
    atomic_write(JSON_PATH, json_str)
    n = len(output["deprecated_events"])
    try:
        display_path = JSON_PATH.relative_to(REPO_ROOT)
    except ValueError:
        display_path = JSON_PATH
    print(
        f"Generated {display_path} "
        f"({n} deprecated event{'s' if n != 1 else ''})."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
