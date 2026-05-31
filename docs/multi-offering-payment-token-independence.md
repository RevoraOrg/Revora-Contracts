# Multi-Offering Payment Token Independence

## Overview

Payment token locking in Revora contracts is **strictly per-offering**, where offerings are uniquely identified by the 3-tuple:
```
OfferingId = (issuer: Address, namespace: Symbol, token: Address)
```

This document specifies the security properties and test coverage ensuring payment token locks do not leak between offerings in the same namespace.

## Security Properties

### Property 1: Independent Locks
Two offerings `A` and `B` in the same `(issuer, namespace)` pair, with different `token` values:
- Can lock to different payment tokens without interference
- Deposits to `A` with payment token `X` do not affect `B`'s locked token
- `get_payment_token()` returns the correct token for each offering independently

**Invariant:** For all distinct offering tuples `oid_a` and `oid_b`:
```
get_locked_payment_token_for_offering(oid_a) ≠ get_locked_payment_token_for_offering(oid_b)
OR
get_locked_payment_token_for_offering(oid_a) = None OR get_locked_payment_token_for_offering(oid_b) = None
```

### Property 2: Atomic Lock-in
First successful deposit locks the payment token. Subsequent deposits with mismatched tokens fail with `PaymentTokenMismatch`.

**Enforcement:** In `do_deposit_revenue()`:
```rust
if let Some(locked_payment_token) = get_locked_payment_token_for_offering(env, &offering_id) {
    if locked_payment_token != payment_token {
        return Err(RevoraError::PaymentTokenMismatch);
    }
}
```

This check uses the full 3-tuple `offering_id`, ensuring isolation per offering.

### Property 3: No Cross-Deposit Leakage
Failed deposit attempts (e.g., `PaymentTokenMismatch`) must not:
- Transfer tokens from issuer to contract
- Modify deposited revenue totals
- Update period counters
- Affect other offerings' state

**Test Coverage:** 
- `multi_offering_cross_deposit_does_not_mutate_state()` verifies token balances unchanged
- State checks confirm period counts and locked tokens unchanged

### Property 4: Snapshot Deposits Also Lock
`deposit_revenue_with_snapshot()` routes through `do_deposit_revenue()`, so:
- First snapshot deposit locks the payment token
- Subsequent deposits (snapshot or normal) with wrong token fail with `PaymentTokenMismatch`
- Snapshot state (snapshot_id, hash) is independent per offering

**Test Coverage:**
- `multi_offering_snapshot_deposits_independent()` verifies snapshot locks
- `multi_offering_snapshot_locks_payment_token()` verifies locking behavior

## Test Coverage

### Test Matrix

| Scenario | Test Name | Assertions |
|----------|-----------|-----------|
| **Basic Independence** | `multi_offering_different_payment_tokens_independent()` | Two offerings lock to different tokens independently |
| **Cross-Deposit Error** | `multi_offering_cross_deposit_fails_with_payment_token_mismatch()` | Mismatch deposit fails, state unchanged |
| **State Isolation** | `multi_offering_cross_deposit_does_not_mutate_state()` | Failed deposit: no token transfer, balances stable |
| **Sequential Deposits** | `multi_offering_independent_deposits_then_cross_fail()` | A(X), B(Y), then A(?Z) fails |
| **Shared Token** | `multi_offering_same_payment_token_both_offerings()` | Both offerings can lock to same token |
| **Period Independence** | `multi_offering_independent_period_sequencing()` | Period counters independent |
| **Snapshot Behavior** | `multi_offering_snapshot_deposits_independent()` | Snapshot deposits lock independently |
| **Snapshot Locking** | `multi_offering_snapshot_locks_payment_token()` | Snapshot locks, prevents subsequent wrong tokens |
| **3-Way Isolation** | `multi_offering_three_offerings_full_isolation()` | Three offerings (A, B, C) fully isolated |
| **Interleaved Deposits** | `multi_offering_interleaved_deposits_maintain_isolation()` | Interleaved A.1, B.1, A.2, B.2, A.3 maintains isolation |

### Coverage Areas

1. **Basic Isolation (5 tests)**
   - Different payment tokens: independent locks
   - Shared payment token: both can lock to same asset
   - Three-way isolation: full N-offering independence

2. **Error Paths (3 tests)**
   - PaymentTokenMismatch on cross-deposit
   - State safety: no mutation on failed deposit
   - Sequential failure handling

3. **Period Sequencing (2 tests)**
   - Independent period counters
   - Interleaved deposits maintain isolation

4. **Snapshot Behavior (2 tests)**
   - Snapshot locks independently
   - Snapshot prevents subsequent wrong tokens

## Implementation Details

### Storage Key Structure
Payment token locks use the full `OfferingId` 3-tuple as the key:
```rust
DataKey::PaymentToken(offering_id: OfferingId)
```

This ensures offerings cannot collide or interfere.

### Get Accessor
```rust
pub fn get_payment_token(
    env: Env,
    issuer: Address,
    namespace: Symbol,
    token: Address,
) -> Option<Address> {
    let offering_id = OfferingId { issuer, namespace, token };
    Self::get_locked_payment_token_for_offering(&env, &offering_id)
}
```

Returns `None` before first successful deposit, then locked token thereafter.

### Deposit Validation
`do_deposit_revenue()` validates:
1. Offering exists (3-tuple must be registered)
2. Period ID valid and sequential
3. Supply cap not exceeded
4. **Payment token matches (if already locked)**

Step 4 uses the full 3-tuple to isolate per-offering.

## Edge Cases Covered

### Edge Case 1: Registration Without Deposit
Registering an offering does NOT lock the payment token.
- `get_payment_token()` returns `None` until first successful deposit
- Different offerings can be registered with same payment token

### Edge Case 2: Failed First Deposit
If the issuer lacks sufficient balance, first deposit fails without locking:
- `get_payment_token()` still returns `None`
- Second attempt with same token succeeds if balance restored

### Edge Case 3: Snapshot Then Normal Deposit
- First snapshot deposit locks token
- Subsequent normal deposit must use same token
- Snapshot state (snapshot_id) is independent from normal deposits

### Edge Case 4: Same Token on Multiple Offerings
Multiple offerings can lock to the same payment token without collision:
- Each offering tracks its own locked token
- `get_payment_token()` returns the token for the specific offering
- Period counts are independent

## Security Assumptions

### Assumption 1: Soroban Storage Model
- `env.storage().persistent()` provides atomic, per-key storage
- Multiple keys with different `offering_id` tuples cannot collide
- `OfferingId` derives `Eq` and `Hash` correctly for use as map keys

**Verification:** Rust type system + Soroban SDK enforces proper hashing/equality.

### Assumption 2: Address and Symbol Stability
- `Address` and `Symbol` remain stable across lookups
- No partial equality or encoding issues that could cause collisions
- Soroban SDK guarantees these types hash/compare correctly

**Verification:** Soroban SDK documentation + test coverage across multiple offerings.

### Assumption 3: Atomic Transactions
- `deposit_revenue()` succeeds or fails atomically
- If PaymentTokenMismatch is raised, all prior state changes are rolled back
- No partial updates to storage

**Verification:** Soroban transaction model guarantees atomicity.

### Assumption 4: Correct OfferingId Struct
- `OfferingId` must include all three components (issuer, namespace, token)
- Omitting any component would create collisions

**Verification:** Code review + compile-time type checking.

## Test Execution

To run multi-offering tests:
```bash
cargo test --lib multi_offering
```

To run with verbose output:
```bash
cargo test --lib multi_offering -- --nocapture
```

To check coverage:
```bash
cargo tarpaulin --lib --out Html
```

Expected: All 10 new tests pass, contributing to 95%+ coverage.

## Future Enhancements

1. **Property-Based Testing:** Use proptest to generate arbitrary offering tuples and verify independence
2. **Fuzzing:** Fuzz the deposit path with random offerings, tokens, periods
3. **Formal Verification:** Prove independence invariant formally using Coq/Lean
4. **Benchmarking:** Measure performance impact of 3-tuple keys vs. flattened keys

## Related Issues

- **#287:** Payment token locking mechanism
- **#375:** Payment token locking invariant suite
- **#163:** Negative Amount Validation Matrix (applied to deposits)

## Conclusion

This comprehensive test suite validates that payment token locks are strictly per-offering with no cross-talk, ensuring financial safety and data integrity across multiple offerings in a single namespace.
