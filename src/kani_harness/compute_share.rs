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

#![cfg_attr(not(kani), allow(dead_code))]

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
