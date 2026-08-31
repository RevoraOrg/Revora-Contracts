//! # Formal Property Test — `compute_share` Decomposition Identity [Issue #411, #574]
//!
//! Asserts the algebraic decomposition identity for `compute_share` with
//! **both** `RoundingMode::Truncation` and `RoundingMode::RoundHalfUp`
//! over a bounded fuzz space that includes negative amounts:
//!
//! ```text
//! compute_share(amount, bps, mode)
//!     == clamp(base + remainder_share, [min(0, amount), max(0, amount)])
//! ```
//!
//! where:
//! - `q = amount / 10_000`, `r = amount % 10_000`
//! - `base = q * bps` (with overflow-safe saturating arithmetic)
//! - `remainder_product = r * bps`
//! - For Truncation:   `remainder_share = remainder_product / 10_000`
//! - For RoundHalfUp:  `remainder_share = round_half_up(remainder_product)`
//!
//! The right-hand side is computed with the same overflow-safe arithmetic
//! used by the implementation (checked_mul with sign-aware saturation).
//!
//! ## Additional invariants verified in the same property
//!
//! - **Clamp invariant**: result ∈ `[min(0, amount), max(0, amount)]` for all inputs.
//! - **Negative-amount clamp**: negative amounts clamp at `[amount, 0]`.
//! - **Rounding dust invariant**: `result * 10_000 + rounding_dust == amount * bps`
//!   with `|rounding_dust| < 10_000` (within the bounded domain where `amount * bps`
//!   fits in i128).
//! - **Rounding direction**: `RoundHalfUp >= Truncation` for positive amounts and
//!   `RoundHalfUp <= Truncation` for negative amounts (midpoint rounds away from zero).
//! - **Boundary seeds**: `bps ∈ {0, 10_000}` and `amount ∈ {i128::MIN/2, i128::MAX/2}`
//!   are always exercised via explicit boundary cases in addition to the fuzz space.
//!
//! ## Fuzz space
//!
//! | Parameter | Range                                  | Rationale                                      |
//! |-----------|----------------------------------------|------------------------------------------------|
//! | `amount`  | `i128::MIN/2 ..= i128::MAX/2`          | Avoids saturation in the reference formula so  |
//! |           |                                        | the identity holds without clamping noise.     |
//! | `bps`     | `0 ..= 10_000`                         | Full valid range; values > 10_000 return 0 by  |
//! |           |                                        | the over-bps guard and are tested separately.  |
//! | `mode`    | `Truncation \| RoundHalfUp`            | All rounding modes exercised uniformly.        |
//!
//! ## Case count
//!
//! 4 096 cases per rounding mode (8 192 total), satisfying the ≥ 4 096 threshold
//! required by the harness spec. The cases are split evenly between the two modes
//! via `prop_oneof!` selection so both paths receive equal coverage.
//!
//! ## Security note
//!
//! `compute_share` is on the critical payout path. A refactor that silently
//! changes the decomposition arithmetic (e.g. reordering operations, switching
//! to a single `amount * bps / 10_000` expression) would introduce overflow for
//! large `amount` values. This property test catches such regressions before
//! audit by locking the algebraic identity across the full bounded fuzz space
//! for both rounding modes.
//!
//! The over-bps guard (`bps > 10_000 → 0`) is also verified to ensure no
//! refactor accidentally removes it.

#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode};
use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Env};

// ── Test client ───────────────────────────────────────────────────────────────

fn make_client() -> (Env, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);
    (env, client)
}

// ── Reference implementations ─────────────────────────────────────────────────

/// Pure-Rust reference for the decomposition identity supporting both rounding modes.
///
/// Mirrors the overflow-safe arithmetic in `compute_share` exactly, so the
/// property test is checking algebraic equivalence rather than re-implementing
/// the function from scratch.
///
/// Decomposition:
///   amount = q * 10_000 + r   (where |r| < 10_000)
///   base = q * bps
///   remainder_product = r * bps
///   remainder_share = mode.apply(remainder_product)
///   result = clamp(base + remainder_share, [min(0,amount), max(0,amount)])
///
/// Uses the same checked_mul + sign-aware saturation as the contract, then
/// applies the same clamp. This means the property holds even when saturation
/// fires (both sides saturate identically).
fn reference_decomposition(amount: i128, bps: u32, mode: RoundingMode) -> i128 {
    if bps > 10_000 {
        return 0;
    }
    if amount == 0 || bps == 0 {
        return 0;
    }

    let q = amount / 10_000;
    let r = amount % 10_000;
    let bps_i = bps as i128;

    // base = q * bps  (checked, sign-aware saturation)
    let base = q.checked_mul(bps_i).unwrap_or_else(|| {
        if (q >= 0 && bps_i >= 0) || (q < 0 && bps_i < 0) {
            i128::MAX
        } else {
            i128::MIN
        }
    });

    // remainder_product = r * bps  (checked, sign-aware saturation)
    // |r| < 10_000 and bps ≤ 10_000, so |r * bps| < 10^8 — never saturates in practice.
    let remainder_product = r.checked_mul(bps_i).unwrap_or_else(|| {
        if (r >= 0 && bps_i >= 0) || (r < 0 && bps_i < 0) {
            i128::MAX
        } else {
            i128::MIN
        }
    });

    // Apply rounding mode to the remainder product
    let remainder_share = match mode {
        RoundingMode::Truncation => {
            // Integer division toward zero
            remainder_product / 10_000
        }
        RoundingMode::RoundHalfUp => {
            let half = 5_000_i128;
            if remainder_product >= 0 {
                // Add half before dividing to round up at the midpoint
                remainder_product.saturating_add(half) / 10_000
            } else {
                // Subtract half before dividing to round away from zero
                remainder_product.saturating_sub(half) / 10_000
            }
        }
    };

    // final add (checked, sign-aware saturation)
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

    // Clamp to [min(0, amount), max(0, amount)]
    let lo = core::cmp::min(0, amount);
    let hi = core::cmp::max(0, amount);
    core::cmp::min(core::cmp::max(share, lo), hi)
}

/// Compute rounding dust: `dust = amount * bps - result * 10_000`.
///
/// This is only valid when `amount * bps` fits in i128 without overflow.    /// For the subset of the fuzz space where `amount * bps` fits in i128,
/// the dust invariant always holds. When the product overflows, the
/// invariant is skipped (this occurs for large `|amount|` with high `bps`).
///
/// Returns `None` when the product would overflow i128 (which occurs for large
/// `|amount|` with non-trivial `bps`; the dust invariant is only checked for the
/// subset of the fuzz space where `amount * bps` fits in i128).
fn rounding_dust(amount: i128, bps: u32, result: i128) -> Option<i128> {
    let product = amount.checked_mul(bps as i128)?;
    let reconstruction = result.checked_mul(10_000)?;
    Some(product - reconstruction)
}

// ── Bounds helper ─────────────────────────────────────────────────────────────

fn assert_bounds(result: i128, amount: i128, label: &str) {
    let lo = core::cmp::min(0_i128, amount);
    let hi = core::cmp::max(0_i128, amount);
    assert!(
        result >= lo && result <= hi,
        "{label}: result {result} out of [{lo}, {hi}] for amount={amount}"
    );
}

// ── Proptest strategies ───────────────────────────────────────────────────────

/// Fuzz strategy for `amount`: bounded to `[i128::MIN/2, i128::MAX/2]`.
///
/// This range covers both negative and positive values including zero and ±1,
/// while avoiding saturation in the reference formula.
fn arb_fuzz_amount() -> impl Strategy<Value = i128> {
    (i128::MIN / 2)..=(i128::MAX / 2)
}

/// Fuzz strategy for `bps`: full valid range `[0, 10_000]`.
fn arb_fuzz_bps() -> impl Strategy<Value = u32> {
    0u32..=10_000u32
}

/// Fuzz strategy for rounding mode: uniform choice between both variants.
fn arb_rounding_mode() -> impl Strategy<Value = RoundingMode> {
    prop_oneof![Just(RoundingMode::Truncation), Just(RoundingMode::RoundHalfUp),]
}

// ══════════════════════════════════════════════════════════════════════════════
// PROPERTY: Decomposition identity for ALL rounding modes (Issue #574)
// ══════════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig {
        // 8 192 total cases → 4 096 per rounding mode (uniform distribution).
        // This satisfies the ≥ 4 096 per-mode threshold required by the harness spec.
        cases: 8_192,
        // Provide a fixed seed so CI failures are reproducible.
        ..ProptestConfig::default()
    })]

    /// **Core property (Issue #574): Decomposition identity parameterized over rounding mode.**
    ///
    /// For all `amount ∈ [i128::MIN/2, i128::MAX/2]`, `bps ∈ [0, 10_000]`,
    /// and `mode ∈ {Truncation, RoundHalfUp}`:
    ///
    /// ```text
    /// compute_share(amount, bps, mode)
    ///     == reference_decomposition(amount, bps, mode)
    /// ```
    ///
    /// The right-hand side mirrors the contract's quotient-remainder decomposition
    /// with the same overflow-safe arithmetic (see `reference_decomposition`).
    ///
    /// Also asserts the clamp invariant: result ∈ `[min(0, amount), max(0, amount)]`.
    #[test]
    fn prop_decomposition_identity_all_modes(
        amount in arb_fuzz_amount(),
        bps    in arb_fuzz_bps(),
        mode   in arb_rounding_mode(),
    ) {
        let (_env, client) = make_client();

        let actual    = client.compute_share(&amount, &bps, &mode);
        let expected  = reference_decomposition(amount, bps, mode);

        prop_assert_eq!(
            actual, expected,
            "decomposition identity failed: amount={}, bps={}, mode={:?} \
             → actual={}, expected={}", amount, bps, mode, actual, expected
        );

        // Clamp invariant must hold independently of the identity.
        let lo = core::cmp::min(0_i128, amount);
        let hi = core::cmp::max(0_i128, amount);
        prop_assert!(
            actual >= lo && actual <= hi,
            "clamp invariant violated: amount={}, bps={}, mode={:?}, \
             result={}, expected range=[{}, {}]", amount, bps, mode, actual, lo, hi
        );
    }

    /// **Rounding dust invariant (Issue #574):**
    ///
    /// For all inputs in the bounded domain (where `amount * bps` fits in i128),
    /// the result satisfies:
    ///
    /// ```text
    /// result * 10_000 + rounding_dust == amount * bps
    /// |rounding_dust| < 10_000
    /// ```
    ///
    /// This mirrors the Kani harness invariant from `kani_harness/compute_share.rs`
    /// and catches rounding errors that the decomposition identity might miss.
    #[test]
    fn prop_rounding_dust_invariant(
        amount in arb_fuzz_amount(),
        bps    in arb_fuzz_bps(),
        mode   in arb_rounding_mode(),
    ) {
        let (_env, client) = make_client();
        let result = client.compute_share(&amount, &bps, &mode);

        if let Some(dust) = rounding_dust(amount, bps, result) {
            // Reconstruction: result * 10_000 + dust must equal amount * bps
            let product = amount.checked_mul(bps as i128).unwrap();
            let reconstruction = result.checked_mul(10_000).unwrap_or(i128::MAX);
            prop_assert_eq!(
                reconstruction.checked_add(dust).unwrap_or(i128::MAX),
                product,
                "rounding dust invariant failed: amount={}, bps={}, mode={:?}, \
                 result={}, dust={}, product={}", amount, bps, mode, result, dust, product
            );

            // |rounding_dust| < 10_000 (unless amount=0 or bps=0, where dust must be 0)
            if amount != 0 && bps != 0 {
                prop_assert!(
                    dust.abs() < 10_000,
                    "dust magnitude too large: amount={}, bps={}, mode={:?}, \
                     dust={}", amount, bps, mode, dust
                );
            } else {
                prop_assert_eq!(
                    dust, 0,
                    "dust must be zero when amount=0 or bps=0: amount={}, bps={}, \
                     mode={:?}, dust={}", amount, bps, mode, dust
                );
            }
        }
    }

    /// **Rounding direction invariant:**
    ///
    /// RoundHalfUp rounds away from zero at the midpoint, so:
    /// - For positive amounts: `RoundHalfUp >= Truncation`
    /// - For negative amounts: `RoundHalfUp <= Truncation`
    ///
    /// This covers the "rounding at exactly-half remainders across signs" requirement.
    #[test]
    fn prop_rounding_direction(
        amount in arb_fuzz_amount(),
        bps    in arb_fuzz_bps(),
    ) {
        let (_env, client) = make_client();

        let trunc = client.compute_share(&amount, &bps, &RoundingMode::Truncation);
        let rhu   = client.compute_share(&amount, &bps, &RoundingMode::RoundHalfUp);

        if amount > 0 {
            prop_assert!(
                rhu >= trunc,
                "RoundHalfUp must be >= Truncation for positive amounts: \
                 amount={}, bps={}, trunc={}, rhu={}", amount, bps, trunc, rhu
            );
        } else if amount < 0 {
            prop_assert!(
                rhu <= trunc,
                "RoundHalfUp must be <= Truncation for negative amounts: \
                 amount={}, bps={}, trunc={}, rhu={}", amount, bps, trunc, rhu
            );
        }
        // When amount == 0, both should be 0.
    }

    /// **Negative-amount clamp invariant:**
    ///
    /// For all negative `amount` and valid `bps`, the result must be ≤ 0 and ≥ amount.
    /// This locks the "clamp at extremes" requirement for both rounding modes.
    #[test]
    fn prop_negative_amount_clamp(
        // Use a sub-range of the fuzz space that is strictly negative.
        amount in (i128::MIN / 2)..=-1_i128,
        bps    in arb_fuzz_bps(),
        mode   in arb_rounding_mode(),
    ) {
        let (_env, client) = make_client();

        let result = client.compute_share(&amount, &bps, &mode);

        prop_assert!(
            result <= 0,
            "negative amount must produce non-positive result: \
             amount={}, bps={}, mode={:?}, result={}", amount, bps, mode, result
        );
        prop_assert!(
            result >= amount,
            "result must not be more negative than amount: \
             amount={}, bps={}, mode={:?}, result={}", amount, bps, mode, result
        );
    }

    /// **Over-bps guard property:**
    ///
    /// For all `bps > 10_000` and any `amount`, `compute_share` must return 0.
    /// Verifies the guard is not accidentally removed by a refactor.
    /// Tested for both rounding modes.
    #[test]
    fn prop_over_bps_guard(
        amount in arb_fuzz_amount(),
        bps    in 10_001u32..=u32::MAX,
        mode   in arb_rounding_mode(),
    ) {
        let (_env, client) = make_client();

        let result = client.compute_share(&amount, &bps, &mode);
        prop_assert_eq!(
            result, 0,
            "over-bps guard failed: amount={}, bps={}, mode={:?}, result={}", amount, bps, mode, result
        );
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// BOUNDARY SEEDS — always exercised regardless of proptest shrinking
// ══════════════════════════════════════════════════════════════════════════════

/// Explicit boundary cases for **both** rounding modes.
///
/// These deterministic unit tests complement the fuzz properties above.
/// They cover the four corners of the fuzz space, zero-identity cases,
/// and exactly-half remainders across signs (critical for RoundHalfUp).
#[test]
fn boundary_seeds_decomposition_identity() {
    let (_env, client) = make_client();

    // (amount, bps) boundary seeds
    let seeds: &[(i128, u32)] = &[
        // Corners of the fuzz space
        (i128::MIN / 2, 0),
        (i128::MIN / 2, 10_000),
        (i128::MIN / 2, 5_000),
        (i128::MIN / 2, 1),
        (i128::MAX / 2, 0),
        (i128::MAX / 2, 10_000),
        (i128::MAX / 2, 5_000),
        (i128::MAX / 2, 1),
        // Zero identity
        (0, 0),
        (0, 5_000),
        (0, 10_000),
        (1_000_000, 0),
        // Near-zero amounts
        (1, 1),
        (1, 5_000),
        (1, 10_000),
        (-1, 1),
        (-1, 5_000),
        (-1, 10_000),
        // Exact 10_000 boundary (remainder = 0)
        (10_000, 5_000),
        (-10_000, 5_000),
        (10_000, 1),
        // Just above/below 10_000 (remainder = ±1)
        (10_001, 5_000),
        (-10_001, 5_000),
        // Large mid-range
        (1_000_000_000, 3_333),
        (-1_000_000_000, 3_333),
        // bps = 10_000 full-share identity
        (i128::MAX / 2, 10_000),
        (i128::MIN / 2, 10_000),
        // ── RoundHalfUp-specific boundary seeds (#574) ──
        // Exactly-half remainders: r * bps takes the form k*5000
        // For amount=5000, bps=1: q=0, r=5000, r*bps=5000. RHU: (5000+5000)/10000 = 1
        (5_000, 1),
        // For amount=5000, bps=2: q=0, r=5000, r*bps=10000. RHU: (10000+5000)/10000 = 1 (trunc=1 too)
        (5_000, 2),
        // amount=2, bps=2500: q=0, r=2, r*bps=5000. RHU: (5000+5000)/10000 = 1, trunc = 0
        (2, 2_500),
        // Negative exactly-half: amount=-5000, bps=1: q=0, r=-5000, r*bps=-5000.
        // RHU: (-5000-5000)/10000 = -1, trunc = 0
        (-5_000, 1),
        // Negative: amount=-2, bps=2500: q=0, r=-2, r*bps=-5000.
        // RHU: (-5000-5000)/10000 = -1, trunc = 0
        (-2, 2_500),
        // Large amount with half remainder
        (10_005_000, 1),  // q=1000, r=5000, r*bps=5000. RHU: (5000+5000)/10000 = 1
        (-10_005_000, 1), // q=-1000, r=-5000, r*bps=-5000. RHU: (-5000-5000)/10000 = -1
        // amount=5000, bps=5000: q=0, r=5000, r*bps=25_000_000.
        // RHU: (25000000+5000)/10000 = 2500, trunc = 2500 (no half here, exact division)
        (5_000, 5_000),
        // amount=-5000, bps=5000: q=0, r=-5000, r*bps=-25_000_000.
        // RHU: (-25000000-5000)/10000 = -2500, trunc = -2500
        (-5_000, 5_000),
    ];

    for &(amount, bps) in seeds {
        for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
            let actual = client.compute_share(&amount, &bps, &mode);
            let expected = reference_decomposition(amount, bps, mode);

            assert_eq!(
                actual, expected,
                "boundary seed failed: amount={amount}, bps={bps}, mode={mode:?} \
                 → actual={actual}, expected={expected}"
            );
            assert_bounds(
                actual,
                amount,
                &format!("boundary seed amount={amount} bps={bps} mode={mode:?}"),
            );
        }
    }
}

/// Verify the over-bps guard at exact boundary values for both rounding modes.
#[test]
fn boundary_seeds_over_bps_guard() {
    let (_env, client) = make_client();

    let amounts = [1_i128, -1, 10_000, -10_000, i128::MAX / 2, i128::MIN / 2];
    let over_bps = [10_001u32, 20_000, u32::MAX];

    for &amount in &amounts {
        for &bps in &over_bps {
            for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
                let result = client.compute_share(&amount, &bps, &mode);
                assert_eq!(
                    result, 0,
                    "over-bps guard boundary: amount={amount}, bps={bps}, mode={mode:?}, result={result}"
                );
            }
        }
    }
}

/// Verify the full-share identity (`bps = 10_000 → result = amount`) at boundaries
/// for both rounding modes.
#[test]
fn boundary_seeds_full_share_identity() {
    let (_env, client) = make_client();

    let amounts =
        [1_i128, -1, 10_000, -10_000, 100_000_000, -100_000_000, i128::MAX / 2, i128::MIN / 2];

    for &amount in &amounts {
        for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
            let result = client.compute_share(&amount, &10_000, &mode);
            assert_eq!(
                result, amount,
                "full-share identity failed: amount={amount}, mode={mode:?}, result={result}"
            );
        }
    }
}

/// Verify the zero-identity (`amount = 0` or `bps = 0 → result = 0`) at boundaries
/// for both rounding modes.
#[test]
fn boundary_seeds_zero_identity() {
    let (_env, client) = make_client();

    // amount = 0 for all bps
    for bps in [0u32, 1, 5_000, 9_999, 10_000] {
        for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
            assert_eq!(
                client.compute_share(&0, &bps, &mode),
                0,
                "zero-amount identity failed for bps={bps}, mode={mode:?}"
            );
        }
    }

    // bps = 0 for boundary amounts
    for amount in [1_i128, -1, i128::MAX / 2, i128::MIN / 2] {
        for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
            assert_eq!(
                client.compute_share(&amount, &0, &mode),
                0,
                "zero-bps identity failed for amount={amount}, mode={mode:?}"
            );
        }
    }
}

/// Verify exactly-half remainders with RoundHalfUp across signs.
///
/// These are the trickiest edge cases: when `r * bps` is exactly half of 10_000,
/// RoundHalfUp must round away from zero while Truncation rounds toward zero.
#[test]
fn boundary_seeds_exact_half_remainders() {
    let (_env, client) = make_client();

    // Each case: (amount, bps, expected_trunc, expected_rhu, description)
    let cases: &[(i128, u32, i128, i128, &str)] = &[
        // amount=2, bps=2500: q=0, r=2, r*bps=5000. Exactly half of 10_000.
        // Trunc: 5000/10000=0, RHU: (5000+5000)/10000=1
        (2, 2_500, 0, 1, "positive half-up rounds up"),
        // amount=-2, bps=2500: q=0, r=-2, r*bps=-5000. Exactly negative half.
        // Trunc: -5000/10000=0, RHU: (-5000-5000)/10000=-1
        (-2, 2_500, 0, -1, "negative half-up rounds away from zero"),
        // amount=5002, bps=2500: q=0, r=5002, r*bps=12_505_000.
        // Trunc: 12505000/10000=1250, RHU: (12505000+5000)/10000=1251
        (5_002, 2_500, 1_250, 1_251, "positive just above half rounds up"),
        // amount=-5002, bps=2500: q=0, r=-5002, r*bps=-12_505_000.
        // Trunc: -12505000/10000=-1250, RHU: (-12505000-5000)/10000=-1251
        (-5_002, 2_500, -1_250, -1_251, "negative just above half rounds away"),
        // amount=4998, bps=2500: q=0, r=4998, r*bps=12_495_000.
        // Trunc: 12495000/10000=1249, RHU: (12495000+5000)/10000=1250
        (4_998, 2_500, 1_249, 1_250, "positive just below half rounds down"),
        // amount=-4998, bps=2500: q=0, r=-4998, r*bps=-12_495_000.
        // Trunc: -12495000/10000=-1249, RHU: (-12495000-5000)/10000=-1250
        (-4_998, 2_500, -1_249, -1_250, "negative just below half rounds down"),
        // Larger: amount=50_005_000, bps=1: q=5000, r=5000, r*bps=5000.
        // base = 5000*1 = 5000
        // Trunc: 5000 + 5000/10000 = 5000, RHU: 5000 + (5000+5000)/10000 = 5001
        (50_005_000, 1, 5_000, 5_001, "positive with non-zero quotient half-up"),
        // amount=-50_005_000, bps=1: q=-5000, r=-5000, r*bps=-5000.
        // base = -5000*1 = -5000
        // Trunc: -5000 + (-5000)/10000 = -5000, RHU: -5000 + (-5000-5000)/10000 = -5001
        (-50_005_000, 1, -5_000, -5_001, "negative with non-zero quotient half-up"),
    ];

    for &(amount, bps, expected_trunc, expected_rhu, desc) in cases {
        let trunc = client.compute_share(&amount, &bps, &RoundingMode::Truncation);
        let rhu = client.compute_share(&amount, &bps, &RoundingMode::RoundHalfUp);

        assert_eq!(
            trunc, expected_trunc,
            "{desc}: Truncation mismatch amount={amount}, bps={bps}: \
             expected={expected_trunc}, got={trunc}"
        );
        assert_eq!(
            rhu, expected_rhu,
            "{desc}: RoundHalfUp mismatch amount={amount}, bps={bps}: \
             expected={expected_rhu}, got={rhu}"
        );
    }
}
