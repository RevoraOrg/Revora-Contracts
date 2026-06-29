# Multisig Initialization Validation

This document describes the security assumptions, design rationale, and validation rules applied to the `init_multisig` capability in the `Revora-Contracts` project.

## Context
The multisig initialization function transitions the contract into a secure multi-signature administration model. Due to the high-stakes nature of this action (often transferring full control to a decentralized set of owners), strict validation is essential to prevent operational pitfalls, lockouts, or unintended takeovers.

## Security Assumptions

1. **Singleton Admin Authority**
   The contract is initially deployed and configured by a single `Admin` address. It is assumed that only the recognized `Admin` is authorized to transition the contract's governance to a multi-signature model. Initialization attempts by any other address represent an abuse vector and are strictly denied.

2. **Bounded Execution Contexts**
   Smart contract environments (such as Soroban) enforce strict computational and memory budgets. Unbounded iterations can lead to out-of-gas errors or budget exhaustion. 
   - **Assumption:** The number of multisig owners must be small and fixed.
   - **Enforcement:** A hard limit of `MAX_MULTISIG_OWNERS = 20` is enforced to ensure that iterations (such as duplicate checks or multi-signature aggregations) always cost a predictable and small amount of gas.

3. **Owner Integrity**
   Multisig threshold logic assumes independent and unique signers. If duplicate owner addresses are allowed, a single entity could trivially satisfy the threshold by signing multiple times or breaking quorum assumptions.
   - **Enforcement:** Duplicate owners are explicitly rejected during initialization via an $O(N^2)$ cross-comparison. Due to the small bounded $N$ (max 20), this check is highly efficient.

## Validation Rules

When `init_multisig` is called, the following checks are sequentially evaluated:

1. **Caller Verification** (`RevoraError::NotAuthorized`)
   The `caller` is verified against the currently stored `Admin`. Since `caller.require_auth()` is enforced, the caller must cryptographically sign the transaction.

2. **Re-initialization Guard** (`RevoraError::LimitReached`)
   The system checks whether the multisig has already been initialized (via the presence of `DataKey::MultisigThreshold`). Initialization may occur exactly once.

3. **Owner Array Validity** (`RevoraError::LimitReached`)
   - The array must not be empty.
   - The array size must not exceed `MAX_MULTISIG_OWNERS`.
   - The threshold must be greater than 0 and less than or equal to `owners.len()`.
   
4. **Duplicate Prevention** (`RevoraError::LimitReached`)
   The `owners` array is scanned for duplicates. If any two indices contain the same exact address, initialization aborts.

5. **Duration Validity** (`RevoraError::InvalidAmount`)
   - The `proposal_duration` must be greater than 0 seconds.
   - The `proposal_duration` must not exceed `MAX_PROPOSAL_DURATION` (365 days = 31,536,000 seconds).
   - Zero-duration would cause immediate proposal expiry; excessive duration creates operational risk.
   - Duration is persisted to `MultisigProposalDuration` storage key for use by `propose_action`.

## Event Emission
Once all state modifications succeed, the contract emits an `ms_init` (`EVENT_MULTISIG_INIT`) event containing:
- Topic 0: `ms_init`
- Topic 1: The `caller` address (the admin who initialized it)
- Data: A tuple of `(owners_count: u32, threshold: u32)`

This provides off-chain indexers deterministic proof of the exact configuration successfully agreed upon.

## Security Risks and Mitigations

### Risk 1: Uninitialized Duration
**Impact**: If `proposal_duration` is not stored during `init_multisig`, all subsequent `propose_action` calls will fail with `NotInitialized`, permanently bricking the multisig governance.

**Mitigation**: Duration is now validated and persisted during initialization. The `MultisigProposalDuration` storage key is set atomically with other multisig state.

### Risk 2: Zero or Excessive Duration
**Impact**: 
- Zero duration: Proposals expire immediately upon creation, making governance impossible.
- Excessive duration (e.g., 100 years): Stuck proposals cannot be cleaned up, creating ledger bloat and operational confusion.

**Mitigation**: Duration is bounded to [1, 365 days] range via validation in `init_multisig`.

### Risk 3: Misconfigured Threshold
**Impact**: Threshold > owners or threshold = 0 makes it impossible to ever reach quorum, permanently locking the multisig.

**Mitigation**: Validated during initialization; threshold must be in [1, owners.len()].

### Risk 4: Duplicate Owners
**Impact**: A single entity could satisfy the threshold by signing multiple times with the same address, defeating multisig security assumptions.

**Mitigation**: O(N²) duplicate check during initialization. This is computationally acceptable due to the `MAX_MULTISIG_OWNERS = 20` bound.

### Risk 5: Too Many Owners
**Impact**: An unbounded owner list could cause gas exhaustion during duplicate checks or proposal operations.

**Mitigation**: Hard limit of 20 owners enforced. This ensures predictable gas costs for all multisig operations.
