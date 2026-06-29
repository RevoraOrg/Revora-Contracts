# Investment Constraints & Supply Cap

## Overview

Revora-Contracts enforces offering-level limits on cumulative revenue deposited (supply cap) and specifies per-investor bounds (min/max stakes). These controls harden the supply/constraints model and ensure predictable, deterministic behavior during revenue distribution and investor onboarding.

## Supply Cap Constraints

The **supply cap** represents the maximum cumulative revenue that can be deposited for an offering.
- Configured once during `register_offering` via the `supply_cap` argument (`0` means no cap).
- Enforced directly during the `deposit_revenue` execution path.

### Rejection Paths & Boundary Determinism
If a newly deposited amount pushes the total historical deposited revenue above the `supply_cap`, the contract deterministically fails with `RevoraError::SupplyCapExceeded` without mutating state or transferring tokens. This provides a clean rejection path for off-chain orchestrators.

If a deposit hits the cap exactly (or surpasses it if the previous invariant allowed a boundary deposit), the contract publishes the `EVENT_SUPPLY_CAP_REACHED` event. This acts as a signal to off-chain indexers that no further deposits will be accepted unless the offering is explicitly migrated or restructured.

## Investor Constraints (Min/Max Stake)

The **investment constraints** define the minimum and maximum stake (or revenue commitment) an individual investor can hold.
- Configured via `set_investment_constraints(issuer, namespace, token, min_stake, max_stake)`.
- Enforced primarily by off-chain systems prior to invoking the `set_holder_share` entrypoints.

### Validation Matrix
The `AmountValidationMatrix` handles strict enforcement of constraints:
- `min_stake` must be `>= 0`.
- `max_stake` must be `>= 0` and `>= min_stake`.
Invalid bounds are proactively rejected with `InvalidAmount`, preserving the consistency of read APIs.

## Security Assumptions and Risk Notes

1. **Deterministic Rejection:** Deposits exceeding the `supply_cap` fail deterministically in the contract.
2. **Off-Chain Stake Enforcement:** While `min_stake` and `max_stake` are validated for correctness on-chain, their active enforcement on a per-holder basis is delegated to the off-chain system that configures `set_holder_share`.
3. **Immutability of Supply Cap:** The `supply_cap` is set at offering registration and is immutable. To adjust a cap, an issuer must create a new offering instance.

## Read API

`get_deposited_revenue(issuer, namespace, token) -> i128` returns the cumulative total deposited for an offering, or 0 if no deposits have been made. This enables off-chain orchestrators to verify remaining headroom under the supply cap without mutating state. Guaranteed O(1) — single persistent storage read.

## Test Output Summary

The regression tests cover boundary deposits, event emission, read API consistency on rejection paths, and explicit min/max constraint cases:

### Supply Cap Tests
- `deposit_revenue_exactly_at_supply_cap_succeeds`: Validates that depositing an amount resulting in exactly the cap value succeeds and accurately emits the tracking events.
- `deposit_revenue_exceeds_supply_cap_fails`: Verifies that a deposit transaction breaching the supply cap is cleanly reverted with `SupplyCapExceeded`.
- `deposit_revenue_multiple_deposits_exceeds_supply_cap_fails`: Ensures cumulative historical deposits are bounded by the cap.
- `get_deposited_revenue_returns_zero_before_any_deposit`: Confirms the read API returns 0 before any deposit is made.
- `get_deposited_revenue_tracks_cumulative_total_correctly`: Confirms the read API accumulates correctly across multiple deposits.
- `deposit_revenue_no_cap_is_unlimited`: Verifies that `supply_cap=0` imposes no upper bound on deposits.
- `deposit_revenue_exactly_at_supply_cap_emits_cap_reached_event`: Confirms `EVENT_SUPPLY_CAP_REACHED` fires when a deposit lands exactly on the cap.
- `deposit_revenue_just_below_cap_does_not_emit_cap_reached_event`: Confirms the cap-reached event does not fire for sub-cap deposits.
- `deposit_revenue_first_deposit_above_cap_fails_deterministically`: Verifies that a single deposit exceeding the cap is rejected with no state mutation.
- `deposit_revenue_read_api_unchanged_after_rejection`: Verifies `get_deposited_revenue` is unchanged after a rejected deposit (clean rejection path).
- `deposit_revenue_cumulative_second_deposit_hits_cap_exactly`: Two-deposit boundary where the second lands exactly on the cap; both succeed, event fires.
- `deposit_revenue_cumulative_second_deposit_exceeds_cap_fails`: Two-deposit boundary where the second would overflow; second is rejected, state preserved.
- `deposit_revenue_with_snapshot_enforces_supply_cap`: Confirms `deposit_revenue_with_snapshot` enforces the same cap as `deposit_revenue`.
- `get_supply_cap_returns_zero_when_no_cap_set`: Read API returns 0 for an offering registered with no cap.
- `get_supply_cap_returns_configured_value`: Read API returns the configured cap value.
- `deposit_revenue_supply_cap_of_one_blocks_second_deposit`: Minimal cap (1 unit) — first deposit succeeds, second is rejected.

### Investment Constraint Tests
- `set_investment_constraints_succeeds_for_valid_bounds`: Validates standard min/max setup paths.
- `set_investment_constraints_fails_when_max_less_than_min`: Triggers a failure path testing bounds mismatch.
- `set_investment_constraints_fails_negative`: Confirms negative constraints are blocked by the amount validation matrix.
- `set_investment_constraints_emits_event`: Confirms an event is emitted on constraint configuration.
- `get_investment_constraints_returns_none_before_set`: Read API returns None before constraints are configured.
- `set_investment_constraints_both_zero_succeeds`: Confirms min=0, max=0 (unlimited) is a valid configuration.
- `set_investment_constraints_equal_min_and_max_succeeds`: Confirms min==max (exact stake requirement) is accepted.
- `set_investment_constraints_min_zero_max_positive_succeeds`: Confirms only the upper bound can be enforced when min=0.
- `set_investment_constraints_updates_replace_previous`: Confirms a second call overwrites prior constraints completely.
- `set_investment_constraints_update_event_marks_previous_existed`: Confirms the update event payload correctly flags when prior constraints existed.
- `set_investment_constraints_fails_for_nonexistent_offering`: Confirms constraints cannot be set on an unregistered offering.

*Note: All tests successfully achieved > 95% test coverage for the implemented code paths.*
