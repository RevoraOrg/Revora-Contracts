# Token Vesting Core

**Module:** `src/vesting.rs` + `src/vesting_test.rs`
**Issue:** RC26Q2-C26 / #275
**Branch:** `feature/vesting-core-invariants`

---

## Overview

The vesting module implements **cliff + linear-schedule token vesting** on Soroban (Stellar).
An issuer deposits tokens into the contract at registration time; a beneficiary can claim
vested tokens progressively as on-chain time advances.

---

## Design

### Time model

All time checks use `env.ledger().timestamp()` — the Unix timestamp of the closing ledger,
set by Stellar consensus.  It is:

- Monotonically non-decreasing across ledgers.
- Not manipulable per-transaction or by any single party.
- Available as a `u64` (seconds since Unix epoch).

This is identical to the time source used by the existing claim-delay and time-window
features in the main contract.

### Schedule parameters

| Field          | Type   | Meaning                                                              |
|----------------|--------|----------------------------------------------------------------------|
| `issuer`       | Address| Address that funds and manages the schedule.                        |
| `beneficiary`  | Address| Recipient of vested tokens.                                         |
| `token`        | Address| SEP-41 token contract.                                              |
| `total_amount` | i128   | Total tokens to vest (must be > 0).                                 |
| `cliff_ts`     | u64    | Unix timestamp before which nothing unlocks.                        |
| `start_ts`     | u64    | Start of linear vesting window (must be ≥ `cliff_ts`).             |
| `end_ts`       | u64    | End of linear vesting window — 100 % vested here (must be > `start_ts`). |

### Vesting formula

```
vested(now) =
    0                                        if now < cliff_ts
    0                                        if cliff_ts ≤ now < start_ts
    total_amount * (now - start_ts)
        / (end_ts - start_ts)               if start_ts ≤ now < end_ts
    total_amount                             if now ≥ end_ts
```

This supports:

- **Pure cliff** — set `cliff_ts == start_ts`; tokens unlock linearly immediately after the cliff.
- **Cliff + delay** — set `start_ts > cliff_ts`; tokens stay locked until `start_ts` even after the cliff passes.
- **Instant fully-vested** — not directly supported; the minimum schedule width is 1 second (`end_ts = start_ts + 1`).

---

## Public API

### `vesting_register`

```rust
pub fn vesting_register(
    env: Env,
    issuer: Address,
    beneficiary: Address,
    token: Address,
    total_amount: i128,
    cliff_ts: u64,
    start_ts: u64,
    end_ts: u64,
) -> Result<(), VestingError>
```

Registers a new schedule and transfers `total_amount` from `issuer` into the contract.
The issuer must call `approve` on the token contract first (standard SEP-41 pattern).

**Errors:** `InvalidAmount`, `InvalidTimestamps`, `ScheduleAlreadyExists`.

---

### `vesting_claim`

```rust
pub fn vesting_claim(env: Env, beneficiary: Address) -> Result<i128, VestingError>
```

Transfers all newly-vested tokens to `beneficiary`.  Returns `0` (no error) when nothing
new has vested since the last claim (idempotent).

**Errors:** `ScheduleNotFound`, `NothingToClaimYet` (before cliff).

---

### `vesting_revoke`

```rust
pub fn vesting_revoke(env: Env, issuer: Address, beneficiary: Address) -> Result<(), VestingError>
```

Revokes a schedule.  Vested-but-unclaimed tokens are sent to `beneficiary`; unvested tokens
are returned to `issuer`.  The schedule is deleted from storage.

**Errors:** `ScheduleNotFound`, `Unauthorized`.

---

### Read-only queries

| Method                   | Returns                   | Description                                   |
|--------------------------|---------------------------|-----------------------------------------------|
| `get_vesting_schedule`   | `Option<VestingSchedule>` | Full schedule, or `None`.                    |
| `get_claimed_amount`     | `i128`                    | Cumulative tokens already claimed.           |
| `get_vested_amount`      | `Option<i128>`            | Tokens vested at current ledger time.        |
| `get_claimable_amount`   | `Option<i128>`            | Vested minus already claimed.                |
| `get_vesting_schedules`  | `Vec<Option<...>>`        | Batch query for off-chain dashboards.        |

---

## Invariants verified by tests

| # | Invariant | Test(s) |
|---|-----------|---------|
| 1 | Nothing claimable before `cliff_ts` | `test_claim_before_cliff_fails` |
| 2 | Linear interpolation correct | `test_partial_release_at_midpoint` |
| 3 | Cumulative claims never exceed `total_amount` | `test_no_overclaim_after_full_vest` |
| 4 | Cursor is monotonically increasing | `test_cursor_advances_monotonically` |
| 5 | Double-claim at same timestamp returns 0 | `test_idempotent_claim_same_timestamp` |
| 6 | `start_ts < cliff_ts` rejected at registration | `test_register_start_before_cliff_fails` |
| 7 | `end_ts ≤ start_ts` rejected at registration | `test_register_end_not_after_start_fails` |
| 8 | After cliff but before `start_ts`: 0 vested | `test_pure_cliff_period_no_unlock_before_start` |
| 9 | Revoke splits tokens correctly at midpoint | `test_revoke_midway_splits_correctly` |
| 10 | Non-issuer cannot revoke | `test_revoke_wrong_issuer_fails` |
| 11 | Non-existent schedule yields `ScheduleNotFound` | `test_claim_on_nonexistent_schedule_fails` |
| 12 | Pure-function `vested_amount` boundary values | `test_vested_amount_pure_function` |

---

## Error codes

| Code | Name                   | Meaning                                              |
|------|------------------------|------------------------------------------------------|
| 100  | `ScheduleAlreadyExists`| A schedule already exists for this beneficiary.     |
| 101  | `ScheduleNotFound`     | No schedule for this beneficiary.                   |
| 102  | `InvalidAmount`        | `total_amount` ≤ 0.                                 |
| 103  | `InvalidTimestamps`    | `start_ts < cliff_ts` or `end_ts ≤ start_ts`.       |
| 104  | `NothingToClaimYet`    | Cliff has not been reached.                         |
| 105  | `Unauthorized`         | Caller is not the issuer (for revocation).          |

---

## Events

| Topic       | Payload                                              | When                         |
|-------------|------------------------------------------------------|------------------------------|
| `vest_reg`  | `(total_amount, cliff_ts, start_ts, end_ts)`        | After `vesting_register`.   |
| `vest_clm`  | `(amount_claimed, new_total_claimed, total_amount)` | After a non-zero `vesting_claim`. |
| `vest_rev`  | `(beneficiary_due, issuer_due)`                     | After `vesting_revoke`.     |

---

## Storage layout

Two persistent keys per beneficiary:

| Key                            | Value              | TTL policy |
|--------------------------------|--------------------|------------|
| `VestingKey::Schedule(addr)`   | `VestingSchedule`  | Persistent |
| `VestingKey::Claimed(addr)`    | `i128`             | Persistent |

Both keys are deleted on revocation.

---

## Running tests

```bash
# All tests (single-threaded for deterministic Soroban output)
cargo test -- --test-threads=1

# Vesting tests only
cargo test vesting -- --test-threads=1

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Format check
cargo fmt --all -- --check
```

---

## Security assumptions & risk notes

1. **Time source** — The contract relies on `env.ledger().timestamp()`, which is
   set by Stellar validator consensus.  Validators can, in principle, produce
   ledgers with a timestamp slightly ahead of wall-clock time, but the Stellar
   protocol keeps this within tight bounds (seconds, not minutes).  For vesting
   schedules measured in days or months, this is not a meaningful attack surface.

2. **Token contract trust** — The vesting contract calls SEP-41 `transfer`.
   A malicious token could re-enter; however Soroban's cross-contract call model
   makes re-entrancy structurally very difficult and the `claimed` cursor is
   updated *before* the token transfer (checks-effects-interactions pattern).

3. **One schedule per beneficiary** — The current design allows one active
   schedule per beneficiary address.  Issuers needing multiple tranches should
   either revoke-and-re-register or use distinct beneficiary addresses.

4. **No supply-cap enforcement** — The contract does not validate that the issuer
   has minted only a certain amount of the token.  The `total_amount` parameter is
   advisory; the token contract's own supply logic governs issuance.

5. **Revocation is issuer-initiated** — There is no beneficiary-initiated
   early-exit mechanism.  If a schedule needs to be paused, the issuer must
   revoke and optionally re-register a new schedule starting from the current
   vested amount.

6. **No upgradeability** — Consistent with the main contract's design philosophy,
   this module deploys as a single WASM contract.  Storage-layout changes require
   a new deployment and migration.

---
