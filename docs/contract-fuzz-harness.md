# Revora Fuzzing & Stress Harness

This document describes the automated stress testing story for the Revora-Contracts suite.

## Execution
- **Standard Unit Tests:** `cargo test`
- **Harden/Stress Tests (CI-Safe):** `cargo test --features stress-tests`

## Security Invariants
1. **Loop Bounding:** Input parameters like `period_id` are gated (max 1000) to prevent CPU instruction exhaustion.
2. **Panic-Free Execution:** The harness verifies that edge-case inputs return `Result::Err` rather than panicking.
3. **Deterministic Seeds:** Fixed seeds are used in CI to ensure reproducibility of failures.
The harness covers three layers:

1. Deterministic boundary sweeps — explicit `[i128::MIN, -1, 0, 1, i128::MAX]` inputs exercised in unit tests.
2. Property-based tests — `proptest!` blocks that generate thousands of random inputs and assert invariants hold for all of them.
3. Sequence-based tests — random operation sequences that verify multi-step invariants (period ordering, blacklist isolation, idempotency).

---

## Security Assumptions

The following assumptions are encoded as explicit test assertions. Any regression that breaks them is a security issue.

| Assumption | Enforced by |
|---|---|
| Negative amounts are rejected in all deposit contexts | `fuzz_deposit_revenue_rejects_negative_amounts`, `prop_negative_amount_always_rejected` |
| Zero amounts are rejected in deposit_revenue | `fuzz_deposit_revenue_rejects_zero` |
| Zero amounts are rejected in report_revenue | `fuzz_report_revenue_rejects_zero` |
| `period_id == 0` is always rejected | `fuzz_report_revenue_rejects_period_zero`, `prop_period_zero_always_rejected` |
| BPS > 10 000 is always rejected | `fuzz_register_offering_rejects_invalid_bps`, `prop_invalid_bps_always_rejected` |
| Frozen contract blocks all state mutations | `fuzz_frozen_blocks_report_revenue`, `fuzz_frozen_blocks_register_offering` |
| Blacklisted holder cannot claim | `fuzz_blacklisted_holder_claim_rejected` |
| Blacklist is per-offering (namespace-isolated) | `fuzz_blacklist_isolation_across_offerings` |
| Period IDs must be strictly increasing | `fuzz_strictly_increasing_periods_accepted`, `prop_operation_sequence_period_ordering` |
| Concentration limit blocks report when enforced | `fuzz_concentration_limit_enforced` |
| Concentration limit is a no-op when `enforce=false` | `fuzz_concentration_limit_not_enforced_when_disabled` |
| Blacklist add/remove is idempotent | `prop_blacklist_idempotent` |

---

## Abuse / Failure Paths

Each path below is explicitly tested:

- `i128::MIN` and `i128::MIN + 1` as deposit/report amounts → `InvalidAmount`
- `-1` as deposit/report amount → `InvalidAmount`
- `0` as deposit amount → `InvalidAmount`
- `0` as report amount → accepted audit entry (no transfer), still subject to threshold/override rules
- `period_id == 0` → `InvalidPeriodId`
- `bps = 10_001`, `bps = u32::MAX` → `InvalidRevenueShareBps`
- `share_bps = 10_001` → `InvalidShareBps`
- Duplicate report period without `override_existing=true` → `rev_rej` event, no state change
- `report_revenue` on non-existent offering → `OfferingNotFound`
- Claim by blacklisted holder → `HolderBlacklisted`
- Any mutation while frozen → `ContractFrozen`
- Concentration > enforced limit → `ConcentrationLimitExceeded`

---

## Strategy Reference (`src/proptest_helpers.rs`)

| Strategy | Description |
|---|---|
| `arb_valid_bps()` | `0u32..=10_000` |
| `arb_invalid_bps()` | `10_001u32..=u32::MAX` |
| `any_positive_amount()` | `1i128..=100_000_000` |
| `arb_non_negative_amount()` | `0i128..=100_000_000` |
| `arb_negative_amount()` | `i128::MIN..=-1` |
| `arb_boundary_amount()` | `{MIN, MIN+1, -1, 0, 1, MAX-1, MAX}` |
| `arb_positive_period_id()` | `1u64..=u64::MAX` |
| `arb_boundary_period_id()` | `{0, 1, 2, MAX-1, MAX}` |
| `arb_strictly_increasing_periods(n)` | Vector of `n` strictly increasing u64 IDs |
| `arb_valid_operation_sequence(n)` | `n` operations with normalized period ordering |

---

## Running the Harness

```bash
# Run all fuzz harness tests
cargo test fuzz_harness -- --nocapture

# Run a specific property test
cargo test prop_negative_amount_always_rejected -- --nocapture

# Run with more proptest cases (override default 256)
PROPTEST_CASES=1000 cargo test fuzz_harness
```

---

## Adding New Fuzz Cases

1. Add a strategy to `src/proptest_helpers.rs` if a new input shape is needed.
2. Add a unit test or `proptest!` block inside `mod fuzz_harness` in `src/test.rs`.
3. Document the security assumption in the table above.
4. Run `cargo test fuzz_harness` and include output in the PR.

---

## Determinism

All LCG-based sweeps use a fixed seed (`0xDEAD_BEEF_1234_5678`). The `prop_deterministic_lcg_sweep_reproducible` test verifies that two runs with the same seed produce identical accept/reject counts. This ensures CI failures are reproducible locally.

---

## Notes

- Auth is mocked via `env.mock_all_auths()` in all fuzz tests. Auth boundary tests live in `src/test_auth.rs`.
- Token transfers in `deposit_revenue` require a real token contract. Fuzz tests that hit the transfer path may fail with a transfer error rather than a validation error — this is expected and explicitly handled in `fuzz_deposit_revenue_accepts_max_i128`.
- The `TestOperation` enum in `proptest_helpers.rs` encodes parameters as primitive types (not `Address`) to avoid Soroban SDK lifetime constraints in strategy composition.
