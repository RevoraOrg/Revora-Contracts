# Time Window Boundary Matrix

## Overview

This document describes the semantics of the reporting and claiming time windows in
Revora-Contracts, including boundary inclusivity, zero-width windows, reconfiguration
behaviour, and the relationship to the claim-delay mechanism.

Cross-reference: [core-event-version-field.md](./core-event-version-field.md) for
versioned event schemas emitted by `set_report_window` and `set_claim_window`.

---

## Soroban Time Model

Soroban exposes ledger time via `env.ledger().timestamp()`, which returns the Unix
timestamp (seconds since epoch) of the **current ledger's close time**. Key properties:

| Property | Detail |
|----------|--------|
| Unit | Seconds since Unix epoch (u64) |
| Source | Stellar network consensus — not manipulable per-transaction |
| Monotonicity | Non-decreasing across ledgers (protocol guarantee) |
| Test access | `env.ledger().with_mut(\|l\| l.timestamp = T)` |
| Precision | 1 second (no sub-second resolution) |

Integrators **must not** assume wall-clock time matches ledger time in real-time;
ledger close times can lag or batch. Always use `env.ledger().timestamp()` as the
authoritative time source within the contract.

---

## AccessWindow Structure

```rust
pub struct AccessWindow {
    pub start_timestamp: u64,
    pub end_timestamp: u64,
}
```

Stored per-offering in persistent storage under `WindowDataKey::Report(offering_id)`
or `WindowDataKey::Claim(offering_id)`.

### Boundary Check (inclusive on both ends)

```rust
fn is_window_open(env: &Env, window: &AccessWindow) -> bool {
    let now = env.ledger().timestamp();
    now >= window.start_timestamp && now <= window.end_timestamp
}
```

Both `start_timestamp` and `end_timestamp` are **inclusive**. A transaction whose
ledger closes at exactly `start_timestamp` or `end_timestamp` is permitted.

---

## Which Operations Are Window-Gated

| Operation | Report Window | Claim Window | Notes |
|-----------|:---:|:---:|-------|
| `report_revenue` | ✅ | — | Blocked by `ReportingWindowClosed` if outside window |
| `deposit_revenue` | — | — | **No window gate** — always permitted |
| `claim` | — | ✅ | Blocked by `ClaimWindowClosed` if outside window |

> **Important:** `deposit_revenue` has no time-window restriction. Issuers can deposit
> revenue at any time regardless of any configured window.

---

## Report Window Matrix

| Scenario | `now` vs `[start, end]` | Result |
|----------|------------------------|--------|
| No window configured | any | ✅ OK — always open |
| `now < start` | before | ❌ `ReportingWindowClosed` |
| `now == start` | at start (inclusive) | ✅ OK |
| `start < now < end` | inside | ✅ OK |
| `now == end` | at end (inclusive) | ✅ OK |
| `now > end` | after | ❌ `ReportingWindowClosed` |
| `start == end`, `now == start` | zero-width, exact match | ✅ OK |
| `start == end`, `now != start` | zero-width, no match | ❌ `ReportingWindowClosed` |
| Window reconfigured to exclude `now` | mid-flight | ❌ `ReportingWindowClosed` |
| Window reconfigured to include `now` | mid-flight | ✅ OK |

---

## Claim Window Matrix

| Scenario | `now` vs `[start, end]` | Result |
|----------|------------------------|--------|
| No window configured | any | ✅ OK — always open |
| `now < start` | before | ❌ `ClaimWindowClosed` |
| `now == start` | at start (inclusive) | ✅ OK |
| `start < now < end` | inside | ✅ OK |
| `now == end` | at end (inclusive) | ✅ OK |
| `now > end` | after | ❌ `ClaimWindowClosed` |
| `start == end`, `now == start` | zero-width, exact match | ✅ OK |
| `start == end`, `now != start` | zero-width, no match | ❌ `ClaimWindowClosed` |
| Window reconfigured to exclude `now` | mid-flight | ❌ `ClaimWindowClosed` |
| Window reconfigured to include `now` | mid-flight | ✅ OK |

---

## Window Validation (set_report_window / set_claim_window)

| `start` vs `end` | Result |
|-----------------|--------|
| `start < end` | ✅ Accepted |
| `start == end` | ✅ Accepted (zero-width / single-second window) |
| `start > end` | ❌ `LimitReached` — no storage write occurs |

---

## Claim Delay vs Claim Window (Orthogonal Mechanisms)

The per-offering `ClaimDelaySecs` and the claim window are **independent** checks:

```
claim() execution order:
  1. require_claim_window_open()   ← window check (ClaimWindowClosed if fails)
  2. for each period:
       if delay_secs > 0 && now < deposit_time + delay_secs: break
       ← delay check per period (ClaimDelayNotElapsed if all periods blocked)
```

| Claim Window | Delay Elapsed | Result |
|:---:|:---:|--------|
| Open | Yes | ✅ Claim succeeds |
| Open | No | ❌ `ClaimDelayNotElapsed` |
| Closed | Yes | ❌ `ClaimWindowClosed` (window checked first) |
| Closed | No | ❌ `ClaimWindowClosed` (window checked first) |

---

## Reconfiguration Mid-Flight

An issuer can call `set_report_window` or `set_claim_window` at any time. The contract
applies the window that is **active at the ledger that closes the transaction** — there
is no snapshot of the window at submission time.

**Scenario:** Holder submits a `claim` transaction at T=2000 while the window is
`[1000, 3000]`. Before the transaction closes, the issuer submits `set_claim_window`
with `[4000, 5000]`. If the issuer's reconfiguration closes first, the holder's claim
will fail with `ClaimWindowClosed`.

This is a known race condition. Integrators should:
- Use sufficiently wide windows to reduce race probability.
- Monitor `rep_win` / `clm_win` events for window changes.
- Retry failed claims after verifying the current window.

---

## Zero-Width Windows

A window where `start_timestamp == end_timestamp` is valid and creates a
**single-second eligibility slot**. This is intentional but operationally fragile:

- Only transactions whose ledger closes at exactly that second are permitted.
- Stellar ledger close times are approximately every 5 seconds; hitting a specific
  second is not guaranteed.
- **Recommendation:** Use zero-width windows only for testing. Production windows
  should have a meaningful duration (e.g., ≥ 3600 seconds).

---

## Security / Risk Notes

1. **No deposit window**: `deposit_revenue` is never time-gated. An issuer can deposit
   revenue outside any reporting or claiming window. This is intentional — deposits
   fund future claims and should not be blocked by operational windows.

2. **Reconfiguration race**: Window changes take effect at the ledger they are included
   in. In-flight transactions see the window active at their closing ledger, not at
   submission time. See "Reconfiguration Mid-Flight" above.

3. **Zero-width window fragility**: Single-second windows are valid but unreliable in
   production. Prefer windows with meaningful duration.

4. **Timestamp source**: Only `env.ledger().timestamp()` is used. Wall-clock time,
   block numbers, and sequence numbers are not used for window checks.

5. **Window isolation**: Report and claim windows are scoped per-offering
   `(issuer, namespace, token)`. A window on offering A has no effect on offering B.

6. **No automatic expiry**: Windows do not self-delete after `end_timestamp` passes.
   The stored window remains in persistent storage; it simply evaluates as closed.
   Issuers can reconfigure or effectively remove a window by setting a new one.

---

## Test Coverage

All semantics above are asserted in `src/test_time_windows.rs`:

| Test | Covers |
|------|--------|
| `report_window_unset_always_open` | No window → always open |
| `report_window_before_start_is_closed` | `now < start` |
| `report_window_at_start_is_open_inclusive` | `now == start` (inclusive) |
| `report_window_inside_is_open` | `start < now < end` |
| `report_window_at_end_is_open_inclusive` | `now == end` (inclusive) |
| `report_window_after_end_is_closed` | `now > end` |
| `report_window_zero_width_open_at_exact_timestamp` | Zero-width, exact match |
| `report_window_zero_width_closed_before` | Zero-width, before |
| `report_window_zero_width_closed_after` | Zero-width, after |
| `report_window_reconfigured_to_exclude_now_closes_reporting` | Mid-flight close |
| `report_window_reconfigured_to_include_now_opens_reporting` | Mid-flight open |
| `claim_window_unset_always_open` | No window → always open |
| `claim_window_before_start_is_closed` | `now < start` |
| `claim_window_at_start_is_open_inclusive` | `now == start` (inclusive) |
| `claim_window_inside_is_open` | `start < now < end` |
| `claim_window_at_end_is_open_inclusive` | `now == end` (inclusive) |
| `claim_window_after_end_is_closed` | `now > end` |
| `claim_window_zero_width_open_at_exact_timestamp` | Zero-width, exact match |
| `claim_window_zero_width_closed_before` | Zero-width, before |
| `claim_window_zero_width_closed_after` | Zero-width, after |
| `claim_window_reconfigured_to_exclude_now_closes_claiming` | Mid-flight close |
| `claim_window_reconfigured_to_include_now_opens_claiming` | Mid-flight open |
| `set_report_window_valid_range_accepted` | Validation: start < end |
| `set_report_window_zero_width_accepted` | Validation: start == end |
| `set_report_window_inverted_range_rejected` | Validation: start > end |
| `set_claim_window_valid_range_accepted` | Validation: start < end |
| `set_claim_window_zero_width_accepted` | Validation: start == end |
| `set_claim_window_inverted_range_rejected` | Validation: start > end |
| `deposit_revenue_ignores_report_and_claim_windows` | No deposit window gate |
| `claim_window_open_but_delay_not_elapsed_returns_delay_error` | Delay orthogonal |
| `claim_window_open_and_delay_elapsed_succeeds` | Both pass |
| `claim_window_closed_even_if_delay_elapsed` | Window checked first |
| `report_window_is_scoped_per_offering` | Per-offering isolation |
| `claim_window_is_scoped_per_offering` | Per-offering isolation |
| `set_report_window_emits_event` | Event emission |
| `set_claim_window_emits_event` | Event emission |
| `get_report_window_returns_none_when_unset` | Read-back: unset |
| `get_claim_window_returns_none_when_unset` | Read-back: unset |
| `get_report_window_returns_correct_values` | Read-back: values |
| `get_claim_window_returns_correct_values` | Read-back: values |
| `set_report_window_overwrites_previous` | Reconfiguration replaces |
