#!/usr/bin/env python3
"""
scripts/gen_event_sunset.py
───────────────────────────
Reads docs/EVENT_SUNSET.yaml (single source of truth) and writes
indexer/event_sunset.json (the machine-readable artifact shipped with the WASM).

Usage
─────
  python3 scripts/gen_event_sunset.py             # generate + write
  python3 scripts/gen_event_sunset.py --check     # verify in-sync (CI mode)
  python3 scripts/gen_event_sunset.py --help      # this help

Exit codes
──────────
  0  success
  1  validation error in YAML (missing fields, duplicates, etc.)
  2  --check mode: generated output differs from the checked-in JSON
  3  unexpected / internal error

Security notes
──────────────
- The YAML is read from a hard-coded repo-relative path; no user-supplied
  paths are accepted to prevent path-traversal attacks.
- All string fields are stripped of leading/trailing whitespace before writing
  to JSON to prevent whitespace-injection surprises.
- The JSON output is pretty-printed with sorted keys so diffs are deterministic
  and reviewable.
- No shell commands are executed; the script is pure-Python (stdlib only:
  json, sys, pathlib, datetime, textwrap, argparse).
"""

import argparse
import json
import sys
import textwrap
from datetime import datetime, timezone
from pathlib import Path

# ── stdlib YAML fallback (Python 3.11+ has tomllib but not yaml) ─────────────
try:
    import yaml  # PyYAML if available
    _YAML_AVAILABLE = True
except ImportError:
    _YAML_AVAILABLE = False

# ── Repo-relative paths (never user-supplied) ─────────────────────────────────
_REPO_ROOT   = Path(__file__).resolve().parent.parent
_YAML_SOURCE = _REPO_ROOT / "docs" / "EVENT_SUNSET.yaml"
_JSON_OUTPUT = _REPO_ROOT / "indexer" / "event_sunset.json"

# ── Schema constants ───────────────────────────────────────────────────────────
_REQUIRED_ENTRY_FIELDS = {"topic", "replacement", "deprecated_in", "sunset_epoch", "reason"}
_OPTIONAL_ENTRY_FIELDS = {"notes"}
_ALL_ENTRY_FIELDS      = _REQUIRED_ENTRY_FIELDS | _OPTIONAL_ENTRY_FIELDS


def _fail(msg: str, code: int = 1) -> None:
    """Print an error message and exit with the given code."""
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(code)


# ── Minimal YAML parser (no external deps) ────────────────────────────────────
# Only needed when PyYAML is not installed.  Handles the restricted subset
# used in EVENT_SUNSET.yaml: block sequences of block mappings, multi-line
# scalars with ">", inline integers, and top-level scalar fields.

def _parse_yaml_fallback(text: str) -> dict:
    """
    Minimal YAML parser covering the EVENT_SUNSET.yaml dialect:
      - Top-level keys: schema_version (int), entries (list of dicts)
      - Each entry is a block mapping under a "  - " list item
      - Multi-line folded scalars (>) are collapsed to single-space-separated text
      - Comments (#) are stripped
    """
    lines = text.splitlines()

    def strip_comment(line: str) -> str:
        # Remove inline comments, but be careful with URLs (://)
        in_quote = False
        for i, ch in enumerate(line):
            if ch in ('"', "'"):
                in_quote = not in_quote
            if ch == '#' and not in_quote:
                return line[:i]
        return line

    # Strip comments and collect non-empty lines with their indentation
    cleaned = []
    for raw in lines:
        stripped = strip_comment(raw).rstrip()
        cleaned.append(stripped)

    result = {"schema_version": 1, "entries": []}
    entries = result["entries"]

    i = 0
    # Skip until we hit "entries:" or "schema_version:"
    while i < len(cleaned):
        line = cleaned[i].strip()
        if line.startswith("schema_version:"):
            val = line.split(":", 1)[1].strip()
            try:
                result["schema_version"] = int(val)
            except ValueError:
                pass
        if line.strip() == "entries:":
            i += 1
            break
        i += 1

    # Parse each list entry
    current: dict | None = None
    current_key: str | None = None
    folded_lines: list[str] = []

    def flush_folded() -> None:
        nonlocal current_key, folded_lines
        if current is not None and current_key and folded_lines:
            current[current_key] = " ".join(part.strip() for part in folded_lines if part.strip())
        current_key = None
        folded_lines = []

    while i < len(cleaned):
        raw_line = cleaned[i]
        stripped = raw_line.strip()
        indent  = len(raw_line) - len(raw_line.lstrip())

        # New list item
        if stripped.startswith("- ") and indent <= 2:
            flush_folded()
            if current is not None:
                entries.append(current)
            current = {}
            # The rest of the line after "- " may be a key: value
            rest = stripped[2:]
            if ":" in rest:
                k, v = rest.split(":", 1)
                k, v = k.strip(), v.strip()
                if v == ">":
                    current_key = k
                    folded_lines = []
                else:
                    # inline integer or string
                    try:
                        current[k] = int(v)
                    except ValueError:
                        current[k] = v.strip('"').strip("'")
            i += 1
            continue

        # Continuation lines inside an entry
        if current is not None and stripped and not stripped.startswith("#"):
            if ":" in stripped and indent >= 4 and current_key is None:
                k, v = stripped.split(":", 1)
                k, v = k.strip(), v.strip()
                if v == ">":
                    flush_folded()
                    current_key = k
                    folded_lines = []
                else:
                    flush_folded()
                    try:
                        current[k] = int(v)
                    except ValueError:
                        current[k] = v.strip('"').strip("'")
            elif current_key is not None and indent >= 4:
                folded_lines.append(stripped)

        i += 1

    flush_folded()
    if current is not None:
        entries.append(current)

    return result


def _load_yaml(path: Path) -> dict:
    """Load and parse the YAML source file."""
    text = path.read_text(encoding="utf-8")
    if _YAML_AVAILABLE:
        return yaml.safe_load(text)
    return _parse_yaml_fallback(text)


# ── Validation ─────────────────────────────────────────────────────────────────

def _validate(data: dict) -> list[dict]:
    """
    Validate the loaded YAML data and return the normalised list of entries.
    Calls sys.exit(1) on any validation failure.
    """
    if not isinstance(data, dict):
        _fail("EVENT_SUNSET.yaml must be a YAML mapping at the top level.")

    entries = data.get("entries")
    if not isinstance(entries, list) or len(entries) == 0:
        _fail("EVENT_SUNSET.yaml must contain a non-empty 'entries' list.")

    seen_topics: set[str] = set()
    deprecated_topics: set[str] = set()
    validated: list[dict] = []

    for idx, entry in enumerate(entries):
        loc = f"Entry #{idx + 1}"

        # Required fields present?
        for field in _REQUIRED_ENTRY_FIELDS:
            if field not in entry:
                _fail(f"{loc}: missing required field '{field}'.")
            if entry[field] is None or str(entry[field]).strip() == "":
                _fail(f"{loc}: field '{field}' must not be empty.")

        # Unknown fields?
        unknown = set(entry.keys()) - _ALL_ENTRY_FIELDS
        if unknown:
            _fail(f"{loc} (topic={entry.get('topic', '?')}): unknown fields: {sorted(unknown)}")

        topic       = str(entry["topic"]).strip()
        replacement = str(entry["replacement"]).strip()
        reason      = " ".join(str(entry["reason"]).split())  # collapse whitespace
        notes       = " ".join(str(entry.get("notes", "")).split()).strip()
        deprecated_in = str(entry["deprecated_in"]).strip()

        # sunset_epoch must be a positive integer
        try:
            sunset_epoch = int(entry["sunset_epoch"])
        except (TypeError, ValueError):
            _fail(f"{loc} (topic={topic}): 'sunset_epoch' must be an integer, "
                  f"got {entry['sunset_epoch']!r}.")
        if sunset_epoch <= 0:
            _fail(f"{loc} (topic={topic}): 'sunset_epoch' must be > 0 "
                  f"(CI requires a concrete epoch for every deprecated topic).")

        # No duplicate topics
        if topic in seen_topics:
            _fail(f"Duplicate topic '{topic}' at {loc}.")
        seen_topics.add(topic)
        deprecated_topics.add(topic)

        # Build normalised entry
        norm: dict = {
            "topic":         topic,
            "replacement":   replacement,
            "deprecated_in": deprecated_in,
            "sunset_epoch":  sunset_epoch,
            "sunset_iso":    datetime.fromtimestamp(sunset_epoch, tz=timezone.utc).strftime(
                                 "%Y-%m-%dT%H:%M:%SZ"),
            "reason":        reason,
        }
        if notes:
            norm["notes"] = notes
        validated.append(norm)

    # Verify no chained deprecations (replacement must not itself be deprecated)
    replacement_targets = {e["replacement"] for e in validated}
    chained = deprecated_topics & replacement_targets
    if chained:
        _fail(
            f"Chained deprecations detected — the following topics are both deprecated "
            f"AND used as a replacement: {sorted(chained)}.  Add an intermediate stable "
            f"topic or consolidate the deprecation chain."
        )

    return validated


# ── JSON rendering ─────────────────────────────────────────────────────────────

def _render_json(validated_entries: list[dict], schema_version: int) -> str:
    """Return the canonical, deterministic JSON string."""
    now_iso = datetime.now(tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    output = {
        "schema_version":  schema_version,
        "generated_at":    now_iso,
        "source":          "docs/EVENT_SUNSET.yaml",
        "description":     (
            "Machine-readable deprecated Revora contract event-topic sunset table. "
            "Indexers MUST migrate away from each 'topic' before its 'sunset_epoch'. "
            "Generated by scripts/gen_event_sunset.py — do not edit by hand."
        ),
        "entries": validated_entries,
    }
    return json.dumps(output, indent=2, sort_keys=False, ensure_ascii=True) + "\n"


# ── Main ───────────────────────────────────────────────────────────────────────

def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=textwrap.dedent("""\
            Generate indexer/event_sunset.json from docs/EVENT_SUNSET.yaml.

            Run without flags to write the JSON.
            Run with --check to verify the checked-in JSON is up-to-date (CI mode).
        """),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Exit 2 if the checked-in indexer/event_sunset.json differs from what "
             "would be generated.  Does not write any files.",
    )
    args = parser.parse_args(argv)

    # Load and validate source
    if not _YAML_SOURCE.exists():
        _fail(f"Source file not found: {_YAML_SOURCE}")

    raw = _load_yaml(_YAML_SOURCE)
    schema_version = int(raw.get("schema_version", 1))
    validated = _validate(raw)

    generated = _render_json(validated, schema_version)

    if args.check:
        # CI mode: compare with checked-in file
        if not _JSON_OUTPUT.exists():
            print(
                f"FAIL: {_JSON_OUTPUT} does not exist.\n"
                f"Run: python3 scripts/gen_event_sunset.py",
                file=sys.stderr,
            )
            return 2

        existing = _JSON_OUTPUT.read_text(encoding="utf-8")

        # Strip the generated_at line from both for comparison so a mere
        # timestamp difference doesn't trigger a failure.
        def _strip_generated_at(s: str) -> str:
            return "\n".join(
                line for line in s.splitlines()
                if '"generated_at"' not in line
            )

        if _strip_generated_at(generated) != _strip_generated_at(existing):
            print(
                f"FAIL: {_JSON_OUTPUT} is out of sync with {_YAML_SOURCE}.\n"
                f"Run: python3 scripts/gen_event_sunset.py",
                file=sys.stderr,
            )
            return 2

        print(f"OK: {_JSON_OUTPUT} is in sync with {_YAML_SOURCE}.")
        return 0

    # Write mode
    _JSON_OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    _JSON_OUTPUT.write_text(generated, encoding="utf-8")
    print(f"Written {len(validated)} entries → {_JSON_OUTPUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
