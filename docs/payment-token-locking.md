# Payment Token Locking

## Summary

Each offering's payout asset is locked at registration time via the `payout_asset` parameter of
`register_offering`. Once locked, every `deposit_revenue` and `deposit_revenue_with_snapshot` call
must use that same token. Attempts to deposit with a different token are rejected with
`RevoraError::PaymentTokenMismatch`.

## Behavior

- The canonical payout token is set when `register_offering` is called.
- `get_payment_token(issuer, namespace, token)` returns `Some(address)` immediately after
  registration, before any deposit has occurred.
- On the first successful deposit the lock entry is persisted to storage (backfill).
- All subsequent deposits must use the locked token; mismatches are rejected atomically — no
  period state is written on failure.
- Claims resolve the payment token via the same canonical lock path.

## Security Assumptions

1. **Single canonical payout asset per offering.** Revenue deposits and claims use one token only.
   This prevents asset-mixing across periods for the same offering.

2. **Offering configuration is the trust boundary.** The issuer chooses `payout_asset` during
   `register_offering`. After registration the contract treats that asset as immutable
   payment-token policy.

3. **Fail closed on mismatch.** A deposit using any other token returns
   `RevoraError::PaymentTokenMismatch`. No period state is written when this happens.

4. **Lock is per-offering.** Two offerings registered by the same issuer may use different payout
   assets; each lock is independent.

## Interface

### `get_payment_token(issuer, namespace, token) -> Option<Address>`

Returns:
- `Some(address)` — the locked payout token for a known offering.
- `None` — the offering does not exist.

### `deposit_revenue(issuer, namespace, token, payment_token, amount, period_id)`

Fails with `PaymentTokenMismatch` if `payment_token` differs from the locked token.

## Test Coverage

| Test | What it verifies |
|------|-----------------|
| `register_offering_locks_payment_token_before_first_deposit` | Lock visible immediately after registration |
| `get_payment_token_returns_none_for_unknown_offering` | Unknown offering returns `None` |
| `deposit_revenue_preserves_locked_payment_token_across_deposits` | Lock stable across multiple deposits |
| `report_revenue_rejects_mismatched_payout_asset` | Mismatch rejected at report time |
| `first_deposit_uses_registered_payment_token_lock` | First deposit uses configured asset |
| `snapshot_deposit_preserves_registered_payment_token_lock` | Snapshot deposit respects lock |
| `deposit_revenue_rejects_mismatched_token_after_lock` | Different token rejected after lock-in |
| `deposit_revenue_rejects_wrong_token_on_first_deposit` | Wrong token rejected on first deposit |
| `payment_token_lock_is_stable_across_multiple_deposits` | Lock address unchanged after N deposits |
| `payment_token_lock_is_per_offering` | Two offerings lock independently |

## Review Scope

Changes are limited to:
- `src/lib.rs`
- `src/test.rs`
- this document
