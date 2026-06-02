# Snapshot Monotonicity Replay Stress Tests

**Module:** `src/test_snapshot_monotonicity_replay.rs`  
**Issue:** Snapshot replay rejection not exercised under rapid out-of-order coordinator behaviour  
**Branch:** `feat/snapshot-monotonicity-replay-tests`

---

## Problem Statement

`commit_snapshot` enforces strict monotonicity: a new `snapshot_ref` is only
accepted when it is **strictly greater** than the last committed ref for the
same offering. The existing test suite covered the basic equal-ref and
less-than-ref cases in isolation, but did not exercise a rapid out-of-order
replay sequence that emulates a buggy or malicious off-chain coordinator
re-sending stale references in quick succession.

---

## Security Assumptions Validated

| Assumption | Mechanism |
|---|---|
| Strict monotonicity | `snapshot_ref <= last_ref` → `OutdatedSnapshot` |
| Write-once per ref | Second commit at same ref → `OutdatedSnapshot` |
| No partial state mutation | Rejected calls leave `LastSnapshotRef` unchanged |
| Forward-only advancement | `last_ref` never decreases across any sequence |
| Per-offering isolation | Replay on offering A cannot corrupt offering B |
| Typed errors | All rejections surface as `RevoraError::OutdatedSnapshot` via `try_commit_snapshot` |

---

## Test Coverage

### Primary stress test — `snapshot_replay_stress_out_of_order_sequence`

Drives the ref sequence `[5, 3, 5, 4, 6]` against a single offering:

| Step | `snapshot_ref` | Condition | Expected result | `last_ref` after |
|------|---------------|-----------|-----------------|-----------------|
| 1 | 5 | 5 > 0 (initial) | **Accepted** | 5 |
| 2 | 3 | 3 ≤ 5 | `OutdatedSnapshot` | 5 |
| 3 | 5 | 5 ≤ 5 (equal) | `OutdatedSnapshot` | 5 |
| 4 | 4 | 4 ≤ 5 | `OutdatedSnapshot` | 5 |
| 5 | 6 | 6 > 5 | **Accepted** | **6** |

Final assertion: `get_last_snapshot_ref == 6`.

### Edge case — `snapshot_ref_zero_is_rejected`

`snapshot_ref == 0` is rejected because `last_ref` initialises to `0` and the
invariant requires strictly greater. Prevents coordinators from committing a
"null" snapshot.

### Edge case — `snapshot_ref_u64_max_is_accepted_then_blocks_further_commits`

`u64::MAX` is a valid ref and must be accepted when greater than `last_ref`.
After acceptance, no further commit is possible (no value exceeds `u64::MAX`),
so any retry returns `OutdatedSnapshot`.

### Edge case — `equal_ref_retry_always_returns_outdated_snapshot`

Committing the same ref twice in a row always returns `OutdatedSnapshot` on
the second attempt. The contract is **write-once per ref**, not idempotent.

### Invariant test — `last_ref_never_decreases_across_mixed_sequence`

Sequence `[accept 10, reject 7, reject 10, accept 20, reject 15]` verifies
that `last_ref` equals the highest accepted ref (`20`) and never decreased at
any intermediate step.

### Isolation test — `snapshot_replay_is_isolated_per_offering`

Replay attempts on offering A (token A) do not affect `last_ref` of offering B
(token B, same issuer and namespace). Storage keys are scoped per
`(issuer, namespace, token)`.

---

## Implementation Notes

- All assertions use `try_commit_snapshot` to obtain typed `Result<(), RevoraError>`.
- No `unwrap()` or `expect()` in assertion paths — failures produce descriptive
  messages via the `assert!` format argument.
- `env.mock_all_auths()` is used so auth checks do not interfere with the
  monotonicity logic under test.
- Each test is fully self-contained via the `setup()` helper; no shared mutable
  state between tests.

---

## Running the Tests

```bash
cargo test test_snapshot_monotonicity_replay --all
```

To run the full suite:

```bash
cargo test --all
```
