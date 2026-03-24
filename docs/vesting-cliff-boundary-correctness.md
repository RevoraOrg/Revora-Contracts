# Vesting cliff boundary correctness

This document specifies how `RevoraVesting` interprets **cliff duration** and ledger timestamps so integrators and auditors can rely on deterministic, reviewable behavior (issue #171).

## Definitions

- `start_time`: schedule start (seconds).
- `cliff_duration_secs`: non-negative offset; **cliff end** is `cliff_time = start_time + cliff_duration_secs`.
- `duration_secs`: total schedule length from `start_time`; **vesting end** is `end_time = start_time + duration_secs`.
- Validation: `cliff_duration_secs <= duration_secs` (strict `>` is rejected).

## Boundary semantics

| Condition | Vested amount |
|-----------|----------------|
| `now < cliff_time` | `0` |
| `now == cliff_time` | `0` (first instant of the linear segment; elapsed = 0) |
| `cliff_time < now < end_time` | `floor(total_amount × (now − cliff_time) / (end_time − cliff_time))`, capped at `total_amount` |
| `now >= end_time` | `total_amount` |

So the **cliff holds zero vesting** for every second strictly before `cliff_time`. The **linear window** is `[cliff_time, end_time)` in continuous terms; at `end_time` the position is fully vested.

## Degenerate case: `cliff_duration_secs == duration_secs`

Then `cliff_time == end_time`. There is no open linear interval: nothing vests until `now >= end_time`, at which point **100%** vests in one step (pure cliff / big-bang unlock).

## Security and abuse notes

- **Timestamp source**: Uses Soroban ledger timestamp; callers must not assume wall-clock alignment across chains.
- **Cancelled schedules**: `vested_amount` is always `0` after cancel, regardless of time.
- **Rounding**: Integer division truncates toward zero; sum of holder allocations off-chain should be reconciled with this rounding mode where relevant.

## Tests

See `src/vesting_test.rs`:

- `claimable_at_exact_cliff_timestamp_is_zero`
- `claimable_one_second_after_cliff_matches_linear_slice`
- `claimable_last_second_before_end_one_step_below_full`
- `cliff_equals_duration_unlocks_full_amount_only_at_end`
