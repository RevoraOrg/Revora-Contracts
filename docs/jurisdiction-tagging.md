# Jurisdiction Tagging and Compliance Gating

Issue: #451

## Summary

This change adds issuer-controlled jurisdiction metadata to holder records and a per-offering jurisdiction allowlist that gates new share writes and snapshot inclusion.

Implemented in:
- `src/lib.rs`
- `src/test_jurisdiction.rs`
- `src/structured_error_tests.rs`

## New API

- `set_holder_jurisdiction(issuer, namespace, token, holder, jurisdiction)`
- `get_holder_jurisdiction(issuer, namespace, token, holder) -> Option<Symbol>`
- `set_allowed_jurisdictions(issuer, namespace, token, jurisdictions)`
- `get_allowed_jurisdictions(issuer, namespace, token) -> Vec<Symbol>`

## Enforcement Boundary

- `set_holder_share` rejects with `JurisdictionDisallowed` when the offering allowlist is non-empty and the holder's stored jurisdiction is missing or not allowed.
- `meta_set_holder_share` inherits the same guard because it routes through the shared internal share writer.
- `apply_snapshot_shares` rejects the entire batch before any writes when any holder in the batch is disallowed.
- `claim` does not re-check the allowlist. This is intentional so that tightening or removing jurisdictions does not retroactively block already-persisted holder records.

## Events

- `jur_set`: emitted when the issuer updates a holder jurisdiction or replaces the offering allowlist.
- `jur_reject`: emitted when a share write or snapshot batch is rejected for jurisdiction mismatch.

## Security Notes

- Holder jurisdictions and allowlists are mutable only by the current issuer for the offering.
- The allowlist is checked before share state or snapshot slots are written, preserving atomicity on rejection.
- Empty allowlist means jurisdiction gating is disabled for future writes.
- Issuer transfer migrates the offering-level allowlist to the new issuer-scoped offering record.

## Tests

- Holder tagging and allowlist persistence with audit event coverage.
- Direct `set_holder_share` rejection path with `JurisdictionDisallowed`.
- Snapshot batch rejection without partial state writes.
- Non-retroactive behavior: previously recorded holders remain claimable after the issuer removes their jurisdiction from the allowlist.
- Structured error discriminant coverage for the new error code.
