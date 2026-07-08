# Dividend Accrual Ledger

This document describes the per-offering dividend accrual ledger added for issue `#449`.

## What Changed

- `deposit_revenue` now updates a cumulative per-offering accrual index:
  - `GlobalAccPerShareE18(offering_id)`
  - `AccPerShareAtIndex(offering_id, period_index)`
- Holder share changes are frozen with per-holder checkpoints:
  - `HolderShareSchedule(offering_id, holder)`
- Claims no longer use the holder's current share for all unclaimed periods.
  Instead, each unclaimed deposited period is priced against the share checkpoint
  that was active when that period accrued.
- `acc_upd` is emitted on every successful `deposit_revenue` so indexers can
  reconcile the on-chain cumulative index.

## Important Repo-Specific Note

The issue description referenced `report_revenue`, but this contract's actual
claim funding path is `deposit_revenue`. `report_revenue` is informational and
audit-oriented here; it does not create holder claimable balances.

For that reason, the accrual index is updated on `deposit_revenue`.

## Security Properties

- Share changes are forward-only:
  - Updating a holder from `50%` to `25%` affects future deposits only.
  - Already-deposited periods retain the historical share that was active when
    the revenue accrued.
- Zeroing a holder does not burn already-accrued entitlement:
  - If revenue was deposited while the holder had a non-zero share, a later
    `set_holder_share(..., 0)` does not erase that historical claim.
- Claim delay remains authoritative:
  - The per-offering `ClaimDelaySecs` barrier is still enforced period-by-period.
  - A share change before the delay elapses does not rewrite the older period's
    eventual payout.
- Jurisdiction gating remains non-retroactive:
  - The new accrual path does not change the `#451` rule that removing a
    jurisdiction should not block already-persisted holder claims.

## Indexer Notes

`acc_upd` carries:

- `period_id`
- `period_index`
- `delta_e18`
- `global_acc_e18`

Indexers can reconstruct the cumulative dividend index directly from these
events and pair it with holder share checkpoint history for off-chain reviews.

## Test Coverage Added

- historical share preserved across unclaimed deposits
- zeroing a holder after deposit does not erase accrued value
- `get_claimable` matches the historical share schedule
- claim delay continues to compose correctly with share changes
