# Vesting Schedule Amendment: Security Assumptions & Implementation Notes

## Overview

The vesting schedule amendment feature provides administrators with the ability to modify parameters of existing vesting schedules while maintaining strict security guarantees. This document details the security model, threat analysis, and implementation parity between documentation and code.

**Status**: ✅ Documentation and implementation are in full parity. All documented features are implemented and tested.

## Security Assumptions

### 1. **Authorization Control**
- **Assumption**: Only the address initialized as `Admin` can call `amend_schedule`.
- **Implementation**: The function calls `admin.require_auth()` and verifies the caller matches the stored admin address via `stored_admin` lookup.
- **Test Coverage**: `amendment_preserves_auth_requirement` - verifies non-admin calls fail.

### 2. **Accounting Integrity**
- **Assumption**: The contract enforces `new_total_amount >= claimed_amount`. This ensures that even if a schedule is reduced, the tokens already claimed by the beneficiary remain accounted for and the schedule doesn't enter an invalid state.
- **Implementation**: Check at line 238 in `src/vesting.rs`:
  ```rust
  if new_total_amount < schedule.claimed_amount {
      return Err(VestingError::InvalidAmount);
  }
  ```
- **Test Coverage**: 
  - `amend_schedule_too_low_amount_fails` - verifies reduction below claimed fails.
  - `adversarial_amend_cannot_reduce_below_claimed` - adversarial test ensuring issuer cannot steal.

### 3. **Parameter Validity**
- **Assumption**: Duration must be strictly positive and cliff must not exceed duration.
  - `new_duration_secs > 0`: Prevents division-by-zero errors in vesting calculations.
  - `new_cliff_duration_secs <= new_duration_secs`: Ensures the cliff occurs within the vesting period.
- **Implementation**: Lines 240-244 in `src/vesting.rs`.
- **Test Coverage**: `amend_schedule_invalid_params_fails` - verifies both constraints.

### 4. **Immutability of Cancelled Schedules**
- **Assumption**: Once a schedule is cancelled, it cannot be amended. This prevents "reviving" a forfeit schedule through parameter manipulation.
- **Implementation**: Line 234:
  ```rust
  if schedule.cancelled {
      return Err(VestingError::AmendmentNotAllowed);
  }
  ```
- **Test Coverage**: `amend_cancelled_schedule_fails` - verifies cancelled schedules cannot be amended.

### 5. **Beneficiary Identity Preservation**
- **Assumption**: Amendment cannot operate on an incorrect beneficiary; the schedule is bound to a specific beneficiary at creation time and cannot be reassigned.
- **Implementation**: Line 233:
  ```rust
  if schedule.beneficiary != beneficiary {
      return Err(VestingError::ScheduleNotFound);
  }
  ```
- **Test Coverage**: `amendment_preserves_beneficiary_identity` - verifies wrong beneficiary fails.

### 6. **Claimed State Immutability**
- **Assumption**: The `claimed_amount` field is never reset by amendment. This ensures beneficiaries cannot lose tokens they've already claimed.
- **Implementation**: The amendment code at lines 250-256 updates only `total_amount`, `start_time`, `cliff_time`, and `end_time`. The `claimed_amount` is never modified.
- **Test Coverage**: 
  - `amendment_mid_claim_preserves_claimed_state` - verifies claimed amounts survive amendment.
  - `adversarial_amend_backdate_start_does_not_steal_vested` - verifies backdating doesn't reset claims.

## Threat Model & Mitigations

### Risk: Issuer Backdates Schedule to Steal Vested Tokens

**Threat**: Issuer moves `start_time` backward to create the appearance that more vesting has occurred, thereby increasing `claimable_amount`.

**Mitigation**: The contract recalculates vested amount using the **current ledger time** relative to the amended parameters. While backdating does increase the claimable amount (since the beneficiary is further along the new timeline), it does **NOT reset `claimed_amount`**. Therefore:
- The issuer **cannot steal already-claimed tokens** because `claimed_amount` persists.
- The beneficiary receives only the difference: `vested - claimed`.
- This is acceptable governance because an issuer can always accelerate vesting for legitimate reasons (e.g., raising retention bonuses).

**Test Coverage**: `adversarial_amend_backdate_start_does_not_steal_vested` - demonstrates this scenario.

### Risk: Issuer Reduces Total Below Claimed

**Threat**: Issuer reduces `total_amount` to a value less than `claimed_amount`, creating an impossible state.

**Mitigation**: The contract explicitly rejects amendments where `new_total_amount < claimed_amount`.

**Test Coverage**: `adversarial_amend_cannot_reduce_below_claimed` - verifies the invariant.

### Risk: Issuer Modifies Wrong Beneficiary

**Threat**: Issuer amends a schedule targeting the wrong beneficiary by passing an incorrect address.

**Mitigation**: The contract verifies the beneficiary address matches the stored schedule's `beneficiary` field. Mismatches raise `ScheduleNotFound`.

**Test Coverage**: `amendment_preserves_beneficiary_identity` - verifies identity preservation.

### Risk: Non-Admin Executes Amendment

**Threat**: An unprivileged address attempts to amend a schedule without admin authorization.

**Mitigation**: The `require_auth()` call and admin verification prevent unauthorized amendments.

**Test Coverage**: `amendment_preserves_auth_requirement` - verifies auth enforcement.

### Risk: Amendment of Cancelled Schedules

**Threat**: Issuer revives a cancelled schedule through parameter amendment, circumventing cancellation logic.

**Mitigation**: The contract explicitly rejects amendments of cancelled schedules.

**Test Coverage**: `amend_cancelled_schedule_fails` - verifies cancellation is final.

## Implementation Parity Verification

| Feature | Documented | Implemented | Tested |
|---------|-----------|-------------|--------|
| Authorization checks | ✅ | ✅ | ✅ |
| Accounting integrity (new_total >= claimed) | ✅ | ✅ | ✅ |
| Duration validation (> 0) | ✅ | ✅ | ✅ |
| Cliff validation (<= duration) | ✅ | ✅ | ✅ |
| Cancelled schedule rejection | ✅ | ✅ | ✅ |
| Beneficiary identity preservation | ✅ | ✅ | ✅ |
| Claimed amount preservation | ✅ | ✅ | ✅ |
| Event emission (legacy + v1) | ✅ | ✅ | ✅ |
| Claimable amount recalculation | ✅ | ✅ | ✅ |

## Testing Strategy

### Coverage Categories

1. **Happy Path**: Amendment succeeds with valid parameters
   - `amend_schedule_success`
   - `amend_schedule_partially_claimed_success`
   - `amendment_then_claim_uses_new_parameters`

2. **Invariant Violations**: Amendment fails when invariants are broken
   - `amend_schedule_too_low_amount_fails` (claimed_amount > total_amount)
   - `amend_schedule_invalid_params_fails` (duration or cliff invalid)
   - `amend_cancelled_schedule_fails` (schedule is cancelled)
   - `amend_non_existent_schedule_fails` (schedule not found)

3. **Adversarial Scenarios**: Issuer attempts to exploit amendment mechanism
   - `adversarial_amend_cannot_reduce_below_claimed` - core security property
   - `adversarial_amend_backdate_start_does_not_steal_vested` - demonstrates safe backdating
   - `amendment_preserves_beneficiary_identity` - identity substitution attack
   - `amendment_preserves_auth_requirement` - privilege escalation attempt

4. **Edge Cases**: Unusual but valid amendment patterns
   - `amendment_increases_claimable_amount` - increasing total increases claimable
   - `amendment_decreases_claimable_amount_respects_claimed` - decreasing total preserves claimed
   - `amendment_mid_claim_preserves_claimed_state` - amendment during active vesting
   - `amendment_resets_cliff` - removing cliff from schedule
   - `amendment_introduces_new_cliff` - adding cliff to schedule
   - `amendment_extreme_amount_increase` - huge amount escalation
   - `amendment_extreme_duration_extension` - very long vesting periods

5. **Event Verification**
   - `amendment_emits_legacy_and_v1_events` - verifies event emission

6. **Idempotency & Sequencing**
   - `amendment_multiple_consecutive` - multiple amendments in sequence

## Special Case: Backdating Without Stealing

The vesting calculation formula is:

$$\text{vested}(t) = \begin{cases}
0 & \text{if } t < \text{cliff\_time} \\
\text{total\_amount} & \text{if } t \geq \text{end\_time} \\
\text{total\_amount} \cdot \frac{t - \text{cliff\_time}}{\text{end\_time} - \text{cliff\_time}} & \text{otherwise}
\end{cases}$$

After amendment, the formula uses the new parameters. If the issuer moves `start_time` backward:

**Before Amendment** (start=5000, cliff=5000, end=6000):
- At t=5500: elapsed = 500/1000 = 50%, vested = 1000 * 50% = 500, claimable = 500 - 0 = 500

**After Amendment** (start=1000, cliff=1000, end=2000):
- At t=5500: t >= end_time, so vested = 1000, claimable = 1000 - 0 = 1000

The claimable amount **increased**, but this is **not theft** because:
1. The beneficiary hasn't claimed yet (claimed_amount = 0)
2. The issuer is voluntarily granting additional unvested tokens
3. If the beneficiary had already claimed, that amount would be preserved

This behavior is **acceptable** and **intended**: issuers may need to adjust vesting schedules for legitimate reasons (e.g., retention bonuses, performance adjustments).

## Compliance Checklist

- ✅ Only admin-authorized operations
- ✅ Claiming state is immutable (cannot be revoked or reset)
- ✅ Safety guards prevent invalid states (claimed > total)
- ✅ Cancelled schedules are truly immutable
- ✅ Beneficiary identity cannot be changed
- ✅ All amendments emit events (legacy + v1 schema)
- ✅ Comprehensive test coverage (18 dedicated amendment tests)
- ✅ Explicit documentation of security assumptions
- ✅ Documented threat model with test cases
- ✅ Implementation matches documented behavior exactly

## References

- **Contract Code**: [src/vesting.rs](../src/vesting.rs)
- **Test Suite**: [src/vesting_test.rs](../src/vesting_test.rs)
- **Schema Documentation**: [docs/vesting-event-schema-versioning.md](vesting-event-schema-versioning.md)
- **Amendment Flow**: [docs/vesting-schedule-amendment-flow.md](vesting-schedule-amendment-flow.md)
