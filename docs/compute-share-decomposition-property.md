# `compute_share` Decomposition Identity — Formal Property Test

**Issue:** #411  
**File:** `src/test_compute_share_decomposition_prop.rs`  
**Strategy helper:** `proptest_helpers::arb_fuzz_decomposition_amount`

---

## Overview

`compute_share(amount, bps, Truncation)` decomposes the payout calculation as:

```
result = (amount / 10_000) * bps + (amount % 10_000) * bps / 10_000
       = base                    + remainder_share
```

where:
- `base = (amount / 10_000) * bps` — the quotient contribution
- `remainder_share = (amount % 10_000) * bps / 10_000` — the sub-10 000 leftover contribution

This decomposition avoids the `amount * bps` overflow that would occur for large `i128` values
(e.g. `i128::MAX * 10_000` overflows). The property test in this module locks the algebraic
identity so any refactor that silently changes the arithmetic is caught before audit.

---

## Algebraic Identity

For all `amount ∈ [i128::MIN/2, i128::MAX/2]` and `bps ∈ [0, 10_000]`:

```
compute_share(amount, bps, Truncation)
    == (amount / 10_000) * bps + (amount % 10_000) * bps / 10_000
```

Both sides use the same overflow-safe arithmetic (checked_mul with sign-aware saturation),
so the identity holds even when intermediate saturation fires.

---

## Fuzz Space

| Parameter | Range                         | Rationale                                                                 |
|-----------|-------------------------------|---------------------------------------------------------------------------|
| `amount`  | `i128::MIN/2 ..= i128::MAX/2` | Avoids saturation noise in the reference formula; still covers ±10^37.   |
| `bps`     | `0 ..= 10_000`                | Full valid range. Values > 10_000 are covered by the over-bps guard test. |

The fuzz space runs **1 000 proptest cases** per property, which is safe for CI without a
feature flag and completes in well under 1 second on typical hardware.

---

## Properties Tested

### 1. `prop_decomposition_identity_truncation`

The core property. Asserts that `compute_share(amount, bps, Truncation)` equals the
reference decomposition for all `(amount, bps)` in the fuzz space.

Also asserts the **clamp invariant**: `result ∈ [min(0, amount), max(0, amount)]`.

### 2. `prop_negative_amount_clamp`

For all negative `amount` and valid `bps`:
- `result ≤ 0`
- `result ≥ amount`

This locks the "clamp at extremes" requirement: a negative-amount input can never produce
a result more negative than the input itself.

### 3. `prop_over_bps_guard`

For all `bps > 10_000` and any `amount`, `compute_share` returns `0`.

Verifies the guard is not accidentally removed by a refactor.

---

## Boundary Seeds

In addition to the fuzz properties, deterministic unit tests cover the four corners of the
fuzz space and other critical boundary values:

| Test                                  | What it covers                                              |
|---------------------------------------|-------------------------------------------------------------|
| `boundary_seeds_decomposition_identity` | Corners of fuzz space, zero identity, ±1, ±10_000, ±10_001 |
| `boundary_seeds_over_bps_guard`       | `bps ∈ {10_001, 20_000, u32::MAX}` at boundary amounts     |
| `boundary_seeds_full_share_identity`  | `bps = 10_000 → result = amount` at boundary amounts       |
| `boundary_seeds_zero_identity`        | `amount = 0` or `bps = 0 → result = 0`                     |

---

## Security Rationale

`compute_share` is called on every claim payout path. A regression in the decomposition
arithmetic could allow:

- **Over-distribution**: a holder claiming more than their entitled share, draining the contract.
- **Silent overflow**: a refactor switching to `amount * bps / 10_000` would overflow for
  `amount > i128::MAX / 10_000 ≈ 1.7 × 10^34`, producing a wrong (possibly negative) result.

The property test catches both classes of regression before audit by locking the identity
across the full bounded fuzz space.

The clamp at the end of `compute_share` is the last line of defence; the property test
verifies it holds for all inputs in the fuzz space.

---

## Running the Tests

```sh
# Run all compute_share decomposition property tests
cargo test test_compute_share_decomposition_prop

# Run with verbose output to see proptest statistics
cargo test test_compute_share_decomposition_prop -- --nocapture

# Run the full test suite
cargo test --all
```

---

## Related Files

- `src/lib.rs` — `compute_share` implementation (search for `fn compute_share`)
- `src/proptest_helpers.rs` — `arb_fuzz_decomposition_amount` strategy
- `src/test_compute_share_invariants.rs` — bounds, overflow, and rounding invariants [RC26Q2-C02]
- `docs/compute-share-overflow-protection.md` — overflow protection design notes
