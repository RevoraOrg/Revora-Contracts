# Test Coverage Analysis — `compute_share` Kani Bounded Verification [Issue #465]

## Scope

Formal bounded verification of `compute_share` rounding modes via Kani, complementing the
existing table-driven and proptest suites in `src/test_compute_share_invariants.rs` and
`src/test_compute_share_decomposition_prop.rs`.

## Verified Domain

| Parameter | Range | Rationale |
| --------- | ----- | --------- |
| `amount`  | `[-2^32, 2^32]` | Exhaustive symbolic coverage without i128 product overflow |
| `bps`     | `[0, 10_000]` | Full valid basis-point range |

`i128::MIN` is **outside** the Kani domain (naive `amount * bps` overflows) and is covered by
unit tests that document the panic on the naive path and the correct decomposition result.

## Core Invariant (Tolerance Form)

Within the bounded domain, for both `Truncation` and `RoundHalfUp`:

```text
result * 10_000 + rounding_dust == amount * bps
|rounding_dust| < 10_000   (when amount ≠ 0 and bps ≠ 0)
```

where:

- `result` = `compute_share(amount, bps, mode)`
- `rounding_dust` = `amount * bps - result * 10_000`

This encodes the exact integer identity between the rounded share and the true bps-scaled
product, with sub-unit residue bounded by one bps denominator.

## Kani Proofs

| Proof | Invariant |
| ----- | --------- |
| `truncation_dust_invariant` | Dust identity + tolerance for `Truncation` |
| `round_half_up_dust_invariant` | Dust identity + tolerance for `RoundHalfUp` |
| `bounds_invariant_both_modes` | `result ∈ [min(0, amount), max(0, amount)]` |
| `round_half_up_gte_truncation_for_positive_amounts` | `RoundHalfUp ≥ Truncation` for `amount > 0` |
| `full_bps_returns_amount` | `bps == 10_000 → result == amount` |

Implementation: `src/kani_harness/compute_share.rs` (pure mirror of `RevoraRevenueShare::compute_share`).

## Unit Tests (Default CI)

| Test | Purpose |
| ---- | ------- |
| `i128_min_naive_multiply_overflow_is_detected` | `checked_mul` fails for `i128::MIN * 10_000` |
| `i128_min_naive_multiply_documented_panic` | Panics on naive path, not silent wraparound |
| `i128_min_full_bps_decomposition_is_exact_not_wrapped` | Decomposition returns exact `i128::MIN` |

## Security Notes

1. **No silent wraparound:** `i128::MIN * 10_000` overflows `i128`. The contract uses
   quotient/remainder decomposition; naive multiply must never be used on the payout path.
2. **Bounded proof ≠ full i128 range:** Kani covers `±2^32`; extreme i128 table tests in
   `test_compute_share_invariants.rs` cover saturation and clamp behavior at `i128::MAX/MIN`.
3. **Feature gate:** The Kani harness is behind `--features kani` so default CI (`cargo test`,
   `cargo clippy --all-features`) is not delayed by model checking.

## How to Run

### Default CI (no Kani)

```bash
cargo test --all
cargo test test_compute_share_invariants -- --test-threads=1
```

### Kani bounded verification (optional, local or dedicated job)

Install [Kani](https://model-checking.github.io/kani/), then:

```bash
cargo kani --features kani
```

Run a single proof:

```bash
cargo kani --features kani -h truncation_dust_invariant
```

Expected: all five proofs pass with no counterexamples.

## Coverage Estimate

| Layer | Coverage |
| ----- | -------- |
| Bounded dust identity (`±2^32` × `[0, 10_000]`) | **100%** (exhaustive via Kani) |
| `i128::MIN` naive overflow | **100%** (unit tests) |
| Extreme i128 / table cases | Covered by existing `test_compute_share_invariants.rs` |

**Meets requirement:** ≥ 95% coverage for the rounding invariant within the specified bounded domain.

## Comparison to Existing Tests

| Suite | Method | Domain |
| ----- | ------ | ------ |
| `test_compute_share_invariants.rs` | Table + spot checks | i128 extremes, half-unit boundaries |
| `test_compute_share_decomposition_prop.rs` | Proptest (1000 cases) | `amount ∈ [MIN/2, MAX/2]` |
| `kani_harness/compute_share.rs` | Kani (exhaustive) | `amount ∈ [-2^32, 2^32]` |

Kani fills the gap between spot-checked extremes and sampled proptest coverage for half-value
and negative-input rounding behavior in a reviewable, bounded domain.
