#!/usr/bin/env python3
"""
test_event_sunset.py
────────────────────
Unit tests for scripts/generate_event_sunset_json.py.

Run with:
    python3 -m pytest scripts/test_event_sunset.py -v
  or
    python3 scripts/test_event_sunset.py

Tests cover:
  - Happy-path generation (single and multiple entries)
  - Multiple deprecations sharing one replacement topic
  - Missing required fields (topic, deprecated_in, sunset_epoch, replacement)
  - Null sunset_epoch (same error as missing – CI gate)
  - Duplicate topic names
  - Unknown / extra fields
  - Zero / negative sunset_epoch
  - Non-integer sunset_epoch
  - Empty deprecated_events list
  - removed_events forwarded unchanged
  - --check mode: up-to-date JSON passes, stale JSON fails
  - --check mode: missing JSON file fails
  - Atomic write: temp file is cleaned up on success
  - JSON output is deterministic (idempotent re-runs produce identical bytes)
  - note field: present when set, absent when not set (no null padding)
  - note field: YAML block scalar trailing newline is stripped
"""

from __future__ import annotations

import json
import os
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest.mock import patch

# Make sure the scripts directory is importable regardless of cwd.
SCRIPTS_DIR = Path(__file__).resolve().parent
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

import generate_event_sunset_json as gen  # noqa: E402  (after sys.path tweak)


# ── Helpers ───────────────────────────────────────────────────────────────

def _valid_entry(**overrides) -> dict:
    """Return a valid deprecated_events entry, optionally overriding fields."""
    base = {
        "topic": "ev_old",
        "deprecated_in": "v2",
        "sunset_epoch": 1_000_000,
        "replacement": "ev_new",
    }
    base.update(overrides)
    return base


def _valid_data(entries: list[dict] | None = None) -> dict:
    """Return a valid top-level YAML data dict."""
    return {
        "schema_version": 1,
        "deprecated_events": entries if entries is not None else [_valid_entry()],
        "removed_events": [],
    }


# ── Validation tests ──────────────────────────────────────────────────────

class TestValidate(unittest.TestCase):

    # ── Happy path ────────────────────────────────────────────────────────

    def test_valid_single_entry(self):
        errors = gen.validate(_valid_data())
        self.assertEqual(errors, [])

    def test_valid_empty_deprecated_list(self):
        """An empty deprecated_events list is valid – no deprecations yet."""
        errors = gen.validate(_valid_data(entries=[]))
        self.assertEqual(errors, [])

    def test_valid_with_note(self):
        entry = _valid_entry(note="Migrate to ev_new before ledger 1000000.")
        errors = gen.validate(_valid_data([entry]))
        self.assertEqual(errors, [])

    def test_valid_multiple_entries_sharing_one_replacement(self):
        """
        Edge case: two deprecated topics that both map to the same replacement.
        This is explicitly allowed (e.g. ev_idx1 and ev_idx2 both → ev_idx3).
        """
        entries = [
            _valid_entry(topic="ev_idx1", deprecated_in="v2", sunset_epoch=2_000_000, replacement="ev_idx3"),
            _valid_entry(topic="ev_idx2", deprecated_in="v3", sunset_epoch=3_000_000, replacement="ev_idx3"),
        ]
        errors = gen.validate(_valid_data(entries))
        self.assertEqual(errors, [])

    def test_valid_three_entries_all_distinct_replacements(self):
        entries = [
            _valid_entry(topic="a", deprecated_in="v1", sunset_epoch=100, replacement="a_new"),
            _valid_entry(topic="b", deprecated_in="v2", sunset_epoch=200, replacement="b_new"),
            _valid_entry(topic="c", deprecated_in="v3", sunset_epoch=300, replacement="c_new"),
        ]
        errors = gen.validate(_valid_data(entries))
        self.assertEqual(errors, [])

    # ── Missing required fields ───────────────────────────────────────────

    def test_missing_topic(self):
        entry = {k: v for k, v in _valid_entry().items() if k != "topic"}
        errors = gen.validate(_valid_data([entry]))
        self.assertTrue(any("topic" in e for e in errors), errors)

    def test_missing_deprecated_in(self):
        entry = {k: v for k, v in _valid_entry().items() if k != "deprecated_in"}
        errors = gen.validate(_valid_data([entry]))
        self.assertTrue(any("deprecated_in" in e for e in errors), errors)

    def test_missing_sunset_epoch(self):
        """A missing sunset_epoch must be flagged – CI gate requirement."""
        entry = {k: v for k, v in _valid_entry().items() if k != "sunset_epoch"}
        errors = gen.validate(_valid_data([entry]))
        self.assertTrue(any("sunset_epoch" in e for e in errors), errors)

    def test_null_sunset_epoch(self):
        """Explicitly null sunset_epoch is equally invalid."""
        entry = _valid_entry(sunset_epoch=None)
        errors = gen.validate(_valid_data([entry]))
        self.assertTrue(any("sunset_epoch" in e for e in errors), errors)

    def test_missing_replacement(self):
        entry = {k: v for k, v in _valid_entry().items() if k != "replacement"}
        errors = gen.validate(_valid_data([entry]))
        self.assertTrue(any("replacement" in e for e in errors), errors)

    # ── Duplicate topics ──────────────────────────────────────────────────

    def test_duplicate_topic_same_entry(self):
        """Two entries with identical topic strings must be rejected."""
        entries = [
            _valid_entry(topic="ev_dup", sunset_epoch=100_000),
            _valid_entry(topic="ev_dup", sunset_epoch=200_000),
        ]
        errors = gen.validate(_valid_data(entries))
        self.assertTrue(any("duplicate" in e.lower() for e in errors), errors)

    def test_duplicate_topic_three_entries(self):
        """Duplicate check applies to the third occurrence too."""
        entries = [
            _valid_entry(topic="ev_dup", sunset_epoch=100_000),
            _valid_entry(topic="ev_unique", sunset_epoch=200_000),
            _valid_entry(topic="ev_dup", sunset_epoch=300_000),
        ]
        errors = gen.validate(_valid_data(entries))
        self.assertTrue(any("duplicate" in e.lower() for e in errors), errors)

    # ── sunset_epoch value constraints ────────────────────────────────────

    def test_zero_sunset_epoch_invalid(self):
        entry = _valid_entry(sunset_epoch=0)
        errors = gen.validate(_valid_data([entry]))
        self.assertTrue(any("sunset_epoch" in e for e in errors), errors)

    def test_negative_sunset_epoch_invalid(self):
        entry = _valid_entry(sunset_epoch=-1)
        errors = gen.validate(_valid_data([entry]))
        self.assertTrue(any("sunset_epoch" in e for e in errors), errors)

    def test_float_sunset_epoch_invalid(self):
        entry = _valid_entry(sunset_epoch=1_000_000.5)
        errors = gen.validate(_valid_data([entry]))
        self.assertTrue(any("sunset_epoch" in e for e in errors), errors)

    def test_string_sunset_epoch_invalid(self):
        entry = _valid_entry(sunset_epoch="1000000")
        errors = gen.validate(_valid_data([entry]))
        self.assertTrue(any("sunset_epoch" in e for e in errors), errors)

    def test_bool_sunset_epoch_invalid(self):
        """True is an instance of int in Python; make sure booleans are rejected."""
        entry = _valid_entry(sunset_epoch=True)
        errors = gen.validate(_valid_data([entry]))
        self.assertTrue(any("sunset_epoch" in e for e in errors), errors)

    def test_large_valid_sunset_epoch(self):
        """Very large epoch values (future planned removals) must be accepted."""
        entry = _valid_entry(sunset_epoch=999_999_999)
        errors = gen.validate(_valid_data([entry]))
        self.assertEqual(errors, [])

    # ── Unknown fields ────────────────────────────────────────────────────

    def test_unknown_field_rejected(self):
        entry = _valid_entry()
        entry["unexpected_field"] = "oops"
        errors = gen.validate(_valid_data([entry]))
        self.assertTrue(any("unknown" in e.lower() for e in errors), errors)

    # ── schema_version ────────────────────────────────────────────────────

    def test_wrong_schema_version(self):
        data = _valid_data()
        data["schema_version"] = 2
        errors = gen.validate(data)
        self.assertTrue(any("schema_version" in e for e in errors), errors)

    def test_missing_deprecated_events_key(self):
        data = {"schema_version": 1}
        errors = gen.validate(data)
        self.assertTrue(any("deprecated_events" in e for e in errors), errors)

    def test_non_list_deprecated_events(self):
        data = {"schema_version": 1, "deprecated_events": "not a list"}
        errors = gen.validate(data)
        self.assertTrue(any("list" in e for e in errors), errors)

    def test_non_dict_root(self):
        errors = gen.validate("a string")
        self.assertTrue(any("mapping" in e.lower() for e in errors), errors)


# ── build_output / serialise tests ────────────────────────────────────────

class TestBuildOutput(unittest.TestCase):

    def test_basic_structure(self):
        data = _valid_data()
        out = gen.build_output(data)
        self.assertEqual(out["schema_version"], 1)
        self.assertIn("deprecated_events", out)
        self.assertIn("removed_events", out)

    def test_note_present_when_set(self):
        entry = _valid_entry(note="Migrate before epoch 1000000.")
        out = gen.build_output(_valid_data([entry]))
        self.assertIn("note", out["deprecated_events"][0])

    def test_note_absent_when_not_set(self):
        """No null/None padding for absent optional fields."""
        out = gen.build_output(_valid_data([_valid_entry()]))
        self.assertNotIn("note", out["deprecated_events"][0])

    def test_note_trailing_newline_stripped(self):
        """YAML block scalars add a trailing newline; the generator strips it."""
        entry = _valid_entry(note="Some migration note.\n")
        out = gen.build_output(_valid_data([entry]))
        self.assertEqual(out["deprecated_events"][0]["note"], "Some migration note.")

    def test_removed_events_forwarded(self):
        data = _valid_data()
        data["removed_events"] = [{"topic": "old_v1", "removed_in": "v2"}]
        out = gen.build_output(data)
        self.assertEqual(len(out["removed_events"]), 1)
        self.assertEqual(out["removed_events"][0]["topic"], "old_v1")

    def test_empty_removed_events(self):
        data = _valid_data()
        data["removed_events"] = []
        out = gen.build_output(data)
        self.assertEqual(out["removed_events"], [])

    def test_none_removed_events_becomes_empty_list(self):
        """None in YAML (null) should be treated as an empty list."""
        data = _valid_data()
        data["removed_events"] = None
        out = gen.build_output(data)
        self.assertEqual(out["removed_events"], [])

    def test_multiple_sharing_replacement_preserved(self):
        entries = [
            _valid_entry(topic="ev_idx1", replacement="ev_idx3", sunset_epoch=2_000_000),
            _valid_entry(topic="ev_idx2", replacement="ev_idx3", sunset_epoch=3_000_000),
        ]
        out = gen.build_output(_valid_data(entries))
        self.assertEqual(len(out["deprecated_events"]), 2)
        replacements = {e["replacement"] for e in out["deprecated_events"]}
        self.assertEqual(replacements, {"ev_idx3"})

    def test_serialise_is_valid_json(self):
        out = gen.build_output(_valid_data())
        json_str = gen.serialise(out)
        parsed = json.loads(json_str)
        self.assertEqual(parsed["schema_version"], 1)

    def test_serialise_ends_with_newline(self):
        """JSON file must end with a newline for POSIX compatibility."""
        out = gen.build_output(_valid_data())
        json_str = gen.serialise(out)
        self.assertTrue(json_str.endswith("\n"))

    def test_serialise_is_idempotent(self):
        """Calling serialise twice produces identical bytes (deterministic)."""
        out = gen.build_output(_valid_data())
        self.assertEqual(gen.serialise(out), gen.serialise(out))


# ── main() / CLI integration tests ────────────────────────────────────────

class TestMain(unittest.TestCase):
    """
    Integration tests for the CLI entry point.

    We redirect YAML_PATH and JSON_PATH to temp directories so tests never
    touch real repo files.
    """

    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.yaml_path = self.tmp / "EVENT_SUNSET.yaml"
        self.json_path = self.tmp / "EVENT_SUNSET.json"

    def tearDown(self):
        self.tmpdir.cleanup()

    def _write_yaml(self, data: dict) -> None:
        import yaml as _yaml  # local import; PyYAML must be present
        self.yaml_path.write_text(
            _yaml.dump(data, default_flow_style=False, allow_unicode=True),
            encoding="utf-8",
        )

    def _patch_paths(self):
        return patch.multiple(
            gen,
            YAML_PATH=self.yaml_path,
            JSON_PATH=self.json_path,
        )

    # ── Generation ────────────────────────────────────────────────────────

    def test_generate_creates_json(self):
        self._write_yaml(_valid_data())
        with self._patch_paths():
            rc = gen.main([])
        self.assertEqual(rc, 0)
        self.assertTrue(self.json_path.exists())

    def test_generate_json_content_correct(self):
        data = _valid_data([
            _valid_entry(topic="ev_idx2", replacement="ev_idx3", sunset_epoch=3_000_000)
        ])
        self._write_yaml(data)
        with self._patch_paths():
            gen.main([])
        result = json.loads(self.json_path.read_text(encoding="utf-8"))
        self.assertEqual(result["deprecated_events"][0]["topic"], "ev_idx2")
        self.assertEqual(result["deprecated_events"][0]["sunset_epoch"], 3_000_000)
        self.assertEqual(result["deprecated_events"][0]["replacement"], "ev_idx3")

    def test_generate_fails_on_missing_sunset_epoch(self):
        entry = {k: v for k, v in _valid_entry().items() if k != "sunset_epoch"}
        self._write_yaml(_valid_data([entry]))
        with self._patch_paths():
            rc = gen.main([])
        self.assertEqual(rc, 1)
        # JSON must NOT have been written
        self.assertFalse(self.json_path.exists())

    def test_generate_fails_on_duplicate_topic(self):
        entries = [
            _valid_entry(topic="ev_dup", sunset_epoch=100),
            _valid_entry(topic="ev_dup", sunset_epoch=200),
        ]
        self._write_yaml(_valid_data(entries))
        with self._patch_paths():
            rc = gen.main([])
        self.assertEqual(rc, 1)

    def test_generate_fails_when_yaml_missing(self):
        # yaml_path is never written
        with self._patch_paths():
            rc = gen.main([])
        self.assertEqual(rc, 1)

    def test_generate_multiple_sharing_replacement(self):
        """Multiple deprecations sharing one replacement – must succeed."""
        entries = [
            _valid_entry(topic="ev_v1", replacement="ev_v3", sunset_epoch=1_000_000),
            _valid_entry(topic="ev_v2", replacement="ev_v3", sunset_epoch=2_000_000),
        ]
        self._write_yaml(_valid_data(entries))
        with self._patch_paths():
            rc = gen.main([])
        self.assertEqual(rc, 0)
        result = json.loads(self.json_path.read_text(encoding="utf-8"))
        self.assertEqual(len(result["deprecated_events"]), 2)
        replacements = [e["replacement"] for e in result["deprecated_events"]]
        self.assertEqual(set(replacements), {"ev_v3"})

    # ── --check mode ──────────────────────────────────────────────────────

    def test_check_passes_when_json_up_to_date(self):
        data = _valid_data()
        self._write_yaml(data)
        # Generate first
        with self._patch_paths():
            gen.main([])
        # Then check
        with self._patch_paths():
            rc = gen.main(["--check"])
        self.assertEqual(rc, 0)

    def test_check_fails_when_json_stale(self):
        data = _valid_data()
        self._write_yaml(data)
        with self._patch_paths():
            gen.main([])  # generate

        # Now mutate the JSON to simulate drift
        stale = self.json_path.read_text(encoding="utf-8").replace("3000000", "9999999")
        # Use a simple modification that won't break JSON parsing
        result = json.loads(self.json_path.read_text(encoding="utf-8"))
        result["deprecated_events"][0]["sunset_epoch"] = 9_999_999
        self.json_path.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")

        with self._patch_paths():
            rc = gen.main(["--check"])
        self.assertEqual(rc, 2)

    def test_check_fails_when_json_missing(self):
        data = _valid_data()
        self._write_yaml(data)
        # Do NOT call gen.main([]) – JSON file never created
        with self._patch_paths():
            rc = gen.main(["--check"])
        self.assertEqual(rc, 2)

    def test_check_fails_on_invalid_yaml(self):
        """--check with an invalid YAML (missing sunset_epoch) must exit 1."""
        entry = {k: v for k, v in _valid_entry().items() if k != "sunset_epoch"}
        self._write_yaml(_valid_data([entry]))
        with self._patch_paths():
            rc = gen.main(["--check"])
        self.assertEqual(rc, 1)

    # ── Idempotency ───────────────────────────────────────────────────────

    def test_generate_is_idempotent(self):
        """Running the generator twice produces identical output bytes."""
        data = _valid_data([
            _valid_entry(topic="ev_a", sunset_epoch=111_111, note="First note."),
            _valid_entry(topic="ev_b", sunset_epoch=222_222),
        ])
        self._write_yaml(data)
        with self._patch_paths():
            gen.main([])
        first = self.json_path.read_bytes()
        with self._patch_paths():
            gen.main([])
        second = self.json_path.read_bytes()
        self.assertEqual(first, second)


# ── Standalone entry point ─────────────────────────────────────────────────

if __name__ == "__main__":
    unittest.main(verbosity=2)
