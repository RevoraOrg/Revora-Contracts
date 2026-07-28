# Dispute Window Enforcement (#594)

## Summary

Implements a time-bound dispute window for IssuerDispute freezes. Issuers can only freeze holders with IssuerDispute reason within a configurable window after a period's close.

## Changes

### Core Implementation

1. New Error Variant: DisputeWindowClosed = 56 in RevoraError enum
2. New Storage Key: DisputeWindowSecs(OfferingId) in DataKey2 enum
3. New Constants: DEFAULT_DISPUTE_WINDOW_SECS (30 days), EVENT_DISPUTE_WINDOW_SET
4. New Functions: set_dispute_window(), get_dispute_window()
5. Modified Function: emergency_freeze_holder() - added dispute window check for IssuerDispute
6. Test Module: src/test_dispute_window.rs with comprehensive coverage

### Bug Fixes

- Fixed duplicate function declaration at line 7464 in src/lib.rs
- Fixed incomplete set_holder_share_full function declaration at line 7285
- Fixed duplicate [lints.rust] section in Cargo.toml

## Security

- Uses env.ledger().timestamp() (authoritative Soroban timestamp)
- Only current issuer or global admin can configure window
- Inclusive deadline: now <= closed_at + dispute_window_secs
- Zero window allows disputes only at exact close time
- No period closed = disputes always allowed

## Testing

Comprehensive test coverage in src/test_dispute_window.rs including:
- Default and custom window retrieval
- Authorization checks
- Enforcement before/at/after deadline
- Zero window behavior
- Other freeze reasons bypassing window
- Reconfiguration and most recent period enforcement

## Migration

No migration required. New feature with:
- Default 30-day window for all offerings
- Existing deployments continue working
- Issuers call set_dispute_window() to customize
