# Lockup Schedule Cliff-and-Taper (Issue #585)

## Overview

The `LockupSchedule` enum defines the unlocking schedule for an offering's tokens. In addition to single `Cliff` and `Linear` schedules, the `CliffTaper` variant allows a bulk unlock of `cliff_bps` at `cliff_ts` followed by a linear taper of the remaining BPS until `taper_end_ts`.

```rust
pub enum LockupSchedule {
    Cliff { unlock_ts: u64 },
    Linear { start_ts: u64, end_ts: u64 },
    CliffTaper { cliff_ts: u64, cliff_bps: u32, taper_end_ts: u64 },
}
```

## Calculation & Formula

At timestamp `now`:

- **`now < cliff_ts`**: 0 BPS (0% unlocked).
- **`cliff_ts <= now < taper_end_ts`**:
  $$\text{unlocked\_bps} = \text{cliff\_bps} + \frac{(10\,000 - \text{cliff\_bps}) \times (\text{now} - \text{cliff\_ts})}{\text{taper\_end\_ts} - \text{cliff\_ts}}$$
- **`now >= taper_end_ts`**: 10 000 BPS (100% unlocked).

## Edge Cases

- **`cliff_bps == 10_000` & `taper_end_ts == cliff_ts`**: 10 000 BPS unlocks immediately at `cliff_ts`.
- **Validation**: `cliff_bps` must be $\le 10\,000$ (returns `InvalidRevenueShareBps` otherwise). `taper_end_ts` must be $\ge \text{cliff\_ts}$ (returns `InvalidAmount` otherwise).

## Public API

| Function | Auth | Complexity | Event |
|---|---|---|---|
| `set_lockup_schedule(issuer, namespace, token, schedule: LockupSchedule)` | Issuer | O(1) | `EVENT_LOCKUP_SET` (`"lock_set"`) |
| `get_lockup_schedule(issuer, namespace, token) -> Option<LockupSchedule>` | None | O(1) | — |
| `get_unlocked_bps(issuer, namespace, token) -> u32` | None | O(1) | — |
