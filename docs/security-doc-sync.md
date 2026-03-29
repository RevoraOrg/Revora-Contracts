# Security Doc Sync

Issue: #194

## Summary

This change adds a deterministic on-chain payload to keep security documentation synchronized with contract reality.

Implemented in:
- src/lib.rs
- src/test_security_doc_sync.rs

## New API

`get_security_doc_sync() -> Map<Symbol, u32>`

Returned keys:
- `ver`: contract version
- `ev_sch`: versioned revenue-event schema version
- `idx_sch`: indexed event topic schema version
- `err_xfer`: transfer failure error code
- `err_auth`: authorization error code
- `err_sig`: signature replay error code

## Why

Security docs often drift from implementation details (error codes, schema versions, and guarantees). This API provides a machine-readable source of truth that docs tooling can validate in CI.

## Security Notes

- Read-only method; no state mutation.
- Deterministic output for consistent doc checks.
- Enables explicit detection of silent breaking changes in event/error schema.

## Tests

Added deterministic tests:
- `security_doc_sync_returns_expected_markers`
- `security_doc_sync_is_deterministic`

These verify key presence, expected values, and stable payload shape.
