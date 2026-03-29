# Vesting Event Schema Versioning

Issue: #174

## Overview

This change introduces explicit schema versioning for vesting events while preserving legacy event emission for backward compatibility.

Affected module:
- src/vesting.rs

## What Changed

- Added versioned vesting event symbols:
  - `vst_crt1` for schedule creation
  - `vst_clm1` for claims
  - `vst_can1` for cancellations
- Added `VESTING_EVENT_SCHEMA_VERSION` constant (currently `1`).
- Added `get_event_schema_version()` public method.
- Continued emitting legacy events (`vest_crt`, `vest_clm`, `vest_can`) unchanged.
- Emitted new v1 events in parallel, with `version` as the first data field.

## Security and Compatibility Notes

- Indexers depending on legacy events remain unaffected.
- New indexers should consume v1 events and validate the leading schema version field.
- Versioned payloads make future schema migration explicit and deterministic.

## Event Payloads

### Legacy (unchanged)
- `vest_crt`: `(token, total_amount, start_time, cliff_time, end_time, schedule_index)`
- `vest_clm`: `(schedule_index, token, claimable)`
- `vest_can`: `(schedule_index, token)`

### Versioned v1
- `vst_crt1`: `(version, token, total_amount, start_time, cliff_time, end_time, schedule_index)`
- `vst_clm1`: `(version, schedule_index, token, claimable)`
- `vst_can1`: `(version, schedule_index, token)`

## Tests

Added deterministic tests in `src/vesting_test.rs`:
- `event_schema_version_is_stable`
- `create_schedule_emits_legacy_and_v1_events`

These verify schema stability and dual emission behavior.
