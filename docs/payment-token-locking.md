# Payment Token Locking

## Summary

`PaymentToken` is now locked only by the first **successful** `deposit_revenue`
or `deposit_revenue_with_snapshot` call for an offering.

This hardening restores the storage model documented in the contract:

- `PaymentToken(OfferingId)` records the canonical payout token only after success
- `PeriodRevenue(OfferingId, period_id)` records one successful deposit per period
- `PeriodEntry(OfferingId, index)` enumerates only successfully deposited periods

There is no fallback from `PaymentToken` to `Offering.payout_asset` during
deposit processing or `get_payment_token` reads.

## Behavior

- `get_payment_token` returns `None` if the offering is unknown or if the offering exists but has not yet recorded a successful deposit.
- The first successful deposit writes `PaymentToken = payment_token`.
- Subsequent deposits must use that exact token or fail with
  `RevoraError::PaymentTokenMismatch`.
- Duplicate period deposits fail with `RevoraError::PeriodAlreadyDeposited`.
- Failed deposits do not write `PaymentToken`, `PeriodRevenue`, `PeriodEntry`,
  `PeriodCount`, or `LastPeriodId`.

## Security Assumptions

1. Payment-token locking is success-based:
- A token is canonical only after a transfer-backed deposit succeeds.
- Validation failures or transfer failures must not partially initialize lock state.

2. Asset identity is explicit:
- The contract never silently coerces `payment_token` from `payout_asset`.
- Integrators must pass the intended payout token on the first successful deposit.

3. Retry safety is required:
- A failed first deposit for `period_id = N` must leave `N` reusable for a correct retry.
- This prevents accidental `InvalidPeriodId` or `PaymentTokenMismatch` outcomes on
  correct follow-up sequencing.

4. Duplicate periods fail closed:
- Once `PeriodRevenue(offering, period_id)` exists, the same `period_id` is always
  rejected as already deposited.

## Test Coverage

Covered in `src/test.rs`:

- `register_offering_does_not_lock_payment_token_before_first_deposit`
- `failed_first_deposit_does_not_lock_payment_token_or_consume_period`
- `first_deposit_uses_registered_payment_token_lock`
- `second_deposit_rejects_wrong_payment_token_without_mutating_state`
- `deposit_revenue_fails_for_duplicate_period`
- zero-amount and invalid-period rejection without lock mutation

## Review Notes

- `PaymentTokenMismatch` is now reserved for a real post-lock mismatch.
- Correct integrator sequencing cannot trigger that error accidentally before any
  successful deposit has established the lock.
