#![allow(unexpected_cfgs)]
//! Kani bounded verification for `compute_share` rounding invariants (Issue #465).
//!
//! ## Verified domain
//!
//! | Parameter | Range |
//! |-----------|-------|
//! | `amount`  | `[-2^32, 2^32]` |
//! | `bps`     | `[0, 10_000]` |
//!
//! ## Core invariant (tolerance form)
//!
//! Within the bounded domain, `amount * bps` fits in `i128` without overflow. For both
//! rounding modes:
//!
//! ```text
//! result * 10_000 + rounding_dust == amount * bps
//! |rounding_dust| < 10_000
//! ```
//!
//! where `rounding_dust = amount * bps - result * 10_000` captures the sub-unit residue
//! after the selected rounding mode is applied.
//!
//! ## Out-of-domain security note (`i128::MIN`)
//!
//! `i128::MIN * 10_000` overflows `i128` on the naive multiply path. The production
//! implementation uses quotient/remainder decomposition instead. See
//! `naive_product_or_panic` and `test_compute_share_invariants::i128_min_naive_multiply_documented_panic`.

/// Basis-point denominator used by `compute_share`.
pub const BPS_DENOM: i128 = 10_000;

/// Inclusive absolute bound for Kani symbolic `amount` (`2^32`).
pub const AMOUNT_ABS_BOUND: i128 = 1_i128 << 32;

/// Maximum valid basis points.
pub const MAX_BPS: u32 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundingMode {
    Truncation,
    RoundHalfUp,
}

/// Pure mirror of `RevoraRevenueShare::compute_share` (see `src/lib.rs`).
///
/// Intentionally omits `Env` so Kani can verify the arithmetic in isolation.
pub fn compute_share(amount: i128, revenue_share_bps: u32, mode: RoundingMode) -> i128 {
    if revenue_share_bps > MAX_BPS {
        return 0;
    }
    if amount == 0 || revenue_share_bps == 0 {
        return 0;
    }

    let q = amount / BPS_DENOM;
    let r = amount % BPS_DENOM;
    let bps = revenue_share_bps as i128;
    let base = q.checked_mul(bps).unwrap_or_else(|| {
        if (q >= 0 && bps >= 0) || (q < 0 && bps < 0) {
            i128::MAX
        } else {
            i128::MIN
        }
    });

    let remainder_product = r.checked_mul(bps).unwrap_or_else(|| {
        if (r >= 0 && bps >= 0) || (r < 0 && bps < 0) {
            i128::MAX
        } else {
            i128::MIN
        }
    });
    let remainder_share = match mode {
        RoundingMode::Truncation => remainder_product / BPS_DENOM,
        RoundingMode::RoundHalfUp => {
            let half = 5_000_i128;
            if remainder_product >= 0 {
                remainder_product.saturating_add(half) / BPS_DENOM
            } else {
                remainder_product.saturating_sub(half) / BPS_DENOM
            }
        }
    };

    let share = base.checked_add(remainder_share).unwrap_or_else(|| {
        if (base >= 0 && remainder_share >= 0) || (base < 0 && remainder_share < 0) {
            if base >= 0 {
                i128::MAX
            } else {
                i128::MIN
            }
        } else {
            0
        }
    });

    let lo = core::cmp::min(0, amount);
    let hi = core::cmp::max(0, amount);
    core::cmp::min(core::cmp::max(share, lo), hi)
}

/// Naive `amount * bps` reference used to document overflow hazards outside the bounded domain.
///
/// **Panics** when the product does not fit in `i128` (e.g. `amount == i128::MIN`, `bps == 10_000`).
/// Production code must never call this; use decomposition via `compute_share` instead.
pub fn naive_product_or_panic(amount: i128, bps: u32) -> i128 {
    amount
        .checked_mul(bps as i128)
        .expect("amount * bps overflow: decomposition path must be used instead")
}

/// Returns `(result, rounding_dust)` satisfying `result * BPS_DENOM + rounding_dust == product`.
pub fn share_and_dust(amount: i128, bps: u32, mode: RoundingMode) -> (i128, i128) {
    let product = amount * bps as i128;
    let result = compute_share(amount, bps, mode);
    let rounding_dust = product - result * BPS_DENOM;
    (result, rounding_dust)
}

// ── Cargo-test shims (concrete inputs; always run in CI without Kani) ─────────
//
// Every property verified by a `#[kani::proof]` above is mirrored here as a
// deterministic `#[test]` with concrete values.  This ensures regressions in
// the pure `compute_share` arithmetic are caught by ordinary `cargo test`
// without the Kani toolchain.
//
// Test categories:
//   1. Zero-identity   — `amount=0` or `bps=0` always returns 0.
//   2. Over-bps guard  — `bps > MAX_BPS` returns 0, not a scaled result.
//   3. Full-share      — `bps = MAX_BPS` returns `amount` exactly.
//   4. Truncation      — floor division cases and rounding-down boundary.
//   5. RoundHalfUp     — half-unit boundary and away-from-zero for negatives.
//   6. Bounds invariant — result ∈ [min(0, amount), max(0, amount)] for all cases.
//   7. i128 extremes   — MAX/MIN without overflow or wrapping.
//   8. naive_product_or_panic — documents and detects the overflow hazard.
//   9. share_and_dust  — helper satisfies `result * BPS_DENOM + dust == product`.
//  10. Rounding direction — RoundHalfUp ≥ Truncation for positive amounts;
//                           RoundHalfUp ≤ Truncation for negative amounts.

#[cfg(test)]
mod tests {
    use super::{
        compute_share, naive_product_or_panic, share_and_dust, RoundingMode, BPS_DENOM, MAX_BPS,
    };

    // ── Utility ───────────────────────────────────────────────────────────────

    /// Assert the bounds invariant: result ∈ [min(0, amount), max(0, amount)].
    fn assert_bounds(result: i128, amount: i128, label: &str) {
        let lo = core::cmp::min(0_i128, amount);
        let hi = core::cmp::max(0_i128, amount);
        assert!(
            result >= lo && result <= hi,
            "{label}: result {result} out of [{lo}, {hi}] for amount={amount}"
        );
    }

    // ── 1. Zero-identity ──────────────────────────────────────────────────────

    /// `amount = 0` always yields 0, regardless of `bps` or `mode`.
    #[test]
    fn zero_amount_returns_zero() {
        for bps in [0_u32, 1, 5_000, MAX_BPS, MAX_BPS + 1, u32::MAX] {
            assert_eq!(compute_share(0, bps, RoundingMode::Truncation), 0, "Truncation: bps={bps}");
            assert_eq!(
                compute_share(0, bps, RoundingMode::RoundHalfUp),
                0,
                "RoundHalfUp: bps={bps}"
            );
        }
    }

    /// `bps = 0` always yields 0, regardless of `amount` or `mode`.
    #[test]
    fn zero_bps_returns_zero() {
        for amount in [1_i128, -1, 10_000, -10_000, i128::MAX, i128::MIN] {
            assert_eq!(
                compute_share(amount, 0, RoundingMode::Truncation),
                0,
                "Truncation: amount={amount}"
            );
            assert_eq!(
                compute_share(amount, 0, RoundingMode::RoundHalfUp),
                0,
                "RoundHalfUp: amount={amount}"
            );
        }
    }

    // ── 2. Over-bps guard ─────────────────────────────────────────────────────

    /// `bps > MAX_BPS` returns 0 — the over-bps guard must fire before any math.
    #[test]
    fn over_bps_guard_returns_zero() {
        let bps_values = [MAX_BPS + 1, 20_000_u32, u32::MAX];
        let amounts = [1_i128, -1, 1_000_000, i128::MAX, i128::MIN];
        for bps in bps_values {
            for amount in amounts {
                assert_eq!(
                    compute_share(amount, bps, RoundingMode::Truncation),
                    0,
                    "Truncation: bps={bps} amount={amount}"
                );
                assert_eq!(
                    compute_share(amount, bps, RoundingMode::RoundHalfUp),
                    0,
                    "RoundHalfUp: bps={bps} amount={amount}"
                );
            }
        }
    }

    /// Boundary: exactly `MAX_BPS` is valid; `MAX_BPS + 1` triggers the guard.
    #[test]
    fn over_bps_boundary_is_exclusive() {
        let amount = 1_000_i128;
        // MAX_BPS is valid → result equals amount.
        assert_eq!(compute_share(amount, MAX_BPS, RoundingMode::Truncation), amount);
        // MAX_BPS + 1 is invalid → result is 0.
        assert_eq!(compute_share(amount, MAX_BPS + 1, RoundingMode::Truncation), 0);
    }

    // ── 3. Full-share identity ────────────────────────────────────────────────

    /// `bps = MAX_BPS` returns `amount` exactly for both modes and all amounts.
    #[test]
    fn full_bps_returns_amount() {
        let amounts = [1_i128, -1, 10_000, -10_000, 1_000_000, -1_000_000, i128::MAX, i128::MIN];
        for amount in amounts {
            assert_eq!(
                compute_share(amount, MAX_BPS, RoundingMode::Truncation),
                amount,
                "Truncation: amount={amount}"
            );
            assert_eq!(
                compute_share(amount, MAX_BPS, RoundingMode::RoundHalfUp),
                amount,
                "RoundHalfUp: amount={amount}"
            );
        }
    }

    // ── 4. Truncation (floor) cases ───────────────────────────────────────────

    /// Table-driven cases for `Truncation` mode.
    ///
    /// Formula: `floor(amount * bps / 10_000)` clamped to `[min(0,amount), max(0,amount)]`.
    #[test]
    fn truncation_table() {
        // (amount, bps, expected)
        let cases: &[(i128, u32, i128)] = &[
            // 50 % — exact
            (10_000, 5_000, 5_000),
            // 50 % — truncates: 10_001 * 5_000 / 10_000 = 5_000.5 → 5_000
            (10_001, 5_000, 5_000),
            // 1 at 50 % → 0.5 → 0
            (1, 5_000, 0),
            // Negative 50 %
            (-10_000, 5_000, -5_000),
            // 1 bps cases
            (10_000, 1, 1),
            (9_999, 1, 0), // 9_999 * 1 / 10_000 = 0.9999 → 0
            (1_000_000, 1, 100),
            // Typical revenue
            (100_000_000, 5_000, 50_000_000),
            (100_000_001, 5_000, 50_000_000), // truncates
        ];
        for &(amount, bps, expected) in cases {
            let result = compute_share(amount, bps, RoundingMode::Truncation);
            assert_eq!(
                result, expected,
                "Truncation: amount={amount}, bps={bps} → expected {expected}, got {result}"
            );
            assert_bounds(result, amount, "Truncation");
        }
    }

    // ── 5. RoundHalfUp cases ──────────────────────────────────────────────────

    /// Table-driven cases for `RoundHalfUp` mode.
    ///
    /// Rounding is "half away from zero": fractions exactly at 0.5 round up for
    /// positive amounts and down (more negative) for negative amounts.
    #[test]
    fn round_half_up_table() {
        // (amount, bps, expected)
        let cases: &[(i128, u32, i128)] = &[
            // 50 % — exact
            (10_000, 5_000, 5_000),
            // 10_001 * 5_000 / 10_000 = 5_000.5 → rounds up to 5_001
            (10_001, 5_000, 5_001),
            // 1 at 50 % → 0.5 → rounds up to 1
            (1, 5_000, 1),
            // -1 at 50 % → -0.5 → rounds away from zero → -1
            (-1, 5_000, -1),
            // -3 at 50 % → -1.5 → rounds to -2
            (-3, 5_000, -2),
            // 1 bps cases
            (10_000, 1, 1),
            (9_999, 1, 1), // 0.9999 rounds up to 1
            (4_999, 1, 0), // 0.4999 rounds down
            (5_000, 1, 1), // exactly 0.5 rounds up
        ];
        for &(amount, bps, expected) in cases {
            let result = compute_share(amount, bps, RoundingMode::RoundHalfUp);
            assert_eq!(
                result, expected,
                "RoundHalfUp: amount={amount}, bps={bps} → expected {expected}, got {result}"
            );
            assert_bounds(result, amount, "RoundHalfUp");
        }
    }

    /// Exact half-unit boundary: `amount=3, bps=5_000` → 1.5.
    /// Truncation → 1; RoundHalfUp → 2.
    #[test]
    fn rounding_boundary_exactly_half_positive() {
        assert_eq!(compute_share(3, 5_000, RoundingMode::Truncation), 1);
        assert_eq!(compute_share(3, 5_000, RoundingMode::RoundHalfUp), 2);
    }

    /// Exact half-unit boundary for negative: `amount=-3, bps=5_000` → -1.5.
    /// Truncation → -1; RoundHalfUp → -2 (away from zero).
    #[test]
    fn rounding_boundary_exactly_half_negative() {
        assert_eq!(compute_share(-3, 5_000, RoundingMode::Truncation), -1);
        assert_eq!(compute_share(-3, 5_000, RoundingMode::RoundHalfUp), -2);
    }

    // ── 6. Bounds invariant ───────────────────────────────────────────────────

    /// result ∈ [min(0, amount), max(0, amount)] over a representative grid.
    #[test]
    fn bounds_invariant_grid() {
        let amounts: &[i128] = &[
            0,
            1,
            -1,
            10_000,
            -10_000,
            100_000,
            -100_000,
            i128::MAX,
            i128::MIN,
            i128::MAX / 2,
            i128::MIN / 2,
            i128::MAX - 1,
            i128::MIN + 1,
        ];
        let bps_values: &[u32] = &[0, 1, 100, 5_000, 9_999, MAX_BPS, MAX_BPS + 1];

        for &amount in amounts {
            for &bps in bps_values {
                for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
                    let result = compute_share(amount, bps, mode);
                    assert_bounds(result, amount, &format!("{mode:?}: amount={amount}, bps={bps}"));
                }
            }
        }
    }

    // ── 7. i128 extremes ─────────────────────────────────────────────────────

    /// `i128::MAX` at 100 % must return `i128::MAX` exactly — no overflow.
    #[test]
    fn i128_max_full_share_no_overflow() {
        assert_eq!(compute_share(i128::MAX, MAX_BPS, RoundingMode::Truncation), i128::MAX);
        assert_eq!(compute_share(i128::MAX, MAX_BPS, RoundingMode::RoundHalfUp), i128::MAX);
    }

    /// `i128::MIN` at 100 % must return `i128::MIN` exactly — decomposition must not wrap.
    #[test]
    fn i128_min_full_share_no_overflow() {
        assert_eq!(compute_share(i128::MIN, MAX_BPS, RoundingMode::Truncation), i128::MIN);
        assert_eq!(compute_share(i128::MIN, MAX_BPS, RoundingMode::RoundHalfUp), i128::MIN);
    }

    /// `i128::MAX` at 1 bps remains positive and within bounds.
    #[test]
    fn i128_max_one_bps_in_bounds() {
        let t = compute_share(i128::MAX, 1, RoundingMode::Truncation);
        let r = compute_share(i128::MAX, 1, RoundingMode::RoundHalfUp);
        assert!(t > 0, "Truncation result must be positive");
        assert!(r > 0, "RoundHalfUp result must be positive");
        assert_bounds(t, i128::MAX, "Truncation i128::MAX 1bps");
        assert_bounds(r, i128::MAX, "RoundHalfUp i128::MAX 1bps");
    }

    /// `i128::MIN` at 1 bps remains negative and within bounds.
    #[test]
    fn i128_min_one_bps_in_bounds() {
        let t = compute_share(i128::MIN, 1, RoundingMode::Truncation);
        let r = compute_share(i128::MIN, 1, RoundingMode::RoundHalfUp);
        assert!(t < 0, "Truncation result must be negative");
        assert!(r < 0, "RoundHalfUp result must be negative");
        assert_bounds(t, i128::MIN, "Truncation i128::MIN 1bps");
        assert_bounds(r, i128::MIN, "RoundHalfUp i128::MIN 1bps");
    }

    /// `i128::MAX` at 50 % is within bounds for both modes; RoundHalfUp ≥ Truncation.
    #[test]
    fn i128_max_half_share_consistent() {
        let t = compute_share(i128::MAX, 5_000, RoundingMode::Truncation);
        let r = compute_share(i128::MAX, 5_000, RoundingMode::RoundHalfUp);
        assert_bounds(t, i128::MAX, "Truncation i128::MAX 50%");
        assert_bounds(r, i128::MAX, "RoundHalfUp i128::MAX 50%");
        assert!(r >= t, "RoundHalfUp must be >= Truncation for positive amount");
        // Truncation of i128::MAX / 2 — quotient-remainder decomposition must be exact.
        assert_eq!(t, i128::MAX / 2, "Truncation i128::MAX 50% must equal i128::MAX/2");
    }

    /// `i128::MIN` at 50 % is within bounds for both modes; RoundHalfUp ≤ Truncation.
    #[test]
    fn i128_min_half_share_consistent() {
        let t = compute_share(i128::MIN, 5_000, RoundingMode::Truncation);
        let r = compute_share(i128::MIN, 5_000, RoundingMode::RoundHalfUp);
        assert_bounds(t, i128::MIN, "Truncation i128::MIN 50%");
        assert_bounds(r, i128::MIN, "RoundHalfUp i128::MIN 50%");
        assert!(r <= t, "For negatives, RoundHalfUp must be <= Truncation (more negative)");
    }

    // ── 8. naive_product_or_panic — overflow hazard documentation ─────────────

    /// `i128::MIN * 10_000` does not fit in `i128`; `checked_mul` must return `None`.
    ///
    /// This documents the overflow hazard that the decomposition path avoids.
    #[test]
    fn i128_min_naive_multiply_overflows() {
        assert!(i128::MIN.checked_mul(10_000).is_none(), "i128::MIN * 10_000 must overflow i128");
    }

    /// `naive_product_or_panic` panics for `i128::MIN` at full bps — the
    /// production path must never use this naive approach.
    #[test]
    #[should_panic(expected = "amount * bps overflow: decomposition path must be used instead")]
    fn naive_product_panics_for_i128_min() {
        naive_product_or_panic(i128::MIN, MAX_BPS);
    }

    /// `naive_product_or_panic` succeeds within the bounded domain (`|amount| ≤ 2^32`).
    #[test]
    fn naive_product_succeeds_in_bounded_domain() {
        // 2^32 * 10_000 = 4_294_967_296 * 10_000 ≈ 4.3 × 10^13, well within i128.
        let bound: i128 = 1_i128 << 32;
        assert_eq!(naive_product_or_panic(bound, MAX_BPS), bound * MAX_BPS as i128);
        assert_eq!(naive_product_or_panic(-bound, MAX_BPS), -bound * MAX_BPS as i128);
    }

    // ── 9. share_and_dust helper ──────────────────────────────────────────────

    /// `result * BPS_DENOM + rounding_dust == amount * bps` within the bounded domain.
    #[test]
    fn share_and_dust_identity_bounded_domain() {
        // (amount, bps, mode)
        let cases: &[(i128, u32, RoundingMode)] = &[
            (0, 5_000, RoundingMode::Truncation),
            (1, 5_000, RoundingMode::Truncation),
            (1, 5_000, RoundingMode::RoundHalfUp),
            (10_001, 5_000, RoundingMode::Truncation),
            (10_001, 5_000, RoundingMode::RoundHalfUp),
            (-1, 5_000, RoundingMode::Truncation),
            (-1, 5_000, RoundingMode::RoundHalfUp),
            (9_999, 1, RoundingMode::Truncation),
            (9_999, 1, RoundingMode::RoundHalfUp),
            (1_000_000, MAX_BPS, RoundingMode::Truncation),
            (1_000_000, 0, RoundingMode::Truncation),
        ];
        for &(amount, bps, mode) in cases {
            let (result, dust) = share_and_dust(amount, bps, mode);
            let product = amount * bps as i128;
            assert_eq!(
                result * BPS_DENOM + dust,
                product,
                "share_and_dust: result*BPS_DENOM+dust must equal amount*bps for \
                 amount={amount}, bps={bps}, mode={mode:?}"
            );
            if amount != 0 && bps != 0 {
                assert!(
                    dust.abs() < BPS_DENOM,
                    "share_and_dust: |dust| must be < BPS_DENOM for \
                     amount={amount}, bps={bps}, mode={mode:?}; got dust={dust}"
                );
            }
        }
    }

    /// Zero inputs: `share_and_dust` returns `(0, 0)`.
    #[test]
    fn share_and_dust_zero_inputs() {
        assert_eq!(share_and_dust(0, 5_000, RoundingMode::Truncation), (0, 0));
        assert_eq!(share_and_dust(1_000, 0, RoundingMode::RoundHalfUp), (0, 0));
    }

    // ── 10. Rounding direction invariant ─────────────────────────────────────

    /// For positive `amount` and valid `bps > 0`: `RoundHalfUp ≥ Truncation`.
    #[test]
    fn rounding_direction_positive_amounts() {
        let amounts: &[i128] = &[1, 9_999, 10_000, 10_001, 1_000_000, i128::MAX / 2, i128::MAX];
        let bps_values: &[u32] = &[1, 100, 1_000, 3_333, 5_000, 7_500, 9_999, MAX_BPS];

        for &amount in amounts {
            for &bps in bps_values {
                let t = compute_share(amount, bps, RoundingMode::Truncation);
                let r = compute_share(amount, bps, RoundingMode::RoundHalfUp);
                assert!(
                    r >= t,
                    "RoundHalfUp ({r}) < Truncation ({t}) for amount={amount}, bps={bps}"
                );
            }
        }
    }

    /// For negative `amount` and valid `bps > 0`: `RoundHalfUp ≤ Truncation`
    /// (rounds away from zero, i.e. is more negative).
    #[test]
    fn rounding_direction_negative_amounts() {
        let amounts: &[i128] =
            &[-1, -9_999, -10_000, -10_001, -1_000_000, i128::MIN / 2, i128::MIN + 1];
        let bps_values: &[u32] = &[1, 100, 1_000, 5_000, 9_999, MAX_BPS];

        for &amount in amounts {
            for &bps in bps_values {
                let t = compute_share(amount, bps, RoundingMode::Truncation);
                let r = compute_share(amount, bps, RoundingMode::RoundHalfUp);
                assert!(
                    r <= t,
                    "For negatives, RoundHalfUp ({r}) > Truncation ({t}) \
                     for amount={amount}, bps={bps}"
                );
            }
        }
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;

    fn assume_bounded_inputs(amount: &mut i128, bps: &mut u32) {
        *amount = kani::any();
        *bps = kani::any();
        kani::assume(*amount >= -AMOUNT_ABS_BOUND && *amount <= AMOUNT_ABS_BOUND);
        kani::assume(*bps <= MAX_BPS);
    }

    /// `result * 10_000 + rounding_dust == amount * bps` with `|rounding_dust| < 10_000`.
    #[kani::proof]
    #[kani::unwind(4)]
    fn truncation_dust_invariant() {
        let mut amount = 0_i128;
        let mut bps = 0_u32;
        assume_bounded_inputs(&mut amount, &mut bps);

        let (result, rounding_dust) = share_and_dust(amount, bps, RoundingMode::Truncation);
        let product = amount * bps as i128;

        assert_eq!(result * BPS_DENOM + rounding_dust, product);
        if amount != 0 && bps != 0 {
            assert!(rounding_dust.abs() < BPS_DENOM);
        } else {
            assert_eq!(result, 0);
            assert_eq!(rounding_dust, 0);
        }
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn round_half_up_dust_invariant() {
        let mut amount = 0_i128;
        let mut bps = 0_u32;
        assume_bounded_inputs(&mut amount, &mut bps);

        let (result, rounding_dust) = share_and_dust(amount, bps, RoundingMode::RoundHalfUp);
        let product = amount * bps as i128;

        assert_eq!(result * BPS_DENOM + rounding_dust, product);
        if amount != 0 && bps != 0 {
            assert!(rounding_dust.abs() < BPS_DENOM);
        } else {
            assert_eq!(result, 0);
            assert_eq!(rounding_dust, 0);
        }
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn bounds_invariant_both_modes() {
        let mut amount = 0_i128;
        let mut bps = 0_u32;
        assume_bounded_inputs(&mut amount, &mut bps);

        for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
            let result = compute_share(amount, bps, mode);
            let lo = core::cmp::min(0, amount);
            let hi = core::cmp::max(0, amount);
            assert!(result >= lo && result <= hi);
        }
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn round_half_up_gte_truncation_for_positive_amounts() {
        let mut amount = 0_i128;
        let mut bps = 0_u32;
        assume_bounded_inputs(&mut amount, &mut bps);
        kani::assume(amount > 0);
        kani::assume(bps > 0);

        let trunc = compute_share(amount, bps, RoundingMode::Truncation);
        let round = compute_share(amount, bps, RoundingMode::RoundHalfUp);
        assert!(round >= trunc);
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn full_bps_returns_amount() {
        let mut amount = 0_i128;
        let mut bps = 0_u32;
        assume_bounded_inputs(&mut amount, &mut bps);
        kani::assume(amount != 0);
        kani::assume(bps == MAX_BPS);

        let trunc = compute_share(amount, bps, RoundingMode::Truncation);
        let round = compute_share(amount, bps, RoundingMode::RoundHalfUp);
        assert_eq!(trunc, amount);
        assert_eq!(round, amount);
    }
}
// ---------------------------------------------------------------------------
// Adversarial unit tests for the `share_and_dust` reconstruction contract
// (Issue #1053).
//
// The Kani harness above proves the invariant symbolically for
// `amount ∈ [-2^32, 2^32]` and `bps ∈ [0, 10_000]`. The proofs are only run in
// `cfg(kani)` builds, so the same contract has no coverage in a normal
// `cargo test` run. These tests pin the concrete edges the symbolic proof
// abstracts away:
//
// * short-circuit inputs (`amount == 0`, `bps == 0`) return `(0, 0)` rather
//   than a non-zero dust;
// * the invariant survives negative amounts and the half-unit rounding edge
//   (`RoundHalfUp` rounds away from zero at exactly 5_000 residue);
// * `result` is always clamped into `[min(0, amount), max(0, amount)]`;
// * `bps > MAX_BPS` is intentionally out of the invariant's domain: the share
//   collapses to `0` and the entire product becomes dust;
// * `i128::MIN` is handled by the decomposition path in `compute_share` but
//   *panics* on the naive `share_and_dust` multiply — the hazard the module
//   docs call out is pinned here as an executable expectation.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod share_and_dust_adversarial_tests {
    use super::*;

    /// Domain where `amount * bps` is guaranteed to fit in `i128` and therefore
    /// the invariant `result * BPS_DENOM + dust == amount * bps` must hold.
    fn assert_invariants(amount: i128, bps: u32, mode: RoundingMode) {
        let (result, dust) = share_and_dust(amount, bps, mode);
        let product = amount * bps as i128;

        // Exact reconstruction: no value is created or destroyed.
        assert_eq!(
            result * BPS_DENOM + dust,
            product,
            "reconstruction failed for amount={amount}, bps={bps}, mode={mode:?}"
        );

        // Result never leaves the [min(0, amount), max(0, amount)] band.
        let lo = core::cmp::min(0, amount);
        let hi = core::cmp::max(0, amount);
        assert!(
            result >= lo && result <= hi,
            "result {result} outside [{lo}, {hi}] for amount={amount}, bps={bps}"
        );

        // Sub-unit residue for in-domain, non-degenerate inputs.
        if amount != 0 && bps != 0 && bps <= MAX_BPS {
            assert!(
                dust.abs() < BPS_DENOM,
                "dust {dust} not bounded for amount={amount}, bps={bps}"
            );
        } else if amount == 0 || bps == 0 {
            assert_eq!(result, 0);
            assert_eq!(dust, 0);
        }
    }

    #[test]
    fn zero_amount_or_zero_bps_short_circuits() {
        for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
            assert_eq!(share_and_dust(0, 0, mode), (0, 0));
            assert_eq!(share_and_dust(0, MAX_BPS, mode), (0, 0));
            assert_eq!(share_and_dust(123_456, 0, mode), (0, 0));
            assert_eq!(share_and_dust(-123_456, 0, mode), (0, 0));
        }
    }

    #[test]
    fn truncation_edges_reconstruct_exactly() {
        let cases: [(i128, u32); 10] = [
            (1, 1),
            (9_999, 1),
            (10_000, 1),
            (999, 9_999),
            (9_999, 10_000),
            (1, 10_000),
            (-1, 1),
            (-9_999, 1),
            (-10_000, 1),
            (-9_999, 10_000),
        ];
        for (amount, bps) in cases {
            assert_invariants(amount, bps, RoundingMode::Truncation);
        }
    }

    #[test]
    fn round_half_up_edges_reconstruct_exactly() {
        let cases: [(i128, u32); 10] = [
            (5_000, 1),
            (4_999, 1),
            (9_999, 1),
            (1, 9_999),
            (1, 10_000),
            (9_999, 10_000),
            (-5_000, 1),
            (-4_999, 1),
            (-1, 9_999),
            (-9_999, 10_000),
        ];
        for (amount, bps) in cases {
            assert_invariants(amount, bps, RoundingMode::RoundHalfUp);
        }
    }

    #[test]
    fn half_unit_residue_rounds_away_from_zero() {
        // residue = 5_000 -> rounds up for positive, down (away from zero) for negative.
        assert_eq!(compute_share(5_000, 1, RoundingMode::RoundHalfUp), 1);
        assert_eq!(compute_share(4_999, 1, RoundingMode::RoundHalfUp), 0);
        assert_eq!(compute_share(-5_000, 1, RoundingMode::RoundHalfUp), -1);
        assert_eq!(compute_share(-4_999, 1, RoundingMode::RoundHalfUp), 0);
        // Truncation never rounds up.
        assert_eq!(compute_share(5_000, 1, RoundingMode::Truncation), 0);
        assert_eq!(compute_share(-5_000, 1, RoundingMode::Truncation), 0);
    }

    #[test]
    fn full_bps_returns_the_whole_amount() {
        for amount in [-12_345_i128, -1, 1, 999, 12_345] {
            for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
                assert_eq!(compute_share(amount, MAX_BPS, mode), amount);
                assert_eq!(share_and_dust(amount, MAX_BPS, mode), (amount, 0));
            }
        }
    }

    #[test]
    fn bps_above_max_collapses_share_to_zero() {
        // Explicitly out of the verified domain: share == 0 and the whole
        // product is reported as dust.
        let (result, dust) = share_and_dust(1_000, MAX_BPS + 1, RoundingMode::Truncation);
        assert_eq!(result, 0);
        assert_eq!(dust, 1_000 * (MAX_BPS as i128 + 1));
        assert_eq!(compute_share(1_000, MAX_BPS + 1, RoundingMode::RoundHalfUp), 0);
    }

    #[test]
    fn round_half_up_never_undershoots_truncation_for_positive_amounts() {
        let bps_values = [1_u32, 2, 3, 7, 50, 4_999, 5_000, 5_001, 7_500, 9_999, MAX_BPS];
        for amount in 1_i128..=200 {
            for bps in bps_values {
                let trunc = compute_share(amount, bps, RoundingMode::Truncation);
                let round = compute_share(amount, bps, RoundingMode::RoundHalfUp);
                assert!(
                    round >= trunc,
                    "round {round} < trunc {trunc} for amount={amount}, bps={bps}"
                );
            }
        }
    }

    #[test]
    fn invariant_holds_across_a_sampled_grid() {
        let mut bps = 0_u32;
        while bps <= MAX_BPS {
            for amount in [-9_999_i128, -5_000, -1, 0, 1, 4_999, 5_000, 9_999, 10_000] {
                assert_invariants(amount, bps, RoundingMode::Truncation);
                assert_invariants(amount, bps, RoundingMode::RoundHalfUp);
            }
            bps += 97;
        }
    }

    #[test]
    fn i128_min_is_exact_via_the_decomposition_path() {
        // `i128::MIN * 10_000` overflows, but `compute_share` decomposes and
        // clamps, returning the whole (negative) amount.
        assert_eq!(compute_share(i128::MIN, MAX_BPS, RoundingMode::Truncation), i128::MIN);
        assert_eq!(compute_share(i128::MIN, MAX_BPS, RoundingMode::RoundHalfUp), i128::MIN);
        // Lower bps is also safe: result stays inside [MIN, 0].
        let result = compute_share(i128::MIN, 5_000, RoundingMode::Truncation);
        assert!(result >= i128::MIN && result <= 0);
    }

    #[test]
    #[should_panic(expected = "amount * bps overflow")]
    fn naive_multiply_overflow_is_a_documented_panic() {
        let _ = naive_product_or_panic(i128::MIN, MAX_BPS);
    }

    #[test]
    #[should_panic(expected = "overflow")]
    fn share_and_dust_panics_on_the_naive_path_at_i128_min() {
        // Guards against a refactor that routes `share_and_dust` through the
        // naive multiply while still claiming `i128::MIN` safety.
        let _ = share_and_dust(i128::MIN, MAX_BPS, RoundingMode::Truncation);
    }

    #[test]
    fn naive_reference_agrees_with_decomposition_in_domain() {
        let cases: [(i128, u32); 8] = [
            (1, 1),
            (9_999, 7),
            (-9_999, 7),
            (12_345, 3_333),
            (-12_345, 3_333),
            (1_000_000, MAX_BPS),
            (-1_000_000, 1),
            (7, 9_999),
        ];
        for (amount, bps) in cases {
            let (result, dust) = share_and_dust(amount, bps, RoundingMode::Truncation);
            assert_eq!(result * BPS_DENOM + dust, naive_product_or_panic(amount, bps));
        }
    }
}
