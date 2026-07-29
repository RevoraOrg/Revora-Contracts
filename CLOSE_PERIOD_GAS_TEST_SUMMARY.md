# Close Period Gas Test Summary

## Purpose

This document describes the gas-bound tests for the `close_period` function, covering
two distinct cost dimensions:

1. **Linear holder growth** — the `close_period` entrypoint must remain O(holders).
2. **Deferred queue release** — the internal `DeferredReports` flush must remain O(1)
   per entry regardless of total queue depth.

For the full deferred-queue budget derivation, methodology, and expected output see
[`docs/CLOSE_PERIOD_GAS_TEST_SUMMARY.md`](./docs/CLOSE_PERIOD_GAS_TEST_SUMMARY.md).

---

## Test Cases — Linear Holder Growth

1. **`close_period_cpu_grows_linearly_with_holders`**:
   - Parameterized over holder counts [1, 10, 100, 1000]
   - Measures CPU instructions consumed per call
   - Fits a linear regression line and verifies R² (coefficient of determination) > 0.98
   - Ensures cost grows linearly with number of holders

2. **`close_period_zero_holders_has_constant_cost`**:
   - Tests closing a period with 0 holders
   - Verifies cost is positive but bounded by a constant (<5,000,000 instructions)

## Linearity Check

Uses coefficient of determination (R²) to measure how well the data fits a linear model:
- R² > 0.98 means the data is well explained by a linear relationship
- Calculates slope, intercept, and residual sum of squares
- Handles edge cases like zero variance in y-values

---

## Test Cases — Deferred Queue Release (added: `test/deferred-release-gas-bound`)

| Test | Budget |
|------|--------|
| `close_period_deferred_queue_release_1000_entries_within_budget` | ≤ 350,000,000 instructions |
| `close_period_single_deferred_flush_within_per_call_budget` | ≤ 350,000 instructions |
| `close_period_flush_absent_entry_is_noop_within_budget` | ≤ 350,000 instructions |
| `close_period_deferred_queue_release_100_entries_within_tenth_budget` | ≤ 35,000,000 instructions |
| `close_period_deferred_queue_flush_leaves_no_residue` | Correctness (storage clean) |

**Budget constants** (defined in `src/test_close_period.rs`):
- `DEFERRED_FLUSH_PER_CALL_CPU_BUDGET = 350_000`
- `DEFERRED_FLUSH_1000_ENTRIES_CPU_BUDGET = 350_000_000`

---

## Key Assumptions

- `close_period` contractimpl entrypoint has O(holders) cost (no quadratic paths).
- Internal `close_period(env, period_id)` flush is O(1) — independent of queue depth.
- If future modifications add holder iteration or cross-entry scans, these tests will
  catch O(n²) or worse regressions.
- Uses Soroban's built-in `env.budget().cpu_instruction_count()` for accurate measurements.

## Security Notes

- Linear/constant costs ensure scalability for offerings with many holders or large deferred queues.
- Prevents gas bombs from accidental quadratic loops.
- Tests are designed to fail fast if performance degrades.
