# Offering Duplicate Prevention

## Overview
This update introduces duplicate prevention logic in the contract's offering registration.

## Behavior
- If an offering with the same `(issuer, namespace, token)` already exists:
  - The contract does NOT insert a duplicate entry
  - The function returns success (idempotent behavior)

## Why idempotent?
To preserve backward compatibility with existing contract behavior and tests,
duplicate registrations are treated as no-op instead of errors.

## Security
Prevents:
- Duplicate storage entries
- State inconsistencies
- Replay-style abuse

## Implementation Details
`env.storage().persistent().has(&DataKey::OfferingIssuer(offering_id))`

This O(1) check ensures that any logical duplicate (same issuer, namespace, and token) is detected before any storage writes occur.

## Testing
- Existing test suite is preserved
- No regressions introduced