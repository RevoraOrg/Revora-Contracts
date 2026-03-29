# Period ID Boundary Tests

## Overview

This document describes the design, security assumptions, and test coverage for
Period ID boundary validation in the Revora revenue-share contract.

Period IDs are caller-supplied `u64` values that identify discrete revenue
reporting or deposit periods. Because `0` is a natural default/sentinel in
storage (unset keys return `0`), it is reserved and must never be accepted as a
valid period identifier.

---

## Valid Domain

| Field       | Type   | Valid range        | Notes                              |
|-------------|--------|--------------------|------------------------------------|
| `period_id` | `u64`  | `[1, u64::MAX]`    | `0` is reserved/invalid            |
| `amount`    | `i128` | `[0, i128::MAX]`   | `report_revenue` (zero allowed)    |
| `amount`    | `i128` | `[1, i128::MAX]`   | `deposit_revenue` (must be > 0)    |

---

## Security Assumptions

1. **period_id == 0 is always invalid.** Accepting `0` would be ambiguous with
   the storage default and could allow silent data corruption or double-counting.

2. **Validation fires before any state mutation.** Both `require_valid_period_id`
   and the amount guards (`require_non_negative_amount` / `require_positive_amount`)
   are called at the top of `report_revenue` and `deposit_revenue` respectively,
   before any storage read/write or event emission.

3. **Auth is required before validation.** `issuer.require_auth()` is called
   before input validation in `report_revenue`, so unauthenticated callers cannot
   probe validation behavior without valid credentials.

4. **No overflow on boundary values.** Cumulative revenue uses `saturating_add`,
   so `i128::MAX` inputs do not panic.

5. **Offering isolation.** Period ID state is keyed by `(OfferingId, period_id)`.
   Boundary operations on one offering cannot affect another.

---

## Implementation

### `lib.rs` — `require_valid_period_id`

```rust
/// Input validation (#35): require period_id > 0 where 0 would be ambiguous.
///
/// Period ID 0 is reserved and must not be used as a valid period identifier.
/// Valid period IDs are in the range [1, u64::MAX].
fn require_valid_period_id(period_id: u64) -> Result<(), RevoraError> {
    if period_id == 0 {
        return Err(RevoraError::InvalidPeriodId);
    }
    Ok(())
}
```

This function is called at the start of both `report_revenue` and
`deposit_revenue`, before any offering lookup or state mutation.

### Call sites

**`report_revenue`** (after auth, before offering lookup):
```rust
Self::require_valid_period_id(period_id)?;
Self::require_non_negative_amount(amount)?;
```

**`deposit_revenue`** (after freeze check, before offering lookup):
```rust
Self::require_valid_period_id(period_id)?;
Self::require_positive_amount(amount)?;
```

---

## Test Coverage (`src/test.rs` — `mod period_id_boundary`)

### `report_revenue` — period_id validation

| Test | Scenario | Expected |
|------|----------|----------|
| `report_revenue_rejects_zero_period_id` | `period_id == 0` | `Err` |
| `report_revenue_accepts_period_id_one` | `period_id == 1` (min valid) | `Ok` |
| `report_revenue_accepts_period_id_max` | `period_id == u64::MAX` | `Ok` |
| `report_revenue_accepts_period_id_near_max` | `period_id == u64::MAX - 1` | `Ok` |
| `report_revenue_zero_period_id_does_not_mutate_state` | `period_id == 0` rejected, no state change | revenue == 0, events == 1 |

### `report_revenue` — amount validation

| Test | Scenario | Expected |
|------|----------|----------|
| `report_revenue_rejects_negative_amount_at_valid_period` | `amount < 0` (all boundary negatives) | `Err` |
| `report_revenue_accepts_zero_amount_at_valid_period` | `amount == 0` | `Ok` |
| `report_revenue_accepts_max_amount_at_valid_period` | `amount == i128::MAX` | `Ok` |
| `report_revenue_rejects_zero_period_and_negative_amount` | both invalid | `Err` |

### `report_revenue` — override semantics at boundaries

| Test | Scenario | Expected |
|------|----------|----------|
| `report_revenue_override_at_min_period` | override at `period_id == 1` | `Ok` |
| `report_revenue_override_at_max_period` | override at `period_id == u64::MAX` | `Ok` |
| `report_revenue_duplicate_without_override_at_max_period` | duplicate, no override | rejection event emitted, no panic |

### Revenue query at boundary periods

| Test | Scenario | Expected |
|------|----------|----------|
| `get_revenue_by_period_returns_zero_for_unreported_period` | query at 0, 1, MAX-1, MAX | `0` (no panic) |
| `get_revenue_range_chunk_at_boundary_periods` | range `[1,1]` with one report | correct sum |
| `get_revenue_range_chunk_inverted_range_returns_zero` | `from > to` | `0`, no cursor |

### `deposit_revenue` — period_id and amount validation

| Test | Scenario | Expected |
|------|----------|----------|
| `deposit_revenue_rejects_zero_period_id_boundary` | `period_id == 0` | `Err` |
| `deposit_revenue_accepts_min_valid_period_id` | `period_id == 1` | `Ok` |
| `deposit_revenue_rejects_zero_amount_boundary` | `amount == 0` | `Err` |
| `deposit_revenue_rejects_negative_amount_boundary` | `amount == -1` | `Err` |
| `deposit_revenue_rejects_duplicate_period_at_boundary` | same period twice | `Err` |

### Isolation and auth

| Test | Scenario | Expected |
|------|----------|----------|
| `period_id_boundary_offering_isolation` | boundary period on offering A | offering B unaffected |
| `report_revenue_requires_auth_at_boundary_periods` | unauthenticated call | `Err` |

### Full boundary matrix sweep

| Test | Scenario | Expected |
|------|----------|----------|
| `period_id_boundary_matrix_no_panic` | all (valid/invalid) × (period/amount) combos | correct accept/reject, no panics |

---

## Fuzz Test Updates

The existing fuzz tests (`fuzz_period_and_amount_boundaries_do_not_panic` and
`fuzz_period_and_amount_repeatable_sweep_do_not_panic`) were updated to reflect
the enforced validation:

- Invalid inputs (`period_id == 0`, negative amounts) are now explicitly
  asserted to be rejected rather than silently counted.
- Conflicting/stale `assert_eq!(env.events().all().len(), ...)` assertions that
  assumed all inputs would be accepted were removed.
- The sweep test now asserts both `accepted > 0` and `rejected_invalid > 0` to
  confirm the validation boundary is exercised.

---

## Error Codes

| Error | Code | Meaning |
|-------|------|---------|
| `InvalidPeriodId` | 22 | `period_id == 0` supplied to `report_revenue` or `deposit_revenue` |
| `InvalidAmount` | 21 | Negative amount to `report_revenue`, or zero/negative to `deposit_revenue` |
