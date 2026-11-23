# Event Sunset Table

Machine-readable table of deprecated Revora contract event topics, their
replacements, and the Unix epoch after which each deprecated topic is guaranteed
to stop being emitted.

## Quick reference

| Deprecated topic | Replacement | Sunset epoch | Sunset date (UTC) |
|-----------------|-------------|:------------:|:-----------------:|
| `ev_idx2` | `ev_idx3` | 1785196800 | 2027-07-28 |
| `ofr_reg1` | `ofr_reg2` | 1767225599 | 2025-12-31 |
| `rv_init1` | `rv_init2` | 1767225599 | 2025-12-31 |
| `rv_inia1` | `rv_inia2` | 1767225599 | 2025-12-31 |
| `rv_rep1` | `rv_rep2` | 1767225599 | 2025-12-31 |
| `rv_repa1` | `rv_repa2` | 1767225599 | 2025-12-31 |

> The canonical, auto-parseable version of this table lives in
> [`indexer/event_sunset.json`](../indexer/event_sunset.json).
> The table above is derived from that file for human readability.
> **Do not edit the JSON directly**; edit
> [`docs/EVENT_SUNSET.yaml`](EVENT_SUNSET.yaml) and regenerate.

---

## Files and their roles

| File | Role |
|------|------|
| `docs/EVENT_SUNSET.yaml` | Single source of truth — edit this |
| `scripts/gen_event_sunset.py` | Generator: reads YAML → writes JSON |
| `indexer/event_sunset.json` | Generated artifact shipped with the WASM |
| `scripts/check_event_sunset.sh` | CI wrapper that calls the generator `--check` |
| `tests/event_sunset_json.rs` | Rust integration tests for the JSON artifact |

---

## Schema reference

### `docs/EVENT_SUNSET.yaml`

```yaml
schema_version: 1   # bump when the file structure itself changes

entries:
  - topic:         <string>   # deprecated Soroban event topic symbol (required)
    replacement:   <string>   # successor topic (required)
    deprecated_in: <string>   # semver tag of announcing release, e.g. "v0.26.0"
    sunset_epoch:  <integer>  # Unix timestamp after which the topic stops; CI
                              # FAILS if this is missing or zero
    reason:        <string>   # human-readable rationale (required)
    notes:         <string>   # optional extra guidance for indexer authors
```

**Invariants enforced by the generator:**

1. Every entry **must** have a non-null, non-zero `sunset_epoch`. CI fails otherwise.
2. `topic` must be unique across all entries.
3. `replacement` must **not** itself appear in the deprecated topic list (no
   chained deprecations). If you need to chain, add an intermediate stable topic.
4. `deprecated_in` must be present and non-empty.
5. `reason` must be present and non-empty.

### `indexer/event_sunset.json`

The JSON file mirrors the YAML but is enriched with:

- `generated_at` — RFC-3339 timestamp of the last regeneration
- `source` — relative path back to the YAML source
- `description` — one-line description for tooling that reads the JSON cold
- `sunset_iso` — ISO-8601 rendering of `sunset_epoch` for display purposes

```json
{
  "schema_version": 1,
  "generated_at": "2026-07-28T13:25:50Z",
  "source": "docs/EVENT_SUNSET.yaml",
  "description": "...",
  "entries": [
    {
      "topic": "ev_idx2",
      "replacement": "ev_idx3",
      "deprecated_in": "v0.26.0",
      "sunset_epoch": 1785196800,
      "sunset_iso": "2027-07-28T00:00:00Z",
      "reason": "...",
      "notes": "..."
    }
  ]
}
```

---

## How to add a new deprecation

1. **Edit `docs/EVENT_SUNSET.yaml`** — add a new entry under `entries:`.

   ```yaml
   - topic: my_old_topic
     replacement: my_new_topic
     deprecated_in: "v0.27.0"
     sunset_epoch: 1800000000   # pick a date ≥ 2 minor releases in the future
     reason: >
       Brief explanation of why the topic is deprecated.
     notes: >
       Optional migration guidance for indexers.
   ```

2. **Regenerate the JSON:**

   ```bash
   python3 scripts/gen_event_sunset.py
   ```

3. **Verify CI would pass:**

   ```bash
   python3 scripts/gen_event_sunset.py --check
   # or
   bash scripts/check_event_sunset.sh
   ```

4. **Commit both files** together:

   ```bash
   git add docs/EVENT_SUNSET.yaml indexer/event_sunset.json
   git commit -m "docs: deprecate my_old_topic → my_new_topic (sunset 2027-01-15)"
   ```

> Tip — pick a `sunset_epoch` that is at least **two contract minor versions**
> in the future to give downstream indexers sufficient migration time.

---

## How CI enforces the table

The `test` job in `.github/workflows/ci.yml` runs two checks after build:

1. **Python drift check** (`Check event sunset JSON drift` step):
   ```bash
   python3 scripts/gen_event_sunset.py --check
   ```
   Fails with exit code 2 if the checked-in JSON is out of sync with the YAML.
   Does not write any files.

2. **Rust integration test** (`Test event sunset JSON` step):
   ```bash
   cargo test --test event_sunset_json -- --test-threads=1
   ```
   Runs `tests/event_sunset_json.rs` which validates the JSON structure,
   required fields, uniqueness, sunset epochs, and edge cases.

Both checks must pass before a PR can merge.

---

## How indexers consume the table

Load `indexer/event_sunset.json` at startup and build a lookup from
`topic → SunsetEntry`:

```python
import json, time

with open("indexer/event_sunset.json") as f:
    table = json.load(f)

sunset_by_topic = {e["topic"]: e for e in table["entries"]}

def should_warn(topic: str) -> bool:
    entry = sunset_by_topic.get(topic)
    if entry is None:
        return False
    # Warn 30 days before sunset
    return time.time() > entry["sunset_epoch"] - 30 * 86400

def is_sunsetted(topic: str) -> bool:
    entry = sunset_by_topic.get(topic)
    if entry is None:
        return False
    return time.time() >= entry["sunset_epoch"]

# When processing an event:
if is_sunsetted(event.topic):
    raise ValueError(f"Deprecated topic {event.topic!r} is past its sunset date")
if should_warn(event.topic):
    entry = sunset_by_topic[event.topic]
    logger.warning(
        "Topic %r is deprecated; migrate to %r before epoch %d (%s)",
        event.topic, entry["replacement"],
        entry["sunset_epoch"], entry["sunset_iso"],
    )
```

---

## Security notes

- The YAML path is hard-coded in the generator (no user-supplied paths) to
  prevent path-traversal.
- All string fields are whitespace-normalised before writing to JSON.
- The JSON is `sort_keys=False` (field order matches entry order in YAML) but
  the generator output is deterministic for a given YAML, so diffs are clean.
- The `generated_at` timestamp is **excluded** from the drift comparison so
  mere re-generation without YAML changes does not trigger a CI failure.
- The generator uses only Python stdlib (no third-party dependencies) unless
  PyYAML is available, in which case it is preferred over the built-in fallback
  parser. The fallback handles the restricted EVENT_SUNSET.yaml dialect only.

---

## Multiple deprecations sharing a replacement

When several legacy topics all converge on a single replacement (common in
V1→V2 migrations), list them as separate entries. The generator enforces that
`replacement` is not itself in the deprecated list, catching invalid chains:

```yaml
entries:
  - topic: ofr_reg1
    replacement: ofr_reg2   # multiple V1 topics → distinct V2 topics
    ...
  - topic: rv_init1
    replacement: rv_init2
    ...
```

Two entries sharing the same `replacement` is explicitly allowed and tested.

---

## Related documentation

- [`docs/core-event-version-field.md`](core-event-version-field.md) — full
  dual-stream architecture and V2/V3 event versioning policy
- [`docs/STORAGE_LAYOUT.json`](STORAGE_LAYOUT.json) — parallel pattern for
  machine-readable storage key documentation
- [`tests/event_sunset_json.rs`](../tests/event_sunset_json.rs) — Rust tests
