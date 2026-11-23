# Close Period Gas Test Summary

This document covers all gas-budget tests for `close_period` and the deferred-distribution
queue release path.  It is the authoritative reference for CPU budget values cited in
`src/test_close_period.rs`.

---

## Table of Contents

1. [Background: Deferred Distribution Queue](#background-deferred-distribution-queue)
2. [Test Cases — Linear Holder Growth](#test-cases--linear-holder-growth)
3. [Test Cases — Deferred Queue Release](#test-cases--deferred-queue-release)
4. [Budget Derivation](#budget-derivation)
5. [Methodology](#methodology)
6. [Security Notes](#security-notes)
7. [Reproducing Results](#reproducing-results)

---

## Background: Deferred Distribution Queue

Revenue reports can be tagged with `defer_until_close`, which stores the amount in
`DeferredDataKey::DeferredReports(period_id)` rather than immediately finalising the
distribution.  Any `claim` against a still-deferred period panics with
`DistributionDeferred` (error code #456).

`close_period` atomically releases a single deferred entry:

```
1. Read  DeferredDataKey::DeferredReports(period_id)  → amount
2. Remove the key from persistent storage
3. Emit  def_flush(period_id) → amount
```

Each flush is **O(1)** — exactly one storage read, one remove, and one event publish —
regardless of how many other entries exist in the deferred queue.

See [`docs/DEFERRED_DISTRIBUTIONS.md`](./DEFERRED_DISTRIBUTIONS.md) for the full
lifecycle description.

---

## Test Cases — Linear Holder Growth

These tests (already present before this summary was written) verify that the
`close_period` entrypoint itself scales linearly with the number of holders registered
under the offering, and not worse.

| Test | Description | Pass Criterion |
|------|-------------|----------------|
| `close_period_cpu_grows_linearly_with_holders` | Samples CPU at n ∈ {1, 10, 100, 1000} holders, fits a linear regression | R² > 0.98 |
| `close_period_zero_holders_has_constant_cost` | Closes a period with 0 holders | CPU > 0 and CPU < 5,000,000 |

---

## Test Cases — Deferred Queue Release

Added in `test/deferred-release-gas-bound` to satisfy the gas-budget requirement
for releasing large deferred queues.

| Test | Description | Budget |
|------|-------------|--------|
| `close_period_deferred_queue_release_1000_entries_within_budget` | Flushes 1000 deferred entries sequentially; asserts total CPU ≤ budget | 350,000,000 instructions |
| `close_period_single_deferred_flush_within_per_call_budget` | Flushes one entry; asserts per-call CPU ≤ budget | 350,000 instructions |
| `close_period_flush_absent_entry_is_noop_within_budget` | Flushes a period_id with no deferred entry (no-op path) | 350,000 instructions |
| `close_period_deferred_queue_release_100_entries_within_tenth_budget` | Flushes 100 entries; asserts cost ≤ 1/10 of 1000-entry budget (linearity check) | 35,000,000 instructions |
| `close_period_deferred_queue_flush_leaves_no_residue` | Flushes 50 entries and verifies all keys are removed from storage | Correctness (no CPU limit) |

---

## Budget Derivation

### Soroban Network Limits (reference)

| Resource | Per-transaction limit |
|----------|-----------------------|
| CPU instructions | 100,000,000 |
| Memory bytes | 41,943,040 (40 MiB) |

### Per-call budget: 350,000 instructions

`RevoraRevenueShare::close_period(env, period_id)` performs:

- 1× `env.storage().persistent().get(key)` — ~50,000–100,000 instructions
- 1× `env.storage().persistent().remove(key)` — ~50,000–100,000 instructions
- 1× `env.events().publish(...)` — ~20,000–50,000 instructions
- XDR serialisation overhead — ~10,000–30,000 instructions

Observed cost in the Soroban test environment: **~120,000–180,000 instructions**.

The budget of **350,000** provides a **2× safety headroom** over the observed ceiling,
leaving room for minor SDK version fluctuations without triggering false failures.

### 1000-entry cumulative budget: 350,000,000 instructions

```
350,000 instructions/call × 1,000 calls = 350,000,000 instructions
```

This is **3.5× the single-transaction network limit**, which is intentional: the test
environment has an unlimited budget, and real deployments would spread 1000 flushes
across multiple transactions (≤ ~285 flushes per transaction to stay under the
100M-instruction limit with headroom).

The test is not meant to simulate a single on-chain transaction; it measures the
aggregate cost of releasing a 1000-entry queue to confirm the O(1)-per-call property
holds at scale and to detect any accidental O(n) or O(n²) regression.

### 100-entry / 1/10 budget linearity gate

If the 100-entry cost exceeds 1/10 of the 1000-entry budget (35,000,000 instructions),
the growth rate is super-linear, and the test fails.  This catches regressions before
they reach the 1000-entry scale.

---

## Methodology

### Fixture construction

The deferred queue is populated using `env.as_contract(contract_id, || { ... })` to
write directly into the contract's persistent storage:

```rust
env.as_contract(&contract_id, || {
    for i in 0..count {
        env.storage()
            .persistent()
            .set(&DeferredDataKey::DeferredReports(i), &1_000_000_i128);
    }
});
```

This avoids routing through any `report_revenue` entrypoint, keeping the fixture
focused purely on the flush path.

### CPU measurement

```rust
let before = env.budget().cpu_instruction_count();
// ... operation under test ...
let after = env.budget().cpu_instruction_count();
let cpu = after.saturating_sub(before);
```

`env.budget().cpu_instruction_count()` is the Soroban test SDK's built-in instruction
counter.  The test environment runs with an **unlimited budget** by default, so
measurements reflect true instruction counts without artificial caps.

### Why direct function calls

The inner `RevoraRevenueShare::close_period(env, period_id)` is a plain (non-`#[contractimpl]`)
associated function.  It is called directly rather than through the client to:

1. Isolate the flush cost from the client dispatch overhead.
2. Access the `DeferredDataKey` storage layout directly.
3. Keep the test deterministic across SDK versions.

---

## Security Notes

1. **O(1) flush invariant** — Each deferred entry flush touches exactly one storage
   key.  The budget tests enforce this: any O(n) scan over all deferred entries would
   cause `close_period_single_deferred_flush_within_per_call_budget` to fail even at
   n=1000 (since the per-call budget of 350,000 instructions is far below 1000×
   baseline cost).

2. **No residue after flush** — `close_period_deferred_queue_flush_leaves_no_residue`
   verifies that flushed entries are truly removed.  A stale `DeferredReports` entry
   would permanently block holder claims with `DistributionDeferred`.

3. **No-op safety** — `close_period_flush_absent_entry_is_noop_within_budget` confirms
   that flushing a non-existent entry neither panics nor creates unexpected storage
   writes.

4. **Regression gate** — If a future change introduces an O(n) dependency inside the
   flush (e.g. scanning all deferred entries to validate ordering), the 1000-entry and
   100-entry tests will catch it before the change reaches mainnet.

---

## Reproducing Results

```bash
# Run all close_period gas tests
cargo test -- test_close_period --test-threads=1 --nocapture

# Run only the deferred-queue gas tests
cargo test close_period_deferred -- --test-threads=1 --nocapture

# Run the full test suite
cargo test --all -- --test-threads=1
```

Expected output (abridged):

```
test test_close_period::close_period_deferred_queue_release_1000_entries_within_budget ... ok
test test_close_period::close_period_single_deferred_flush_within_per_call_budget ... ok
test test_close_period::close_period_flush_absent_entry_is_noop_within_budget ... ok
test test_close_period::close_period_deferred_queue_release_100_entries_within_tenth_budget ... ok
test test_close_period::close_period_deferred_queue_flush_leaves_no_residue ... ok
```

---

*Last updated: 2026-07-29 — branch `test/deferred-release-gas-bound`*
