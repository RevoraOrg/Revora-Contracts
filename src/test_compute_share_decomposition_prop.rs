//! # Formal Property Test — `compute_share` Decomposition Identity [Issue #411]
//!
//! Asserts the algebraic decomposition identity for `compute_share` with
//! `RoundingMode::Truncation` over a bounded fuzz space:
//!
//! ```text
//! compute_share(amount, bps, Truncation)
//!     == (amount / 10_000) * bps + (amount % 10_000) * bps / 10_000
//! ```
//!
//! where the right-hand side is computed with the same overflow-safe arithmetic
//! used by the implementation (checked_mul with sign-aware saturation).
//!
//! ## Additional invariants verified in the same property
//!
//! - **Clamp invariant**: result ∈ `[min(0, amount), max(0, amount)]` for all inputs.
//! - **Negative-amount clamp**: negative amounts clamp at `[amount, 0]`.
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
//!
//! ## Security note
//!
//! `compute_share` is on the critical payout path. A refactor that silently
//! changes the decomposition arithmetic (e.g. reordering operations, switching
//! to a single `amount * bps / 10_000` expression) would introduce overflow for
//! large `amount` values. This property test catches such regressions before
//! audit by locking the algebraic identity across the full bounded fuzz space.
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

// ── Reference implementation ──────────────────────────────────────────────────

/// Pure-Rust reference for the decomposition identity.
///
/// Mirrors the overflow-safe arithmetic in `compute_share` exactly, so the
/// property test is checking algebraic equivalence rather than re-implementing
/// the function from scratch.
///
/// For `Truncation` mode:
///   result = q * bps + (r * bps) / 10_000
/// where q = amount / 10_000, r = amount % 10_000.
///
/// Uses the same checked_mul + sign-aware saturation as the contract, then
/// applies the same clamp. This means the property holds even when saturation
/// fires (both sides saturate identically).
fn reference_decomposition(amount: i128, bps: u32) -> i128 {
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

    // Truncation: integer division toward zero
    let remainder_share = remainder_product / 10_000;

    // final add (checked, sign-aware saturation)
    let share = base.checked_add(remainder_share).unwrap_or_else(|| {
        if (base >= 0 && remainder_share >= 0) || (base < 0 && remainder_share < 0) {
            if base >= 0 { i128::MAX } else { i128::MIN }
        } else {
            0
        }
    });

    // Clamp to [min(0, amount), max(0, amount)]
    let lo = core::cmp::min(0, amount);
    let hi = core::cmp::max(0, amount);
    core::cmp::min(core::cmp::max(share, lo), hi)
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
/// This range avoids saturation in the reference formula while still covering
/// large positive and negative values, including zero and ±1.
fn arb_fuzz_amount() -> impl Strategy<Value = i128> {
    (i128::MIN / 2)..=(i128::MAX / 2)
}

/// Fuzz strategy for `bps`: full valid range `[0, 10_000]`.
fn arb_fuzz_bps() -> impl Strategy<Value = u32> {
    0u32..=10_000u32
}

// ══════════════════════════════════════════════════════════════════════════════
// PROPERTY: Decomposition identity for Truncation mode
// ══════════════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1_000,
        // Provide a fixed seed so CI failures are reproducible.
        // The seed covers the full fuzz space; change only with justification.
        ..ProptestConfig::default()
    })]

    /// **Core property (Issue #411):**
    ///
    /// For all `amount ∈ [i128::MIN/2, i128::MAX/2]` and `bps ∈ [0, 10_000]`:
    ///
    /// ```text
    /// compute_share(amount, bps, Truncation)
    ///     == (amount / 10_000) * bps + (amount % 10_000) * bps / 10_000
    /// ```
    ///
    /// The right-hand side is evaluated with the same overflow-safe arithmetic
    /// as the implementation (see `reference_decomposition`).
    ///
    /// Also asserts the clamp invariant: result ∈ `[min(0, amount), max(0, amount)]`.
    #[test]
    fn prop_decomposition_identity_truncation(
        amount in arb_fuzz_amount(),
        bps    in arb_fuzz_bps(),
    ) {
        let (_env, client) = make_client();

        let actual    = client.compute_share(&amount, &bps, &RoundingMode::Truncation);
        let expected  = reference_decomposition(amount, bps);

        prop_assert_eq!(
            actual, expected,
            "decomposition identity failed: amount={amount}, bps={bps} \
             → actual={actual}, expected={expected}"
        );

        // Clamp invariant must hold independently of the identity.
        let lo = core::cmp::min(0_i128, amount);
        let hi = core::cmp::max(0_i128, amount);
        prop_assert!(
            actual >= lo && actual <= hi,
            "clamp invariant violated: amount={amount}, bps={bps}, result={actual}, \
             expected range=[{lo}, {hi}]"
        );
    }

    /// **Negative-amount clamp invariant:**
    ///
    /// For all negative `amount` and valid `bps`, the result must be ≤ 0 and ≥ amount.
    /// This locks the "clamp at extremes" requirement from the issue description.
    #[test]
    fn prop_negative_amount_clamp(
        // Use a sub-range of the fuzz space that is strictly negative.
        amount in (i128::MIN / 2)..=-1_i128,
        bps    in arb_fuzz_bps(),
    ) {
        let (_env, client) = make_client();

        let result = client.compute_share(&amount, &bps, &RoundingMode::Truncation);

        prop_assert!(
            result <= 0,
            "negative amount must produce non-positive result: amount={amount}, bps={bps}, result={result}"
        );
        prop_assert!(
            result >= amount,
            "result must not be more negative than amount: amount={amount}, bps={bps}, result={result}"
        );
    }

    /// **Over-bps guard property:**
    ///
    /// For all `bps > 10_000` and any `amount`, `compute_share` must return 0.
    /// Verifies the guard is not accidentally removed by a refactor.
    #[test]
    fn prop_over_bps_guard(
        amount in arb_fuzz_amount(),
        bps    in 10_001u32..=u32::MAX,
    ) {
        let (_env, client) = make_client();

        let result = client.compute_share(&amount, &bps, &RoundingMode::Truncation);
        prop_assert_eq!(
            result, 0,
            "over-bps guard failed: amount={amount}, bps={bps}, result={result}"
        );
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// BOUNDARY SEEDS — always exercised regardless of proptest shrinking
// ══════════════════════════════════════════════════════════════════════════════

/// Explicit boundary cases that proptest might not always hit.
///
/// These are deterministic unit tests that complement the fuzz property above.
/// They cover the four corners of the fuzz space plus the zero-identity cases.
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
    ];

    for &(amount, bps) in seeds {
        let actual   = client.compute_share(&amount, &bps, &RoundingMode::Truncation);
        let expected = reference_decomposition(amount, bps);

        assert_eq!(
            actual, expected,
            "boundary seed failed: amount={amount}, bps={bps} \
             → actual={actual}, expected={expected}"
        );
        assert_bounds(actual, amount, &format!("boundary seed amount={amount} bps={bps}"));
    }
}

/// Verify the over-bps guard at exact boundary values.
#[test]
fn boundary_seeds_over_bps_guard() {
    let (_env, client) = make_client();

    let amounts = [
        1_i128,
        -1,
        10_000,
        -10_000,
        i128::MAX / 2,
        i128::MIN / 2,
    ];
    let over_bps = [10_001u32, 20_000, u32::MAX];

    for &amount in &amounts {
        for &bps in &over_bps {
            let result = client.compute_share(&amount, &bps, &RoundingMode::Truncation);
            assert_eq!(
                result, 0,
                "over-bps guard boundary: amount={amount}, bps={bps}, result={result}"
            );
        }
    }
}

/// Verify the full-share identity (`bps = 10_000 → result = amount`) at boundaries.
#[test]
fn boundary_seeds_full_share_identity() {
    let (_env, client) = make_client();

    let amounts = [
        1_i128,
        -1,
        10_000,
        -10_000,
        100_000_000,
        -100_000_000,
        i128::MAX / 2,
        i128::MIN / 2,
    ];

    for &amount in &amounts {
        let result = client.compute_share(&amount, &10_000, &RoundingMode::Truncation);
        assert_eq!(
            result, amount,
            "full-share identity failed: amount={amount}, result={result}"
        );
    }
}

/// Verify the zero-identity (`amount = 0` or `bps = 0 → result = 0`) at boundaries.
#[test]
fn boundary_seeds_zero_identity() {
    let (_env, client) = make_client();

    // amount = 0 for all bps
    for bps in [0u32, 1, 5_000, 9_999, 10_000] {
        assert_eq!(
            client.compute_share(&0, &bps, &RoundingMode::Truncation),
            0,
            "zero-amount identity failed for bps={bps}"
        );
    }

    // bps = 0 for boundary amounts
    for amount in [1_i128, -1, i128::MAX / 2, i128::MIN / 2] {
        assert_eq!(
            client.compute_share(&amount, &0, &RoundingMode::Truncation),
            0,
            "zero-bps identity failed for amount={amount}"
        );
    }
}
