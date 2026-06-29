# Period Ordering Invariants

## Security Assumptions
- Deposit periods and report periods are tracked independently per offering.
- Each track is **strictly monotonic increasing** for new entries only.
- Explicit revenue-report overrides reuse an existing `period_id` and do not advance the report cursor.
- Below-threshold new reports are no-ops and do not consume a report `period_id`.

## Enforcement
| Function          | Check Performed |
|-------------------|-----------------|
| `report_revenue` | New periods require `period_id > LastReportedPeriodId(offering)`; successful insert commits the cursor |
| `report_revenue` override | Existing periods may be corrected with `override_existing=true`; cursor unchanged |
| `deposit_revenue` | New deposits require `period_id > LastDepositedPeriodId(offering)`; successful deposit commits the cursor |

## Storage Impact
- `DataKey::LastReportedPeriodId(OfferingId)`: `u64` (~8 bytes + overhead per active offering).
- `DataKey::LastDepositedPeriodId(OfferingId)`: `u64` (~8 bytes + overhead per active offering).

## Gas Cost
- **+1 read/+1 write** per call (negligible vs. existing logic).

## Abuse Mitigations
- Rejects invalid sequencing (e.g., deposit period 1 → 0, duplicate 5, skip to 7).
- Failed deposits do not consume the next valid `period_id`.
- Ensures chronological processing order via sequential `PeriodEntry` indexing.
- Compatible with existing claims/views (index-based, unaffected).

## Validation Examples
```
✅ deposit(1) → deposit(2) → deposit(3)
✅ report(1) → report(2) → override(2)
✅ report(1 below threshold) → report(1 after threshold disabled)
❌ deposit(1) → deposit(1) (duplicate)
❌ deposit(1) → deposit(0) (non-increasing)
❌ deposit(2) → deposit(1) (non-increasing)
✅ deposit(1) → deposit(3) (gaps are allowed; ordering is monotonic, not contiguous)
```

## Upgrade Safety
- New storage key; existing data unaffected.
- CONTRACT_VERSION bump recommended for migration checks.
