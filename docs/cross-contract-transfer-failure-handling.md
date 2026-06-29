# Cross-Contract Transfer Failure Handling [RC26Q2-C13]

## Overview

This document describes the atomicity guarantees for token transfer failures
in `deposit_revenue` and `deposit_revenue_with_snapshot`.

## Atomicity Invariant

`do_deposit_revenue` performs all validation and the token transfer **before**
writing any state. The execution order is:

```
1. validate amount / period_id          ← pure, no writes
2. check OfferingNotFound               ← pure read
3. check PeriodAlreadyDeposited         ← pure read
4. check SupplyCap                      ← pure read
5. check PaymentTokenMismatch           ← pure read
6. try_transfer(issuer → contract)
   └─ FAIL → return Err(TransferFailed) ← NO writes have occurred
7. storage.set(PeriodRevenue)           ← only reached on success
8. storage.set(PeriodDepositTime)
9. storage.set(PeriodCount + 1)
10. storage.set(DepositedRevenue)
11. emit rev_dep2 event
```

If step 6 fails, steps 7–11 are never executed. Storage is unchanged.

For `deposit_revenue_with_snapshot`, the same invariant holds:
`do_deposit_revenue` is called internally, and `LastSnapshotRef` is only
advanced **after** `do_deposit_revenue` returns `Ok`.

## Security Assumptions

- The ordering of `try_transfer` **before** any storage write is the critical
  invariant. Any refactor moving a storage write above the transfer call would
  break atomicity and allow a period to be credited without tokens being deposited.
- `try_transfer` (not `.transfer()`) is used so the contract catches the failure
  and returns `RevoraError::TransferFailed` instead of panicking.
- `TransferFailed` is error code **30** in `RevoraError`.
- Neither `rev_dep2` nor `rev_snp2` events are emitted on failure.

## Error Code

| Error | Code | Condition |
|---|---|---|
| `TransferFailed` | 30 | `try_transfer` returns an error (zero balance, frozen token, etc.) |

## Negative Test Matrix

| Test | Scenario | State Mutation |
|---|---|---|
| `deposit_revenue_transfer_fail_returns_transfer_failed_error` | Zero balance | None |
| `deposit_revenue_transfer_fail_does_not_write_period_revenue` | Zero balance | None |
| `deposit_revenue_transfer_fail_does_not_increment_period_count` | Zero balance | None |
| `deposit_revenue_transfer_fail_does_not_update_deposited_revenue` | Zero balance | None |
| `deposit_revenue_transfer_fail_contract_balance_unchanged` | Zero balance | None |
| `deposit_revenue_insufficient_balance_returns_transfer_failed` | Partial balance | None |
| `deposit_revenue_insufficient_balance_issuer_balance_unchanged` | Partial balance | None |
| `successful_deposit_then_failed_deposit_preserves_first_period_only` | Mixed | Period 1 only |
| `snapshot_deposit_transfer_fail_returns_transfer_failed` | Zero balance | None |
| `snapshot_deposit_transfer_fail_does_not_advance_last_snapshot_ref` | Zero balance | None |
| `snapshot_deposit_transfer_fail_does_not_write_period_revenue` | Zero balance | None |
| `snapshot_deposit_transfer_fail_contract_balance_unchanged` | Zero balance | None |
| `deposit_with_supply_cap_transfer_fail_does_not_update_deposited_revenue_counter` | Cap set, zero balance | None |
| `transfer_fail_in_one_offering_does_not_affect_sibling_offering` | Multi-offering | Sibling intact |
| `failed_deposit_does_not_advance_period_ordering_cursor` | Retry after fail | Cursor intact |

## Implementation Status

All branches are implemented and covered by tests in
`src/test_cross_contract_transfer_fail.rs`.
