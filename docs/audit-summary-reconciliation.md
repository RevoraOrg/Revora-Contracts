# Audit Summary Reconciliation

## Overview

The Revora Revenue Share contract maintains a per-offering `AuditSummary` cache that tracks:

- `total_revenue` — cumulative revenue reported across all periods.
- `report_count` — number of distinct periods that have been initially reported.

This cache is a **derived view** of the authoritative `RevenueReports` map. The reconciliation capability lets operators verify the cache is accurate and repair it if drift is detected.

---

## Why Reconciliation Is Needed

The `AuditSummary` is updated incrementally on every `report_revenue` call. Three distinct outcomes are possible:

| Call type | Effect on `RevenueReports` | Expected `AuditSummary` delta |
|---|---|---|
| Initial report (new period) | Inserts `(amount, timestamp)` | `total_revenue += amount`, `report_count += 1` |
| Override (`override_existing=true`) | Replaces existing `(old, ts)` with `(new, ts)` | `total_revenue += (new - old)`, count unchanged |
| Rejected (`override_existing=false`, period exists) | No change | No change |

A bug in earlier versions of the contract applied the audit update **unconditionally and twice** — once before the event-only guard and once inside it — causing double-counting in normal (non-event-only) mode. Additionally, overrides incorrectly added the new amount on top of the old one instead of computing the net delta.

These bugs are fixed in this implementation. The reconciliation functions provide a safety net to detect and correct any residual drift.

---

## Security Assumptions

1. **`RevenueReports` is the source of truth.** The map stores the actual per-period amounts. `AuditSummary` is a cache derived from it. If they diverge, the map wins.

2. **`reconcile_audit_summary` is read-only.** It requires no authentication and never mutates state. Any caller (including off-chain indexers) may call it safely.

3. **`repair_audit_summary` requires issuer or admin auth.** This prevents arbitrary callers from triggering unnecessary storage writes. The function is idempotent — calling it when the summary is already correct is safe.

4. **Overflow is handled with saturation.** If `computed_total_revenue` would overflow `i128`, it saturates at `i128::MAX` and `is_saturated` is set to `true`. A saturated result always sets `is_consistent = false`.

5. **Frozen contract blocks repair.** `repair_audit_summary` respects the `ContractFrozen` guard. `reconcile_audit_summary` does not mutate state and is always callable.

---

## API Reference

### `get_audit_summary(issuer, namespace, token) → Option<AuditSummary>`

Returns the stored `AuditSummary` for an offering, or `None` if no reports have been filed.

```rust
pub struct AuditSummary {
    pub total_revenue: i128,
    pub report_count: u64,
}
```

### `reconcile_audit_summary(issuer, namespace, token) → AuditReconciliationResult`

Read-only. Compares the stored `AuditSummary` against the authoritative `RevenueReports` map.

```rust
pub struct AuditReconciliationResult {
    pub stored_total_revenue: i128,
    pub stored_report_count: u64,
    pub computed_total_revenue: i128,
    pub computed_report_count: u64,
    pub is_consistent: bool,
    pub is_saturated: bool,
}
```

- `is_consistent` is `true` iff both totals and counts match and no saturation occurred.
- `is_saturated` is `true` iff the computed total hit `i128::MAX` during summation.

**Auth:** None required.

### `repair_audit_summary(caller, issuer, namespace, token) → Result<AuditSummary, RevoraError>`

Rewrites the `AuditSummary` by recomputing it from the `RevenueReports` map.

**Auth:** `caller` must be the current issuer or the contract admin.

**Errors:**
- `ContractFrozen` — contract is frozen.
- `OfferingNotFound` — offering does not exist.
- `NotInitialized` — admin not set.
- `NotAuthorized` — caller is neither issuer nor admin.

**Events:** Emits `aud_rep` with `(total_revenue, report_count)` as data.

---

## Abuse and Failure Paths

| Scenario | Behavior |
|---|---|
| Stranger calls `repair_audit_summary` | Returns `NotAuthorized` |
| Repair called on non-existent offering | Returns `OfferingNotFound` |
| Repair called when frozen | Returns `ContractFrozen` |
| Reconcile called with no reports | Returns `{0, 0, 0, 0, true, false}` |
| Override with same amount | Delta = 0; summary unchanged |
| Rejected report | No mutation to summary |
| Repair called twice | Idempotent; second call produces same result |
| Overflow in computed total | `is_saturated=true`, `is_consistent=false` |

---

## Integration Pattern

```rust
// 1. Check if the summary is accurate.
let result = client.reconcile_audit_summary(&issuer, &ns, &token);

if !result.is_consistent {
    // 2. Log the discrepancy for off-chain audit trail.
    log!(
        "Drift detected: stored={}, computed={}",
        result.stored_total_revenue,
        result.computed_total_revenue
    );

    // 3. Repair (issuer or admin must sign).
    let corrected = client.repair_audit_summary(&issuer, &issuer, &ns, &token)?;
    assert_eq!(corrected.total_revenue, result.computed_total_revenue);
}
```

---

## Test Coverage

All tests are in `src/test.rs` under the `audit_reconciliation` module. Coverage includes:

- Correct summary after single and multiple initial reports
- Override: delta applied, count not incremented
- Override increasing, decreasing, and same-amount cases
- Multiple overrides converge correctly
- Rejected report leaves summary unchanged
- `reconcile_audit_summary` returns consistent on clean state
- `reconcile_audit_summary` returns consistent after override
- Empty offering returns zeroes and `is_consistent=true`
- `repair_audit_summary` corrects a drifted summary
- Auth boundaries: issuer allowed, admin allowed, stranger rejected
- Repair rejected for non-existent offering
- Repair blocked when frozen
- Repair is idempotent
- Multiple offerings are isolated
- `reconcile_audit_summary` requires no auth
- Full lifecycle (initial + override + rejected) is consistent
- Repair emits `aud_rep` event
- Zero-amount reports increment count correctly
- Repair on empty offering resets to zero
- Issuer transfer preserves reconcilability
