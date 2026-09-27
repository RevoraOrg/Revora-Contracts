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

// ── Cargo-test shims (concrete inputs; always run in CI without Kani) ─────────
//
// Each test below mirrors one Kani proof from `proofs` above, using fixed concrete
// inputs that hit the same logical branch.  The harness is tested at the *pure-Rust*
// level — `compute_share`, `share_and_dust`, and `naive_product_or_panic` need no
// `Env` or Soroban setup, so these tests are fast and zero-dependency.
//
// Coverage matrix
// ───────────────────────────────────────────────────────────────────────────────
//  Test name                               | Kani proof analogue
// ──────────────────────────────────────────────────────────────────────────────
//  zero_identity_amount                    | truncation_dust_invariant (amount=0)
//  zero_identity_bps                       | truncation_dust_invariant (bps=0)
//  over_bps_guard_returns_zero             | (guard not exercised by Kani; new)
//  full_bps_returns_amount_both_modes      | full_bps_returns_amount
//  truncation_dust_invariant_concrete      | truncation_dust_invariant
//  round_half_up_dust_invariant_concrete   | round_half_up_dust_invariant
//  bounds_invariant_positive               | bounds_invariant_both_modes
//  bounds_invariant_negative               | bounds_invariant_both_modes
//  bounds_invariant_i128_extremes          | bounds_invariant_both_modes
//  rhu_gte_truncation_positive             | round_half_up_gte_truncation_for_positive_amounts
//  rhu_lte_truncation_negative             | (negative-sign direction; new)
//  exact_half_remainder_positive           | round_half_up_dust_invariant (r*bps==5000)
//  exact_half_remainder_negative           | round_half_up_dust_invariant (r*bps==-5000)
//  truncation_table                        | truncation_dust_invariant (table)
//  round_half_up_table                     | round_half_up_dust_invariant (table)
//  share_and_dust_identity                 | truncation_dust_invariant (share_and_dust)
//  share_and_dust_dust_bound               | (|dust| < 10_000; new)
//  naive_product_panics_on_overflow        | (documented panic; new)
//  i128_min_naive_multiply_overflows       | (overflow detection; new)
//  decomposition_identity_spot_checks      | (self-consistency; new)
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Bounds assertion helper ───────────────────────────────────────────────

    fn assert_bounds(result: i128, amount: i128, label: &str) {
        let lo = core::cmp::min(0_i128, amount);
        let hi = core::cmp::max(0_i128, amount);
        assert!(
            result >= lo && result <= hi,
            "{label}: result {result} not in [{lo}, {hi}] for amount={amount}"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 1. Zero-identity: amount = 0 → 0 regardless of bps or mode
    // Mirrors the (amount=0 branch) path in `truncation_dust_invariant`.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn zero_identity_amount() {
        for bps in [0u32, 1, 5_000, 9_999, 10_000, 10_001, u32::MAX] {
            assert_eq!(
                compute_share(0, bps, RoundingMode::Truncation),
                0,
                "Truncation: amount=0 bps={bps}"
            );
            assert_eq!(
                compute_share(0, bps, RoundingMode::RoundHalfUp),
                0,
                "RoundHalfUp: amount=0 bps={bps}"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 2. Zero-identity: bps = 0 → 0 regardless of amount or mode
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn zero_identity_bps() {
        for amount in [1_i128, -1, 10_000, -10_000, i128::MAX, i128::MIN, 100_000_000] {
            assert_eq!(
                compute_share(amount, 0, RoundingMode::Truncation),
                0,
                "Truncation: bps=0 amount={amount}"
            );
            assert_eq!(
                compute_share(amount, 0, RoundingMode::RoundHalfUp),
                0,
                "RoundHalfUp: bps=0 amount={amount}"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 3. Over-bps guard: bps > 10_000 → 0 (adversarial caller passes invalid bps)
    // The on-chain contract and harness both gate on `revenue_share_bps > MAX_BPS`.
    // This verifies the guard is present in the pure harness function.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn over_bps_guard_returns_zero() {
        for bps in [10_001u32, 20_000, 100_000, u32::MAX] {
            for amount in [1_i128, -1, i128::MAX, i128::MIN, 999_999] {
                assert_eq!(
                    compute_share(amount, bps, RoundingMode::Truncation),
                    0,
                    "Truncation: over-bps guard bps={bps} amount={amount}"
                );
                assert_eq!(
                    compute_share(amount, bps, RoundingMode::RoundHalfUp),
                    0,
                    "RoundHalfUp: over-bps guard bps={bps} amount={amount}"
                );
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 4. Full-share identity: bps = 10_000 → result == amount (both modes)
    // Concrete analogue of Kani `full_bps_returns_amount`.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn full_bps_returns_amount_both_modes() {
        for amount in [
            1_i128,
            -1,
            10_000,
            -10_000,
            999_999,
            -999_999,
            i128::MAX,
            i128::MIN,
            i128::MAX / 2,
            i128::MIN / 2,
        ] {
            assert_eq!(
                compute_share(amount, MAX_BPS, RoundingMode::Truncation),
                amount,
                "Truncation full-bps: amount={amount}"
            );
            assert_eq!(
                compute_share(amount, MAX_BPS, RoundingMode::RoundHalfUp),
                amount,
                "RoundHalfUp full-bps: amount={amount}"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 5. Truncation dust invariant (concrete)
    // Mirrors Kani `truncation_dust_invariant` for hand-chosen bounded inputs.
    // Within the bounded domain: result * 10_000 + rounding_dust == amount * bps
    // and |rounding_dust| < 10_000.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn truncation_dust_invariant_concrete() {
        // Use inputs within the domain where amount * bps fits in i128.
        let cases: &[(i128, u32)] = &[
            (1, 1),
            (9_999, 9_999),
            (10_000, 5_000),
            (10_001, 5_000),
            (-10_000, 5_000),
            (-10_001, 5_000),
            (100_000_000, 3_333),
            (-100_000_000, 3_333),
            (AMOUNT_ABS_BOUND, MAX_BPS),
            (-AMOUNT_ABS_BOUND, MAX_BPS),
            (AMOUNT_ABS_BOUND, 1),
            (-AMOUNT_ABS_BOUND, 1),
        ];

        for &(amount, bps) in cases {
            let (result, rounding_dust) = share_and_dust(amount, bps, RoundingMode::Truncation);
            let product = amount * bps as i128;

            assert_eq!(
                result * BPS_DENOM + rounding_dust,
                product,
                "Truncation dust identity: amount={amount} bps={bps}"
            );
            assert!(
                rounding_dust.abs() < BPS_DENOM,
                "Truncation |dust|={} must be < {BPS_DENOM}: amount={amount} bps={bps}",
                rounding_dust.abs()
            );
            assert_bounds(result, amount, &format!("Truncation amount={amount} bps={bps}"));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 6. RoundHalfUp dust invariant (concrete)
    // Mirrors Kani `round_half_up_dust_invariant`.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn round_half_up_dust_invariant_concrete() {
        let cases: &[(i128, u32)] = &[
            (1, 1),
            (1, 5_000),  // exact half → rounds up
            (-1, 5_000), // exact negative half → rounds away
            (2, 2_500),  // r*bps = 5000, exactly half
            (-2, 2_500), // r*bps = -5000
            (9_999, 9_999),
            (10_000, 5_000),
            (10_001, 5_000),
            (-10_001, 5_000),
            (100_000_000, 3_333),
            (AMOUNT_ABS_BOUND, MAX_BPS),
            (-AMOUNT_ABS_BOUND, MAX_BPS),
            (AMOUNT_ABS_BOUND, 1),
            (-AMOUNT_ABS_BOUND, 1),
        ];

        for &(amount, bps) in cases {
            let (result, rounding_dust) = share_and_dust(amount, bps, RoundingMode::RoundHalfUp);
            let product = amount * bps as i128;

            assert_eq!(
                result * BPS_DENOM + rounding_dust,
                product,
                "RoundHalfUp dust identity: amount={amount} bps={bps}"
            );
            assert!(
                rounding_dust.abs() < BPS_DENOM,
                "RoundHalfUp |dust|={} must be < {BPS_DENOM}: amount={amount} bps={bps}",
                rounding_dust.abs()
            );
            assert_bounds(result, amount, &format!("RoundHalfUp amount={amount} bps={bps}"));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 7. Bounds invariant — positive amounts
    // Concrete analogue of Kani `bounds_invariant_both_modes` (positive branch).
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn bounds_invariant_positive() {
        let amounts = [
            1_i128,
            9_999,
            10_000,
            10_001,
            999_999,
            1_000_000_000,
            AMOUNT_ABS_BOUND,
            i128::MAX / 2,
            i128::MAX,
        ];
        let bps_values = [0u32, 1, 1_000, 5_000, 9_999, 10_000];

        for amount in amounts {
            for bps in bps_values {
                for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
                    let result = compute_share(amount, bps, mode);
                    assert_bounds(
                        result,
                        amount,
                        &format!("pos amount={amount} bps={bps} {mode:?}"),
                    );
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 8. Bounds invariant — negative amounts
    // Concrete analogue of Kani `bounds_invariant_both_modes` (negative branch).
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn bounds_invariant_negative() {
        let amounts = [
            -1_i128,
            -9_999,
            -10_000,
            -10_001,
            -999_999,
            -1_000_000_000,
            -AMOUNT_ABS_BOUND,
            i128::MIN / 2,
            i128::MIN,
        ];
        let bps_values = [0u32, 1, 1_000, 5_000, 9_999, 10_000];

        for amount in amounts {
            for bps in bps_values {
                for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
                    let result = compute_share(amount, bps, mode);
                    assert_bounds(
                        result,
                        amount,
                        &format!("neg amount={amount} bps={bps} {mode:?}"),
                    );
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 9. Bounds invariant — i128 extremes (no panic, no wrap)
    // Concrete analogue of Kani `bounds_invariant_both_modes` (extreme domain).
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn bounds_invariant_i128_extremes() {
        let extreme_amounts = [i128::MAX, i128::MIN, i128::MAX - 1, i128::MIN + 1];
        let bps_values = [0u32, 1, 5_000, 9_999, 10_000];

        for amount in extreme_amounts {
            for bps in bps_values {
                for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
                    let result = compute_share(amount, bps, mode);
                    assert_bounds(
                        result,
                        amount,
                        &format!("extreme amount={amount} bps={bps} {mode:?}"),
                    );
                }
            }
        }
        // bps = 10_000 (full share) must return the exact amount even at extremes.
        assert_eq!(compute_share(i128::MAX, 10_000, RoundingMode::Truncation), i128::MAX);
        assert_eq!(compute_share(i128::MAX, 10_000, RoundingMode::RoundHalfUp), i128::MAX);
        assert_eq!(compute_share(i128::MIN, 10_000, RoundingMode::Truncation), i128::MIN);
        assert_eq!(compute_share(i128::MIN, 10_000, RoundingMode::RoundHalfUp), i128::MIN);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 10. RoundHalfUp >= Truncation for positive amounts
    // Concrete analogue of Kani `round_half_up_gte_truncation_for_positive_amounts`.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn rhu_gte_truncation_for_positive_amounts() {
        let amounts = [
            1_i128,
            9_999,
            10_000,
            10_001,
            100_000,
            1_000_000,
            AMOUNT_ABS_BOUND,
            i128::MAX / 2,
            i128::MAX,
        ];
        let bps_values = [1u32, 100, 1_000, 3_333, 5_000, 7_500, 9_999, 10_000];

        for amount in amounts {
            for bps in bps_values {
                let trunc = compute_share(amount, bps, RoundingMode::Truncation);
                let rhu = compute_share(amount, bps, RoundingMode::RoundHalfUp);
                assert!(
                    rhu >= trunc,
                    "RHU({rhu}) < Truncation({trunc}) for positive amount={amount} bps={bps}"
                );
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 11. RoundHalfUp <= Truncation for negative amounts (rounds away from zero)
    // No direct Kani proof for this case; guards the negative-direction semantics.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn rhu_lte_truncation_for_negative_amounts() {
        let amounts = [
            -1_i128,
            -9_999,
            -10_000,
            -10_001,
            -100_000,
            -AMOUNT_ABS_BOUND,
            i128::MIN / 2,
            i128::MIN,
        ];
        let bps_values = [1u32, 100, 1_000, 3_333, 5_000, 7_500, 9_999, 10_000];

        for amount in amounts {
            for bps in bps_values {
                let trunc = compute_share(amount, bps, RoundingMode::Truncation);
                let rhu = compute_share(amount, bps, RoundingMode::RoundHalfUp);
                assert!(
                    rhu <= trunc,
                    "RHU({rhu}) > Truncation({trunc}) for negative amount={amount} bps={bps}"
                );
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 12. Exact half-remainder — positive: 0.5 rounds up with RoundHalfUp, truncates to 0
    // Exercises the `remainder_product == 5_000` branch.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn exact_half_remainder_positive() {
        // amount=1, bps=5000 → r=1, r*bps=5000. Exactly half of 10_000.
        assert_eq!(compute_share(1, 5_000, RoundingMode::Truncation), 0, "trunc: 0.5 → 0");
        assert_eq!(compute_share(1, 5_000, RoundingMode::RoundHalfUp), 1, "rhu: 0.5 rounds up");

        // amount=2, bps=2500 → r=2, r*bps=5000. Same half point via different decomposition.
        assert_eq!(compute_share(2, 2_500, RoundingMode::Truncation), 0, "trunc: 2*2500 → 0");
        assert_eq!(compute_share(2, 2_500, RoundingMode::RoundHalfUp), 1, "rhu: 2*2500 rounds up");

        // Larger: amount=50_005_000, bps=1 → q=5000, r=5000, r*bps=5000.
        // base = 5000*1 = 5000. Trunc: 5000+0 = 5000. RHU: 5000+1 = 5001.
        assert_eq!(compute_share(50_005_000, 1, RoundingMode::Truncation), 5_000);
        assert_eq!(compute_share(50_005_000, 1, RoundingMode::RoundHalfUp), 5_001);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 13. Exact half-remainder — negative: -0.5 rounds to -1 with RoundHalfUp
    // Exercises the `remainder_product < 0` branch (rounds away from zero).
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn exact_half_remainder_negative() {
        // amount=-1, bps=5000 → r=-1, r*bps=-5000. Exactly negative half.
        assert_eq!(compute_share(-1, 5_000, RoundingMode::Truncation), 0, "trunc: -0.5 → 0");
        assert_eq!(compute_share(-1, 5_000, RoundingMode::RoundHalfUp), -1, "rhu: -0.5 → -1");

        // amount=-2, bps=2500 → r=-2, r*bps=-5000.
        assert_eq!(compute_share(-2, 2_500, RoundingMode::Truncation), 0, "trunc: -2*2500 → 0");
        assert_eq!(compute_share(-2, 2_500, RoundingMode::RoundHalfUp), -1, "rhu: -2*2500 → -1");

        // Larger: amount=-50_005_000, bps=1 → q=-5000, r=-5000, r*bps=-5000.
        // base = -5000. Trunc: -5000+0 = -5000. RHU: -5000-1 = -5001.
        assert_eq!(compute_share(-50_005_000, 1, RoundingMode::Truncation), -5_000);
        assert_eq!(compute_share(-50_005_000, 1, RoundingMode::RoundHalfUp), -5_001);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 14. Truncation table-driven cases
    // Representative values that confirm the concrete arithmetic matches expected.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn truncation_table() {
        // (amount, bps, expected_result)
        let cases: &[(i128, u32, i128)] = &[
            // Zero identity
            (0, 5_000, 0),
            (1_000_000, 0, 0),
            // Full share
            (10_000, 10_000, 10_000),
            (1, 10_000, 1),
            (-1, 10_000, -1),
            // 50% — truncates fractional
            (10_000, 5_000, 5_000),
            (10_001, 5_000, 5_000), // 5000.5 truncates to 5000
            (1, 5_000, 0),          // 0.5 truncates to 0
            (-10_000, 5_000, -5_000),
            (-10_001, 5_000, -5_000), // -5000.5 truncates toward zero → -5000
            (-1, 5_000, 0),           // -0.5 truncates to 0
            // 1 bps = 0.01%
            (10_000, 1, 1),
            (9_999, 1, 0),       // 0.9999 truncates to 0
            (1_000_000, 1, 100), // exact
            // Over-bps guard
            (1_000_000, 10_001, 0),
            // Boundary: bps = 9_999 on amount = 10_000
            (10_000, 9_999, 9_999),
        ];

        for &(amount, bps, expected) in cases {
            let result = compute_share(amount, bps, RoundingMode::Truncation);
            assert_eq!(
                result, expected,
                "Truncation: amount={amount} bps={bps} → expected {expected} got {result}"
            );
            assert_bounds(result, amount, &format!("Truncation amount={amount} bps={bps}"));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 15. RoundHalfUp table-driven cases
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn round_half_up_table() {
        // (amount, bps, expected_result)
        let cases: &[(i128, u32, i128)] = &[
            // Zero identity
            (0, 5_000, 0),
            (1_000_000, 0, 0),
            // Full share
            (10_000, 10_000, 10_000),
            (1, 10_000, 1),
            (-1, 10_000, -1),
            // 50% — half rounds up for positives
            (10_000, 5_000, 5_000),
            (10_001, 5_000, 5_001),   // 5000.5 rounds up to 5001
            (1, 5_000, 1),            // 0.5 rounds up to 1
            (-1, 5_000, -1),          // -0.5 rounds away from zero
            (-10_001, 5_000, -5_001), // -5000.5 rounds away
            // 1 bps
            (10_000, 1, 1),
            (9_999, 1, 1),   // 0.9999 rounds up (> 0.5)
            (4_999, 1, 0),   // 0.4999 rounds down
            (5_000, 1, 1),   // exactly 0.5 rounds up
            (-5_000, 1, -1), // exactly -0.5 rounds away
            // Over-bps guard
            (1_000_000, 10_001, 0),
        ];

        for &(amount, bps, expected) in cases {
            let result = compute_share(amount, bps, RoundingMode::RoundHalfUp);
            assert_eq!(
                result, expected,
                "RoundHalfUp: amount={amount} bps={bps} → expected {expected} got {result}"
            );
            assert_bounds(result, amount, &format!("RoundHalfUp amount={amount} bps={bps}"));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 16. share_and_dust identity: result * BPS_DENOM + dust == product
    // Directly exercises the helper function used by Kani proofs.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn share_and_dust_identity() {
        let cases: &[(i128, u32)] = &[
            (1, 1),
            (1, 5_000),
            (-1, 5_000),
            (10_001, 5_000),
            (-10_001, 5_000),
            (AMOUNT_ABS_BOUND, 9_999),
            (-AMOUNT_ABS_BOUND, 9_999),
        ];

        for &(amount, bps) in cases {
            for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
                let (result, dust) = share_and_dust(amount, bps, mode);
                let product = amount * bps as i128;
                assert_eq!(
                    result * BPS_DENOM + dust,
                    product,
                    "share_and_dust identity: amount={amount} bps={bps} {mode:?}"
                );
                assert_bounds(result, amount, &format!("share_and_dust amount={amount} bps={bps}"));
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 17. Rounding dust magnitude: |dust| < BPS_DENOM for non-trivial inputs
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn share_and_dust_dust_bound() {
        let cases: &[(i128, u32)] = &[
            (1, 1),
            (9_999, 9_999),
            (10_001, 5_000),
            (-10_001, 5_000),
            (100_000_000, 7_777),
            (-100_000_000, 7_777),
            (AMOUNT_ABS_BOUND, 1),
            (-AMOUNT_ABS_BOUND, 1),
            (AMOUNT_ABS_BOUND, MAX_BPS),
            (-AMOUNT_ABS_BOUND, MAX_BPS),
        ];

        for &(amount, bps) in cases {
            for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
                let (_, dust) = share_and_dust(amount, bps, mode);
                assert!(
                    dust.abs() < BPS_DENOM,
                    "|dust|={} must be < {BPS_DENOM}: amount={amount} bps={bps} {mode:?}",
                    dust.abs()
                );
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 18. Naive multiply overflow detection
    // Documents that `i128::MIN * 10_000` overflows i128 (cannot fit).
    // The pure harness exposes `naive_product_or_panic` to make this explicit.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn i128_min_naive_multiply_overflows_i128() {
        // Verify the overflow is detectable via checked_mul.
        assert!(
            i128::MIN.checked_mul(10_000).is_none(),
            "i128::MIN * 10_000 must not fit in i128 — overflow detected"
        );
        // i128::MAX * 10_000 also overflows.
        assert!(
            i128::MAX.checked_mul(10_000).is_none(),
            "i128::MAX * 10_000 must not fit in i128 — overflow detected"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 19. Naive multiply documented panic
    // Confirms the explicit panic path in `naive_product_or_panic` triggers for
    // overflow inputs — this documents *why* the decomposition path must be used.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "amount * bps overflow: decomposition path must be used instead")]
    fn naive_product_or_panic_panics_on_overflow() {
        naive_product_or_panic(i128::MIN, MAX_BPS);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 20. Decomposition identity spot-checks: pure harness vs. manual calculation
    // Self-consistency verification of the decomposition arithmetic.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn decomposition_identity_spot_checks() {
        // (amount, bps, mode, expected)
        // Manual: q = amount / 10_000, r = amount % 10_000
        //         base = q * bps
        //         Truncation:  result = base + (r * bps) / 10_000
        //         RoundHalfUp: result = base + round_half_up(r * bps)
        let cases: &[(i128, u32, RoundingMode, i128)] = &[
            // amount=20_003, bps=5000:
            //   q=2, r=3; base=2*5000=10_000; r*bps=15_000
            //   Trunc: 10_000 + 15_000/10_000 = 10_001
            //   RHU:   10_000 + (15_000+5_000)/10_000 = 10_002
            (20_003, 5_000, RoundingMode::Truncation, 10_001),
            (20_003, 5_000, RoundingMode::RoundHalfUp, 10_002),
            // amount=-20_003, bps=5000: same magnitudes, negative clamped
            //   Trunc: -10_001 (toward zero from -10_001.5)
            //   RHU:   -10_002 (away from zero)
            (-20_003, 5_000, RoundingMode::Truncation, -10_001),
            (-20_003, 5_000, RoundingMode::RoundHalfUp, -10_002),
            // amount=1, bps=10_000: full share → 1
            (1, 10_000, RoundingMode::Truncation, 1),
            (1, 10_000, RoundingMode::RoundHalfUp, 1),
            // amount=10_000, bps=1: q=1, r=0; base=1; r*bps=0 → result=1
            (10_000, 1, RoundingMode::Truncation, 1),
            (10_000, 1, RoundingMode::RoundHalfUp, 1),
            // amount=9_999, bps=1: q=0, r=9_999; base=0; r*bps=9_999
            //   Trunc: 9_999/10_000 = 0; RHU: (9_999+5_000)/10_000 = 1
            (9_999, 1, RoundingMode::Truncation, 0),
            (9_999, 1, RoundingMode::RoundHalfUp, 1),
        ];

        for &(amount, bps, mode, expected) in cases {
            let result = compute_share(amount, bps, mode);
            assert_eq!(
                result, expected,
                "decomposition spot-check: amount={amount} bps={bps} {mode:?} \
                 → expected {expected} got {result}"
            );
            assert_bounds(result, amount, &format!("decomposition amount={amount} bps={bps}"));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 21. State-unchanged adversarial: over-bps does not mutate any shared state
    // The pure function returns 0 and has no side-effects. This test verifies
    // repeated over-bps calls interleaved with valid calls do not affect results.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn over_bps_call_does_not_corrupt_subsequent_calls() {
        // Call with invalid bps first.
        let _ = compute_share(1_000_000, 10_001, RoundingMode::Truncation);
        let _ = compute_share(1_000_000, u32::MAX, RoundingMode::RoundHalfUp);

        // Subsequent valid call must produce the correct result.
        let result = compute_share(100_000, 5_000, RoundingMode::Truncation);
        assert_eq!(result, 50_000, "valid call after over-bps must still return 50_000");
        assert_bounds(result, 100_000, "post-adversarial valid call");
    }
}
