# Payment Token Locking

## Summary

This change hardens payment token locking so the canonical payout token is explicit from offering configuration and queryable before deposit, while remaining backward-compatible with existing storage.

## Behavior

- Each offering's payment token is canonically locked to its configured `payout_asset`.
- `get_payment_token` exposes that lock immediately, even before the first deposit.
- If no explicit `PaymentToken` storage entry exists yet, the contract falls back to `payout_asset`.
- On the first successful deposit with the canonical token, the lock entry is persisted.
- `deposit_revenue` and `deposit_revenue_with_snapshot` must use the locked token.
- `get_payment_token` exposes the locked token for review and integrations.

## Security Assumptions

1. Single canonical payout asset per offering:
- Revenue deposits and claims must use one token only.
- This prevents asset-mixing across periods for the same offering.

2. Offering configuration is the trust boundary:
- The issuer chooses `payout_asset` during `register_offering`.
- After registration, the contract treats that asset as immutable payment-token policy.

3. Backward compatibility for older storage:
- If an older offering does not yet have an explicit `PaymentToken` entry, the contract treats `payout_asset` as the canonical lock.
- When a matching deposit occurs, the lock entry is backfilled.

4. Fail closed on mismatch:
- A deposit using any other token returns `RevoraError::PaymentTokenMismatch`.
- No period state is written when this happens.

## Interface

- `get_payment_token(issuer, namespace, token) -> Option<Address>`

Returns:
- `Some(address)` for known offerings
- `None` if the offering does not exist

## Developer Notes

- Claims now resolve payment token via the same canonical lock path instead of assuming a previously written storage key.
- This removes ambiguity for integrations and avoids missing-key edge cases.

## Test Coverage

Added deterministic coverage for:
- lock visibility immediately after registration
- lock preservation across successful deposits
- lock preservation across snapshot deposits
- unknown-offering lookup behavior
- missing-auth registration leaves no lock behind

## Review Scope

Changes are limited to:
- `src/lib.rs`
- `src/test.rs`
- `src/test_auth.rs`
- this document
