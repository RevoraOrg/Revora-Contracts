# Negative Amount Validation Matrix

**Feature ID:** #163  
**Contract:** RevoraRevenueShare  
**Status:** Implemented

## Overview

The Negative Amount Validation Matrix provides a centralized, deterministic framework for validating `i128` amount values across all contract operations. It ensures consistent handling of edge cases (zero, positive, negative, boundary values) and emits structured events on validation failures for monitoring and debugging.

## Security Assumptions

1. **No Trust Without Validation:** All amount parameters from external callers are treated as untrusted and must pass validation before use.
2. **Fail-Safe Defaults:** Invalid amounts always fail in the direction that prevents potential fund loss (e.g., deposits require positive amounts).
3. **Overflow Protection:** All arithmetic uses checked operations; the validation matrix uses saturating comparisons where overflow is possible.
4. **Auditability:** Validation failures emit structured events (`amt_valid`) with amount, error code, and reason for off-chain monitoring.

## Validation Categories

| Category | Requirement | Valid Values | Invalid Values | Error |
|----------|-------------|--------------|----------------|-------|
| `RevenueDeposit` | `> 0` | `1`, `1000`, `i128::MAX` | `0`, `-1`, `i128::MIN` | `InvalidAmount` |
| `RevenueReport` | `>= 0` | `0`, `1`, `1000` | `-1`, `i128::MIN` | `InvalidAmount` |
| `HolderShare` | `>= 0` | `0`, `500`, `10000` | `-1`, `i128::MIN` | `InvalidAmount` |
| `MinRevenueThreshold` | `>= 0` | `0`, `100`, `1000` | `-1`, `i128::MIN` | `InvalidAmount` |
| `SupplyCap` | `>= 0` | `0`, `1_000_000` | `-1`, `i128::MIN` | `InvalidAmount` |
| `InvestmentMinStake` | `>= 0` | `0`, `100`, `1000` | `-1`, `i128::MIN` | `InvalidAmount` |
| `InvestmentMaxStake` | `>= 0` | `0`, `10000` | `-1`, `i128::MIN` | `InvalidAmount` |
| `SnapshotReference` | `> 0` | `1`, `100`, `9999` | `0`, `-1`, `i128::MIN` | `InvalidAmount` |
| `PeriodId` | `>= 0` | `0`, `1`, `100` | `-1`, `i128::MIN` | `InvalidPeriodId` |
| `Simulation` | Any i128 | `-1`, `0`, `1`, `i128::MIN`, `i128::MAX` | (none) | (none) |

## Entry Points Using the Matrix

| Entry Point | Parameter(s) | Category |
|-------------|--------------|----------|
| `register_offering` | `supply_cap` | `SupplyCap` |
| `deposit_revenue` | `amount` | `RevenueDeposit` |
| `deposit_revenue_with_snapshot` | `amount` | `RevenueDeposit` |
| `deposit_revenue_with_snapshot` | `snapshot_reference` | `SnapshotReference` |
| `report_revenue` | `amount` | `RevenueReport` |
| `set_investment_constraints` | `min_stake` | `InvestmentMinStake` |
| `set_investment_constraints` | `max_stake` | `InvestmentMaxStake` |
| `set_min_revenue_threshold` | `min_amount` | `MinRevenueThreshold` |

## Additional Validation Rules

### Stake Range Validation
`set_investment_constraints` also validates that `min_stake <= max_stake` when `max_stake > 0`.

```rust
validate_stake_range(min_stake, max_stake) -> Result<(), RevoraError>
```

**Rule:** `max_stake > 0 && min_stake > max_stake` → `Err(InvalidAmount)`

### Snapshot Monotonicity
`deposit_revenue_with_snapshot` validates that new snapshot references are strictly increasing.

```rust
validate_snapshot_monotonic(new_ref, last_ref) -> Result<(), RevoraError>
```

**Rule:** `new_ref <= last_ref` → `Err(OutdatedSnapshot)`

## Event Emissions

On validation failure, the contract emits an `amt_valid` event:

```
topic: (EVENT_AMOUNT_VALIDATION_FAILED, issuer, namespace, token)
data: (amount, error_code, reason_symbol)
```

**Reason Symbols:**
- `must_be_pos`: Amount must be strictly positive
- `no_neg_rept`: Negative revenue report not allowed
- `no_neg_share`: Negative holder share not allowed
- `no_neg_thr`: Negative threshold not allowed
- `no_neg_cap`: Negative supply cap not allowed
- `no_neg_min`: Negative minimum stake not allowed
- `no_neg_max`: Negative maximum stake not allowed
- `snap_must_pos`: Snapshot reference must be positive
- `no_neg_per`: Negative period ID not allowed

## API

### Core Validation

```rust
AmountValidationMatrix::validate(amount, category) -> Result<(), (RevoraError, Symbol)>
```

### Stake Range

```rust
AmountValidationMatrix::validate_stake_range(min, max) -> Result<(), RevoraError>
```

### Snapshot Monotonicity

```rust
AmountValidationMatrix::validate_snapshot_monotonic(new_ref, last_ref) -> Result<(), RevoraError>
```

### Detailed Result

```rust
AmountValidationMatrix::validate_detailed(amount, category) -> AmountValidationResult
```

### Batch Validation

```rust
AmountValidationMatrix::validate_batch(amounts, category) -> Option<usize>
// Returns index of first failure, or None if all pass
```

### Function Mapping (for debugging)

```rust
AmountValidationMatrix::category_for_function("deposit_revenue") -> Some(AmountValidationCategory::RevenueDeposit)
```

## Test Coverage

The implementation includes comprehensive tests covering:

- [x] All categories accept valid boundary values (0, 1, i128::MAX)
- [x] All categories reject invalid boundary values (-1, i128::MIN) as appropriate
- [x] Stake range validation (min <= max)
- [x] Snapshot monotonicity (strictly increasing)
- [x] Batch validation (first/middle/last failures)
- [x] Detailed validation result structure
- [x] Function name mapping
- [x] Integration tests with actual contract entry points
- [x] Event emission on validation failure

**Test Module:** `src/test.rs` → `mod negative_amount_validation_matrix`

## Abuse/Failure Paths

| Attack Vector | Mitigation |
|--------------|------------|
| Negative deposit amounts | `RevenueDeposit` requires `> 0`; tokens cannot be created from nothing |
| Negative revenue reports | `RevenueReport` requires `>= 0`; prevents fund extraction via negative adjustments |
| Negative supply caps | `SupplyCap` requires `>= 0`; prevents invalid cap configurations |
| Snapshot regression | `SnapshotReference` requires `> 0` and strict monotonicity |
| Invalid stake ranges | Range validation ensures `min <= max` |
| Integer overflow in comparisons | Uses saturating arithmetic where needed |

## Constants

```rust
const EVENT_AMOUNT_VALIDATION_FAILED: Symbol = symbol_short!("amt_valid");
```

## Migration Notes

This feature is additive and backward-compatible. Existing validations are now centralized through the matrix but produce identical error codes for the same failure modes.

## References

- Issue #163: Implement Negative Amount Validation Matrix
- Issue #35: Input validation for amounts and period IDs
- Soroban SDK: `i128` signed integer type
