# Multisig Owner Removal Safety

**Issue:** #296 · **Label:** security, tests, P1

## Overview

`ProposalAction::RemoveOwner` allows the multisig to remove an owner via the standard
propose → approve → execute flow. This document describes the invariants enforced, the
threat model, and the test coverage that makes those invariants falsifiable in CI.

## Invariants

| # | Invariant | Enforcement point |
|---|-----------|-------------------|
| I1 | `remaining_owners >= threshold` after removal | `execute_action` → `RemoveOwner` branch |
| I2 | Target address must be a current owner | `execute_action` → `RemoveOwner` branch |
| I3 | Removal itself requires threshold approvals | standard `execute_action` approval check |
| I4 | Removed owner's prior approvals on other proposals are preserved | no retroactive state mutation |

## Threat Model

### Griefing via threshold breach (I1)

An attacker who controls `threshold - 1` owners could propose removing the remaining
owners one by one until `remaining_owners < threshold`, permanently bricking the multisig.

**Mitigation:** `execute_action` checks `(owners.len() - 1) >= threshold` before
persisting the removal. If the check fails the transaction reverts with `LimitReached`.

### Phantom-address removal (I2)

A proposal targeting an address that is not an owner would silently succeed (no-op) if
the membership check were absent, wasting a proposal slot and potentially confusing
off-chain tooling.

**Mitigation:** `execute_action` checks `owners.contains(&old_owner)` and returns
`NotAuthorized` if the address is not found.

### Proposer removal mid-flight

An owner proposes action A, then is removed before A reaches threshold. Their approval
on A is already recorded and counts toward threshold. Remaining owners can still reach
threshold and execute A without the removed owner.

**Mitigation:** No special handling needed; approvals are stored in the proposal struct
and are not invalidated by owner removal. Test `multisig_remove_proposer_pending_proposal_still_executable` covers this.

### Last-approver removal

If the only approver of a pending proposal is removed, the approval count drops below
threshold and the proposal can no longer be executed. This is the correct behavior: the
remaining owners must create a new proposal.

**Test:** `multisig_remove_last_approver_blocks_execution`.

## Bug Fixes Applied (PR #296)

Two compile-blocking bugs existed in the `RemoveOwner` branch of `execute_action`:

1. **Undefined variable `addr`** — the membership check used `&addr` (undefined) instead
   of `&old_owner`. Fixed to `owners.contains(&old_owner)`.

2. **Immutable binding reassignment** — `owners = new_owners` attempted to reassign an
   immutable binding. Fixed by writing `new_owners` directly to storage:
   `env.storage().persistent().set(&DataKey::MultisigOwners, &new_owners)`.

3. **Duplicated guard block** — `execute_action` contained a copy-paste of the
   executed/expiry/threshold checks followed by a second `get` of the same proposal key.
   The duplicate block was removed; the single authoritative check now precedes the
   `match` on `proposal.action`.

## Test Coverage

All tests live in `src/test.rs` in the multisig section.

| Test | Scenario | Expected outcome |
|------|----------|-----------------|
| `multisig_remove_owner_action_removes_owner` | Happy path: remove one of three owners | Owner removed, list shrinks to 2 |
| `multisig_remove_owner_that_would_break_threshold_fails` | Sequential removals until 1 owner < threshold=2 | Second removal reverts |
| `multisig_remove_nonexistent_owner_fails` | Target address not in owner list | Reverts with error |
| `multisig_remove_owner_exact_threshold_boundary_succeeds` | 3 owners, threshold=2, remove one → 2==threshold | Succeeds |
| `multisig_remove_owner_below_threshold_is_rejected` | 3→2→1 owners, threshold=2 | Second removal reverts |
| `multisig_remove_proposer_pending_proposal_still_executable` | Proposer removed; remaining owners reach threshold | Pending proposal executes |
| `multisig_remove_last_approver_blocks_execution` | Only approver removed before threshold met | Execution reverts |
| `multisig_owner_self_removal_succeeds_with_threshold` | Owner proposes own removal; others approve | Owner removed |

## Security Notes

- **No time-lock:** Proposals execute immediately once threshold is met. For
  high-security deployments, add a time-lock delay between threshold-met and execution.
- **No proposal expiry on removal:** A stale removal proposal can be executed at any
  time. Combine with `SetProposalDuration` to bound proposal lifetime.
- **Soroban single-transaction constraint:** Each owner must approve in a separate
  transaction; multi-party auth in one transaction is not supported by Soroban.
