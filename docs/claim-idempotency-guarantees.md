# Claim Idempotency Guarantees

This document describes the idempotency and atomicity invariants of the `claim()` entrypoint in `RevoraRevenueShare`. It is intended for integrators, auditors, and contributors.

---

## Overview

`claim(holder, issuer, namespace, token, max_periods)` allows a token holder to collect accumulated revenue across multiple deposited periods. The function is designed so that:

- A failed or repeated call **never double-pays** and **never corrupts** the claim index.
- Every error path exits **before** any storage write or token transfer.
- The claim index (`LastClaimedIdx`) advances **only** for periods that were actually processed.

---

## Invariants

### 1. No Double-Pay

`LastClaimedIdx` is written to persistent storage **after** the token transfer completes. Soroban's atomic transaction model guarantees that if the transfer panics (e.g. insufficient contract balance), the entire transaction is rolled back — the index is not advanced and the holder may retry.

```
read  LastClaimedIdx  →  iterate periods  →  transfer tokens  →  write LastClaimedIdx
                                                     ↑
                                          if this panics, nothing is committed
```

### 2. Index Advances Only on Processed Periods

The index is set to `last_claimed_idx`, which is incremented only for periods that passed the `ClaimDelaySecs` check. If every period in the window is blocked by the delay, `last_claimed_idx == start_idx` and the function returns `Err(ClaimDelayNotElapsed)` **without writing any state**.

### 3. Zero-Payout Periods Advance the Index

A period where `revenue * share_bps / 10_000 == 0` (due to integer truncation) still increments `last_claimed_idx`. No token transfer is issued for a zero amount. This prevents a holder from being permanently stuck behind a dust period.

### 4. Exhausted State Is Safe to Retry

Once `LastClaimedIdx >= PeriodCount`, every call returns `Err(NoPendingClaims)` immediately — before any storage read beyond the index check. Callers may retry any number of times without side effects.

### 5. Per-Holder Index Isolation

Each holder's progress is stored under `DataKey::LastClaimedIdx(offering_id, holder)`. One holder's claim never reads or writes another holder's index.

### 6. Pre-Mutation Checks

The following checks fire **before** any storage write or token transfer, in order:

| Order | Check | Error on failure |
|-------|-------|-----------------|
| 1 | `holder.require_auth()` | host panic |
| 2 | `is_blacklisted(token, holder)` | `HolderBlacklisted` |
| 3 | `get_holder_share(...) == 0` | `NoPendingClaims` |
| 4 | `require_claim_window_open(...)` | `ClaimWindowClosed` |
| 5 | `start_idx >= period_count` | `NoPendingClaims` |
| 6 | delay check on first period | `ClaimDelayNotElapsed` |

None of these checks write state. A caller that fails any check may retry after the condition changes (e.g. after being removed from the blacklist, or after the delay elapses) and will find the index exactly where it was left.

---

## Security Assumptions

- **Trust model**: The contract trusts `holder.require_auth()` for authentication. Off-chain systems must not submit claim transactions on behalf of holders without their explicit authorization.
- **Blacklist timing**: A holder blacklisted between a deposit and a claim loses access to that period's revenue for as long as they remain blacklisted. Removing them from the blacklist restores access; the index is unchanged.
- **Share at claim time**: `share_bps` is read at claim time, not at deposit time. If an issuer reduces a holder's share between deposit and claim, the holder receives the lower share. Integrators should document this behavior to holders.
- **Payment token locked**: The payment token is locked on first deposit. The contract will always transfer the same token regardless of when the holder claims.
- **No on-chain reentrancy**: Soroban does not support reentrancy within a single contract invocation. The index write after the transfer is safe.

---

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| Claim with no deposits | `Err(NoPendingClaims)` — no state written |
| Claim with zero share | `Err(NoPendingClaims)` — no state written |
| Claim while blacklisted | `Err(HolderBlacklisted)` — no state written |
| Claim before delay elapses | `Err(ClaimDelayNotElapsed)` — no state written |
| Claim after all periods exhausted | `Err(NoPendingClaims)` — no state written |
| Claim with `max_periods` > available | Processes all available periods, capped at `MAX_CLAIM_PERIODS` (50) |
| Claim with `max_periods = 0` | Treated as `MAX_CLAIM_PERIODS` (50) |
| Period with zero revenue | Index advances, no transfer, payout contribution is 0 |
| Period where truncation yields 0 payout | Index advances, no transfer |
| New deposit after full claim | Claimable from current index — no re-processing of old periods |

---

## Test Coverage

All invariants above are covered by `src/test.rs :: mod claim_idempotency`:

| Test | Invariant |
|------|-----------|
| `exhausted_claim_returns_no_pending_claims_without_side_effects` | 4 |
| `delay_blocked_claim_leaves_index_unchanged` | 2 |
| `zero_payout_period_advances_index_no_transfer` | 3 |
| `holder_indices_are_fully_isolated` | 5 |
| `blacklisted_holder_rejected_before_state_mutation` | 6 (blacklist) |
| `zero_share_holder_rejected_before_state_mutation` | 6 (share) |
| `partial_batch_no_reprocessing` | 1, 2 |
| `new_periods_after_full_claim_are_claimable` | 4 |
| `max_periods_one_processes_exactly_one_period` | 2 |
| `delay_partial_window_advances_index_to_first_blocked` | 2 |
| `regression_no_state_write_on_no_pending_claims` | 4 |

Run with:

```bash
cargo test claim_idempotency
```

---

## Related

- `src/lib.rs` — `RevoraRevenueShare::claim()` — inline invariant documentation
- `docs/period-amount-fuzz-notes.md` — fuzz notes on period amounts
- `README.md` — Holder Claims Flow sequence diagram
