# Negative Amount Validation Matrix

**Feature ID:** #163  
**Contract:** `RevoraRevenueShare`  
**Status:** Updated for the current branch

## Scope

This checklist covers the current public Revora entrypoints that accept signed amount-like values and should reject invalid negative inputs with `RevoraError::InvalidAmount`.

It is intentionally Revora-specific:

- `register_offering` validates `supply_cap`
- `report_revenue` validates `amount`
- `deposit_revenue` validates `amount`
- `deposit_revenue_with_snapshot` validates both `amount` and `snapshot_reference`
- `set_investment_constraints` validates `min_stake`, `max_stake`, and the `min <= max` invariant
- `set_min_revenue_threshold` validates `min_amount`, including update transitions

## Validation Categories

| Category | Requirement | Valid examples | Invalid examples | Error |
|----------|-------------|----------------|------------------|-------|
| `RevenueDeposit` | `> 0` | `1`, `1000`, `i128::MAX` | `0`, `-1`, `i128::MIN` | `InvalidAmount` |
| `RevenueReport` | `>= 0` | `0`, `1`, `1000` | `-1`, `i128::MIN` | `InvalidAmount` |
| `MinRevenueThreshold` | `>= 0` | `0`, `100`, `1000` | `-1`, `i128::MIN` | `InvalidAmount` |
| `SupplyCap` | `>= 0` | `0`, `1_000_000` | `-1`, `i128::MIN` | `InvalidAmount` |
| `InvestmentMinStake` | `>= 0` | `0`, `100`, `1000` | `-1`, `i128::MIN` | `InvalidAmount` |
| `InvestmentMaxStake` | `>= 0` | `0`, `10_000` | `-1`, `i128::MIN` | `InvalidAmount` |
| `SnapshotReference` | `> 0` | `1`, `100`, `9999` | `0` | `InvalidAmount` |

## Public Entry Checklist

| Entrypoint | Parameter | Expected behavior | Covered by |
|------------|-----------|-------------------|------------|
| `register_offering` | `supply_cap` | Reject negative caps; do not register offering | `register_offering_rejects_negative_supply_cap_values` |
| `report_revenue` | `amount` | Reject negative reports; do not mutate audit summary | `report_revenue_rejects_negative_amount_boundaries_without_audit_mutation` |
| `deposit_revenue` | `amount` | Reject zero/negative deposits; do not create period state | `deposit_revenue_rejects_non_positive_amounts_without_mutating_period_state` |
| `deposit_revenue_with_snapshot` | `amount` | Reject zero/negative deposits; do not create period state or snapshot state | `deposit_revenue_with_snapshot_rejects_non_positive_amounts_without_state_changes` |
| `deposit_revenue_with_snapshot` | `snapshot_reference` | Reject `0`; leave snapshot cursor unchanged | `deposit_revenue_with_snapshot_rejects_zero_snapshot_reference_without_state_changes` |
| `set_investment_constraints` | `min_stake` | Reject negative minimum stake | `set_investment_constraints_rejects_negative_min_stake` |
| `set_investment_constraints` | `max_stake` | Reject negative maximum stake | `set_investment_constraints_rejects_negative_max_stake` |
| `set_investment_constraints` | `min_stake > max_stake` | Reject invalid range; preserve previous config | `set_investment_constraints_rejects_invalid_range_without_overwriting_previous_config` |
| `set_min_revenue_threshold` | `min_amount` | Reject negative threshold updates; preserve previous threshold | `set_min_revenue_threshold_rejects_negative_transition_without_overwriting_previous_value` |

## Fee-Related Note

The current branch does not expose a public fee setter that returns `InvalidAmount`.

The only public fee-related amount helper is:

| Entrypoint | Parameter | Behavior |
|------------|-----------|----------|
| `calculate_fee_for_asset` | `amount` | Pure quote helper; documented outside the rejection matrix because it does not return `Result` or mutate state |

## Security Assumptions

1. SDK wrappers may accidentally pass signed `i128` values where integrators intended unsigned economics.
2. Rejected negative paths must fail before any token movement, period creation, snapshot cursor update, or config overwrite.
3. Threshold and investment-constraint failures must preserve the last valid configuration.
4. Zero is only allowed where the contract semantics explicitly permit it, such as `report_revenue` and disabling `min_revenue_threshold`.

## Test Location

- Consolidated regression file: `src/invalid_amount_matrix_tests.rs`
- Supporting validation helper: `src/lib.rs` -> `AmountValidationMatrix`

## Risk Note

The remaining risk is documentation drift, not arithmetic ambiguity: if README tables or wrapper SDKs disagree with the contract's signedness rules, integrators can accidentally treat rejected negative paths as valid business inputs. This checklist and the consolidated tests are meant to keep that drift visible in review.
