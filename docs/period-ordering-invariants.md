# Period Ordering Invariants

## Security Assumptions
- Periods are **strictly monotonic increasing** `u64` values **per offering**.
- No gaps, duplicates, or reorders (prevents sequencing attacks, front-running, replay).

## Enforcement
| Function          | Check Performed |
|-------------------|-----------------|
| `report_revenue` | `period_id > LastPeriodId(offering)` → `Err(InvalidPeriodId)`; then set `LastPeriodId` |
| `deposit_revenue` | Same double-check + `period_id > 0`; then set `LastPeriodId` |

## Storage Impact
- `DataKey::LastPeriodId(OfferingId)`: `u64` (~8 bytes + overhead per active offering).

## Gas Cost
- **+1 read/+1 write** per call (negligible vs. existing logic).

## Abuse Mitigations
- Rejects invalid sequencing (e.g., deposit period 1 → 0, duplicate 5, skip to 7).
- Ensures chronological processing order via sequential `PeriodEntry` indexing.
- Compatible with existing claims/views (index-based, unaffected).

## Validation Examples
```
✅ deposit(1) → deposit(2) → deposit(3)
❌ deposit(1) → deposit(1) (duplicate)
❌ deposit(1) → deposit(0) (non-increasing)
❌ deposit(2) → deposit(1) (non-increasing)
❌ deposit(1) → deposit(3) (gaps disallowed)
```

## Upgrade Safety
- New storage key; existing data unaffected.
- CONTRACT_VERSION bump recommended for migration checks.

