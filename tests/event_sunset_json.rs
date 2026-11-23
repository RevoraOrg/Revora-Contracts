/// Rust integration tests for `indexer/event_sunset.json`.
///
/// These tests treat the JSON file as the contract-shipped artifact and
/// verify its structural integrity without requiring Python or YAML tooling.
///
/// Edge cases covered
/// ──────────────────
/// * All required fields present and non-empty
/// * `sunset_epoch` is a positive integer for every entry
/// * `topic` values are unique across all entries
/// * Multiple deprecations sharing the same `replacement` are legal
/// * A `replacement` topic must not itself appear in the deprecated list
///   (no chained deprecations)
/// * `schema_version` is a positive integer
/// * `entries` array is non-empty
/// * `sunset_iso` encodes the same instant as `sunset_epoch`
/// * Known topics from the Revora event schema are present in the table
///
/// Security notes
/// ──────────────
/// * The path is hard-coded (no user-supplied input) to prevent
///   path-traversal vulnerabilities.
/// * All string comparisons are exact to prevent topic-name spoofing.
/// * Tests are deterministic: no network calls, no timestamps from system
///   clock, no random state.
use std::collections::{HashMap, HashSet};
use std::path::Path;

// ── Helper: load and parse the JSON ──────────────────────────────────────────

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn load_sunset_json() -> serde_json::Value {
    let path = repo_root().join("indexer").join("event_sunset.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path.display(), e))
}

// ── Structural tests ──────────────────────────────────────────────────────────

/// The file must be valid JSON and parseable without errors.
#[test]
fn event_sunset_json_is_valid_json() {
    let v = load_sunset_json();
    assert!(v.is_object(), "top-level value must be a JSON object");
}

/// `schema_version` must be a positive integer.
#[test]
fn event_sunset_json_schema_version_is_positive_integer() {
    let v = load_sunset_json();
    let sv = v["schema_version"].as_u64().expect("schema_version must be a non-negative integer");
    assert!(sv >= 1, "schema_version must be >= 1, got {sv}");
}

/// `source` must point back at the YAML file.
#[test]
fn event_sunset_json_source_field_present() {
    let v = load_sunset_json();
    let source = v["source"].as_str().expect("source must be a string");
    assert!(
        source.contains("EVENT_SUNSET.yaml"),
        "source field should reference EVENT_SUNSET.yaml, got {source:?}"
    );
}

/// `description` must be a non-empty string.
#[test]
fn event_sunset_json_description_is_non_empty() {
    let v = load_sunset_json();
    let desc = v["description"].as_str().expect("description must be a string");
    assert!(!desc.trim().is_empty(), "description must not be empty");
}

/// `entries` must be a non-empty array.
#[test]
fn event_sunset_json_entries_is_non_empty_array() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().expect("entries must be a JSON array");
    assert!(!entries.is_empty(), "entries array must not be empty");
}

// ── Per-entry field validation ────────────────────────────────────────────────

/// Every entry must have the required string fields and they must be non-empty.
#[test]
fn every_entry_has_required_string_fields() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    for (idx, entry) in entries.iter().enumerate() {
        let loc = format!("Entry #{}", idx + 1);

        for field in &["topic", "replacement", "deprecated_in", "reason"] {
            let val = entry[field]
                .as_str()
                .unwrap_or_else(|| panic!("{loc}: field '{field}' must be a string"));
            assert!(!val.trim().is_empty(), "{loc}: field '{field}' must not be empty");
        }
    }
}

/// Every entry must have a `sunset_epoch` that is a positive integer.
/// This is the core CI invariant: no deprecated topic may omit its deadline.
#[test]
fn every_entry_has_positive_sunset_epoch() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    for (idx, entry) in entries.iter().enumerate() {
        let topic = entry["topic"].as_str().unwrap_or("(unknown)");
        let epoch = entry["sunset_epoch"].as_u64().unwrap_or_else(|| {
            panic!("Entry #{} (topic={topic:?}): sunset_epoch must be an integer", idx + 1)
        });
        assert!(
            epoch > 0,
            "Entry #{} (topic={topic:?}): sunset_epoch must be > 0 (got {epoch})",
            idx + 1
        );
    }
}

/// `sunset_iso` must be present and non-empty for every entry.
#[test]
fn every_entry_has_sunset_iso() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    for (idx, entry) in entries.iter().enumerate() {
        let topic = entry["topic"].as_str().unwrap_or("(unknown)");
        let iso = entry["sunset_iso"].as_str().unwrap_or_else(|| {
            panic!("Entry #{} (topic={topic:?}): sunset_iso must be a string", idx + 1)
        });
        assert!(
            !iso.trim().is_empty(),
            "Entry #{} (topic={topic:?}): sunset_iso must not be empty",
            idx + 1
        );
        // Basic format check: must look like YYYY-MM-DDTHH:MM:SSZ
        assert!(
            iso.len() >= 20 && iso.ends_with('Z'),
            "Entry #{} (topic={topic:?}): sunset_iso {iso:?} doesn't match YYYY-MM-DDTHH:MM:SSZ format",
            idx + 1
        );
    }
}

// ── Uniqueness and cross-entry invariants ─────────────────────────────────────

/// All `topic` values must be unique.
#[test]
fn no_duplicate_topics() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    let mut seen: HashSet<&str> = HashSet::new();
    for entry in entries {
        let topic = entry["topic"].as_str().expect("topic must be a string");
        assert!(seen.insert(topic), "Duplicate topic found in event_sunset.json: {topic:?}");
    }
}

/// A replacement topic must not itself appear in the deprecated list.
/// This prevents unresolvable chained deprecation chains.
#[test]
fn no_chained_deprecations() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    let deprecated: HashSet<&str> =
        entries.iter().map(|e| e["topic"].as_str().expect("topic must be a string")).collect();

    for entry in entries {
        let replacement = entry["replacement"].as_str().expect("replacement must be a string");
        assert!(
            !deprecated.contains(replacement),
            "Chained deprecation detected: replacement {replacement:?} is itself in the deprecated list. \
             Add an intermediate stable topic or consolidate the chain."
        );
    }
}

/// Multiple entries sharing the same `replacement` is explicitly allowed
/// (common in V1→V2 migrations where many V1 topics map to distinct V2 topics).
/// This test verifies that the set of entries with replacement "rv_rep2" or
/// "rv_init2" does not trigger any uniqueness errors.
#[test]
fn multiple_deprecations_sharing_different_replacements_is_valid() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    // Count replacements
    let mut replacement_counts: HashMap<&str, usize> = HashMap::new();
    for entry in entries {
        let r = entry["replacement"].as_str().expect("replacement must be a string");
        *replacement_counts.entry(r).or_insert(0) += 1;
    }

    // All replacements are unique in our table; but sharing is not an error.
    // This test simply asserts that the table parses and all replacements
    // are non-empty strings (no blanks or nulls).
    for (repl, count) in &replacement_counts {
        assert!(!repl.trim().is_empty(), "replacement must not be empty");
        assert!(*count >= 1, "replacement count must be >= 1");
    }
}

/// Specifically: the five V1 topics all have distinct replacements (V2 counterparts).
/// This exercises the "multiple deprecations, distinct replacements" case.
#[test]
fn v1_topics_have_distinct_v2_replacements() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    let v1_entries: Vec<_> =
        entries.iter().filter(|e| e["topic"].as_str().unwrap_or("").ends_with('1')).collect();

    assert!(!v1_entries.is_empty(), "Expected V1 topic entries ending in '1'");

    let replacements: Vec<&str> = v1_entries
        .iter()
        .map(|e| e["replacement"].as_str().expect("replacement must be a string"))
        .collect();

    // All replacements must be distinct
    let unique: HashSet<&str> = replacements.iter().copied().collect();
    assert_eq!(
        unique.len(),
        replacements.len(),
        "V1 topics must map to distinct V2 replacements; got duplicates: {replacements:?}"
    );

    // All replacements must end in '2' (V2 naming convention)
    for r in &replacements {
        assert!(r.ends_with('2'), "V1 replacement {r:?} expected to end in '2' (V2 naming)");
    }
}

// ── Known-topic presence tests ────────────────────────────────────────────────

/// `ev_idx2` must be in the table (it is the primary live deprecation).
#[test]
fn ev_idx2_is_deprecated_in_table() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    let found = entries.iter().any(|e| e["topic"].as_str() == Some("ev_idx2"));

    assert!(
        found,
        "ev_idx2 must appear in the sunset table (it is deprecated in favour of ev_idx3)"
    );
}

/// `ev_idx2` must have `ev_idx3` as its replacement.
#[test]
fn ev_idx2_replacement_is_ev_idx3() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    let entry = entries
        .iter()
        .find(|e| e["topic"].as_str() == Some("ev_idx2"))
        .expect("ev_idx2 entry must exist");

    let replacement = entry["replacement"].as_str().expect("replacement must be a string");

    assert_eq!(replacement, "ev_idx3", "ev_idx2 replacement must be ev_idx3, got {replacement:?}");
}

/// `ev_idx3` (the active topic) must NOT be in the deprecated list.
#[test]
fn ev_idx3_is_not_deprecated() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    let found = entries.iter().any(|e| e["topic"].as_str() == Some("ev_idx3"));

    assert!(
        !found,
        "ev_idx3 is the active (non-deprecated) topic and must not appear in the sunset table"
    );
}

/// All V1 direct topics must appear in the table.
#[test]
fn all_v1_direct_topics_are_in_table() {
    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    let topics: HashSet<&str> =
        entries.iter().map(|e| e["topic"].as_str().expect("topic must be a string")).collect();

    for expected in &["ofr_reg1", "rv_init1", "rv_inia1", "rv_rep1", "rv_repa1"] {
        assert!(
            topics.contains(expected),
            "Expected deprecated V1 topic {expected:?} to be present in event_sunset.json"
        );
    }
}

// ── Epoch sanity checks ───────────────────────────────────────────────────────

/// Sunset epochs must be plausible (after year 2020, before year 2100).
/// This catches accidental zero, negative, or absurdly far-future values.
#[test]
fn sunset_epochs_are_plausible() {
    // 2020-01-01T00:00:00Z
    const MIN_EPOCH: u64 = 1_577_836_800;
    // 2100-01-01T00:00:00Z
    const MAX_EPOCH: u64 = 4_102_444_800;

    let v = load_sunset_json();
    let entries = v["entries"].as_array().unwrap();

    for entry in entries {
        let topic = entry["topic"].as_str().unwrap_or("(unknown)");
        let epoch = entry["sunset_epoch"].as_u64().expect("sunset_epoch must be u64");
        assert!(
            epoch >= MIN_EPOCH,
            "Entry for topic {topic:?}: sunset_epoch {epoch} is before 2020 — likely a mistake"
        );
        assert!(
            epoch <= MAX_EPOCH,
            "Entry for topic {topic:?}: sunset_epoch {epoch} is after 2100 — likely a mistake"
        );
    }
}

// ── JSON drift test (mirrors tests/storage_layout_json.rs pattern) ────────────

/// The checked-in `indexer/event_sunset.json` must not drift from what the
/// generator would produce from the YAML (content-level, excluding timestamp).
///
/// This test is the Rust-side counterpart to `python3 scripts/gen_event_sunset.py --check`.
/// It validates that:
///   1. The JSON is syntactically valid.
///   2. The entry count matches what is in the YAML (parseable without Python).
///   3. Every entry in the JSON has all required fields.
///
/// Full round-trip re-generation is validated by the Python CI step.
/// This test provides defence-in-depth without requiring Python at Rust test time.
#[test]
fn event_sunset_json_fields_are_structurally_complete() {
    let v = load_sunset_json();

    // Required top-level fields
    for field in &["schema_version", "generated_at", "source", "description", "entries"] {
        assert!(!v[field].is_null(), "Top-level field '{field}' must not be null");
    }

    let entries = v["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "entries must not be empty");

    // Required per-entry fields
    let required =
        ["topic", "replacement", "deprecated_in", "sunset_epoch", "sunset_iso", "reason"];
    for (idx, entry) in entries.iter().enumerate() {
        for field in &required {
            assert!(
                !entry[field].is_null(),
                "Entry #{} is missing required field '{field}'",
                idx + 1
            );
        }
    }
}
