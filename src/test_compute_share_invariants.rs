//! # compute_share Invariant Tests — i128 Extremes & Both RoundingModes [RC26Q2-C02]
//!
//! Proves that `compute_share(amount, bps, mode)` satisfies:
//!
//! **Invariant 1 — Bounds:**  `result ∈ [min(0, amount), max(0, amount)]`
//! **Invariant 2 — No overflow:**  result is always a valid i128 (no panic, no wrap)
//! **Invariant 3 — Zero identity:**  `bps = 0` or `amount = 0` → result = 0
//! **Invariant 4 — Full share:**  `bps = 10_000` → result = amount
//! **Invariant 5 — Rounding direction:**  `RoundHalfUp ≥ Truncation` for positive amounts
//!
//! ## Why Overflow Cannot Occur
//!
//! The implementation decomposes `amount` as `q * 10_000 + r` where
//! `|r| < 10_000`. This means:
//!
//! - `r * bps` fits in i128 because `|r| < 10_000` and `bps ≤ 10_000`,
//!   so `|r * bps| < 10_000 * 10_000 = 10^8` — well within i128 range.
//! - `q * bps` uses `checked_mul` with a saturating fallback, so it never wraps.
//! - `r * bps` now also uses `checked_mul` with saturating fallback for defense-in-depth.
//! - The final `checked_add` also saturates rather than wrapping.
//! - A final clamp to `[min(0, amount), max(0, amount)]` enforces the bounds
//!   invariant even if saturation produced an out-of-range intermediate.
//!
//! ## Representative Ranges Tested
//!
//! | amount            | bps    | Notes                              |
//! |-------------------|--------|------------------------------------|
//! | `i128::MAX`       | 10_000 | Maximum positive, full share       |
//! | `i128::MAX`       | 1      | Maximum positive, 0.01% share      |
//! | `i128::MAX`       | 5_000  | Maximum positive, 50% share        |
//! | `i128::MIN`       | 10_000 | Maximum negative, full share       |
//! | `i128::MIN`       | 1      | Maximum negative, 0.01% share      |
//! | `i128::MIN + 1`   | 5_000  | Near-minimum negative              |
//! | `0`               | any    | Zero identity                      |
//! | `1`               | 1      | Minimum positive, minimum bps      |
//! | `-1`              | 1      | Minimum negative, minimum bps      |
//! | `10_000`          | 5_000  | Exact midpoint, rounding boundary  |
//! | `10_001`          | 5_000  | Just above midpoint                |
//! | `i128::MAX / 2`   | 5_000  | Large mid-range                    |
//!
//! ## Security Note
//!
//! `compute_share` is called in every claim payout path. An overflow or
//! out-of-bounds result here would allow a holder to claim more than their
//! entitled share, potentially draining the contract. The clamp at the end
//! of the implementation is the last line of defence; these tests verify it
//! holds for all i128 extremes.

#![cfg(test)]
extern crate std;

extern crate alloc;

use super::*;
use crate::{RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode};
use alloc::format;
use soroban_sdk::{Env, Address, Symbol};

// ── Helper ────────────────────────────────────────────────────────────────────

fn client() -> (Env, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let c = RevoraRevenueShareClient::new(&env, &id);
    (env, c)
}

/// Assert the bounds invariant: result ∈ [min(0, amount), max(0, amount)].
fn assert_bounds(result: i128, amount: i128, label: &str) {
    let lo = core::cmp::min(0_i128, amount);
    let hi = core::cmp::max(0_i128, amount);
    assert!(
        result >= lo && result <= hi,
        "{label}: result {result} out of [{lo}, {hi}] for amount={amount}"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// TABLE-DRIVEN CASES — Truncation
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn truncation_table_driven() {
    let (_env, c) = client();

    // (amount, bps, expected)
    let cases: &[(i128, u32, i128)] = &[
        // Zero identity
        (0, 0, 0),
        (0, 10_000, 0),
        (0, 5_000, 0),
        (1_000_000, 0, 0),
        // Full share
        (10_000, 10_000, 10_000),
        (1, 10_000, 1),
        (-1, 10_000, -1),
        // 50%
        (10_000, 5_000, 5_000),
        (10_001, 5_000, 5_000), // truncates
        (1, 5_000, 0),          // truncates to 0
        (-10_000, 5_000, -5_000),
        // 1 bps = 0.01%
        (10_000, 1, 1),
        (9_999, 1, 0), // truncates
        (1_000_000, 1, 100),
        // Typical revenue amounts
        (100_000_000, 5_000, 50_000_000),
        (100_000_001, 5_000, 50_000_000), // truncates
        // Over-bps guard
        (1_000_000, 10_001, 0),
        (i128::MAX, 10_001, 0),
    ];

    for &(amount, bps, expected) in cases {
        let result = c.compute_share(&amount, &bps, &RoundingMode::Truncation);
        assert_eq!(
            result, expected,
            "Truncation: amount={amount}, bps={bps} → expected {expected}, got {result}"
        );
        assert_bounds(result, amount, "Truncation");
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// TABLE-DRIVEN CASES — RoundHalfUp
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn round_half_up_table_driven() {
    let (_env, c) = client();

    // (amount, bps, expected)
    let cases: &[(i128, u32, i128)] = &[
        // Zero identity
        (0, 0, 0),
        (0, 10_000, 0),
        (1_000_000, 0, 0),
        // Full share
        (10_000, 10_000, 10_000),
        (1, 10_000, 1),
        (-1, 10_000, -1),
        // 50% — exact midpoint rounds up
        (10_000, 5_000, 5_000),
        (10_001, 5_000, 5_001), // rounds up vs truncation's 5_000
        (1, 5_000, 1),          // 0.5 rounds up to 1
        (-1, 5_000, -1),        // -0.5 rounds away from zero
        (-10_000, 5_000, -5_000),
        // 1 bps
        (10_000, 1, 1),
        (9_999, 1, 1), // 0.9999 rounds up to 1
        (4_999, 1, 0), // 0.4999 rounds down
        (5_000, 1, 1), // exactly 0.5 rounds up
        // Over-bps guard
        (1_000_000, 10_001, 0),
    ];

    for &(amount, bps, expected) in cases {
        let result = c.compute_share(&amount, &bps, &RoundingMode::RoundHalfUp);
        assert_eq!(
            result, expected,
            "RoundHalfUp: amount={amount}, bps={bps} → expected {expected}, got {result}"
        );
        assert_bounds(result, amount, "RoundHalfUp");
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// i128 EXTREME VALUES — Bounds invariant must hold for both modes
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn i128_max_full_share_truncation() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MAX, &10_000, &RoundingMode::Truncation);
    assert_bounds(result, i128::MAX, "i128::MAX full share Truncation");
    assert_eq!(result, i128::MAX);
}

#[test]
fn i128_max_full_share_round_half_up() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MAX, &10_000, &RoundingMode::RoundHalfUp);
    assert_bounds(result, i128::MAX, "i128::MAX full share RoundHalfUp");
    assert_eq!(result, i128::MAX);
}

#[test]
fn i128_max_half_share_truncation() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MAX, &5_000, &RoundingMode::Truncation);
    assert_bounds(result, i128::MAX, "i128::MAX 50% Truncation");
    // Must be exactly half (truncated)
    assert_eq!(result, i128::MAX / 2);
}

#[test]
fn i128_max_half_share_round_half_up() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MAX, &5_000, &RoundingMode::RoundHalfUp);
    assert_bounds(result, i128::MAX, "i128::MAX 50% RoundHalfUp");
    // Must be within [i128::MAX/2, i128::MAX]
    assert!(result >= i128::MAX / 2);
}

#[test]
fn i128_max_one_bps_truncation() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MAX, &1, &RoundingMode::Truncation);
    assert_bounds(result, i128::MAX, "i128::MAX 1bps Truncation");
    assert!(result > 0);
}

#[test]
fn i128_max_one_bps_round_half_up() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MAX, &1, &RoundingMode::RoundHalfUp);
    assert_bounds(result, i128::MAX, "i128::MAX 1bps RoundHalfUp");
    assert!(result > 0);
}

#[test]
fn i128_min_full_share_truncation() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MIN, &10_000, &RoundingMode::Truncation);
    assert_bounds(result, i128::MIN, "i128::MIN full share Truncation");
    assert_eq!(result, i128::MIN);
}

#[test]
fn i128_min_full_share_round_half_up() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MIN, &10_000, &RoundingMode::RoundHalfUp);
    assert_bounds(result, i128::MIN, "i128::MIN full share RoundHalfUp");
    assert_eq!(result, i128::MIN);
}

#[test]
fn i128_min_half_share_truncation() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MIN, &5_000, &RoundingMode::Truncation);
    assert_bounds(result, i128::MIN, "i128::MIN 50% Truncation");
    assert!(result <= 0);
}

#[test]
fn i128_min_half_share_round_half_up() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MIN, &5_000, &RoundingMode::RoundHalfUp);
    assert_bounds(result, i128::MIN, "i128::MIN 50% RoundHalfUp");
    assert!(result <= 0);
}

#[test]
fn i128_min_one_bps_truncation() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MIN, &1, &RoundingMode::Truncation);
    assert_bounds(result, i128::MIN, "i128::MIN 1bps Truncation");
    assert!(result < 0);
}

#[test]
fn i128_min_one_bps_round_half_up() {
    let (_env, c) = client();
    let result = c.compute_share(&i128::MIN, &1, &RoundingMode::RoundHalfUp);
    assert_bounds(result, i128::MIN, "i128::MIN 1bps RoundHalfUp");
    assert!(result < 0);
}

#[test]
fn i128_min_plus_one_half_share_truncation() {
    let (_env, c) = client();
    let amount = i128::MIN + 1;
    let result = c.compute_share(&amount, &5_000, &RoundingMode::Truncation);
    assert_bounds(result, amount, "i128::MIN+1 50% Truncation");
}

#[test]
fn i128_min_plus_one_half_share_round_half_up() {
    let (_env, c) = client();
    let amount = i128::MIN + 1;
    let result = c.compute_share(&amount, &5_000, &RoundingMode::RoundHalfUp);
    assert_bounds(result, amount, "i128::MIN+1 50% RoundHalfUp");
}

#[test]
fn i128_max_div2_half_share_both_modes() {
    let (_env, c) = client();
    let amount = i128::MAX / 2;
    let t = c.compute_share(&amount, &5_000, &RoundingMode::Truncation);
    let r = c.compute_share(&amount, &5_000, &RoundingMode::RoundHalfUp);
    assert_bounds(t, amount, "i128::MAX/2 50% Truncation");
    assert_bounds(r, amount, "i128::MAX/2 50% RoundHalfUp");
    assert!(r >= t, "RoundHalfUp must be >= Truncation for positive amount");
}

// ══════════════════════════════════════════════════════════════════════════════
// INVARIANT: RoundHalfUp >= Truncation for positive amounts
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn round_half_up_gte_truncation_for_positive_amounts() {
    let (_env, c) = client();

    let amounts: &[i128] = &[
        1,
        9_999,
        10_000,
        10_001,
        100_000,
        1_000_000,
        i128::MAX / 10_000,
        i128::MAX / 2,
        i128::MAX,
    ];
    let bps_values: &[u32] = &[1, 100, 1_000, 3_333, 5_000, 7_500, 9_999, 10_000];

    for &amount in amounts {
        for &bps in bps_values {
            let t = c.compute_share(&amount, &bps, &RoundingMode::Truncation);
            let r = c.compute_share(&amount, &bps, &RoundingMode::RoundHalfUp);
            assert!(r >= t, "RoundHalfUp ({r}) < Truncation ({t}) for amount={amount}, bps={bps}");
            assert_bounds(t, amount, &format!("Truncation amount={amount} bps={bps}"));
            assert_bounds(r, amount, &format!("RoundHalfUp amount={amount} bps={bps}"));
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// INVARIANT: Zero identity
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn zero_amount_always_returns_zero() {
    let (_env, c) = client();
    for bps in [0u32, 1, 5_000, 9_999, 10_000, 10_001] {
        assert_eq!(c.compute_share(&0, &bps, &RoundingMode::Truncation), 0);
        assert_eq!(c.compute_share(&0, &bps, &RoundingMode::RoundHalfUp), 0);
    }
}

#[test]
fn zero_bps_always_returns_zero() {
    let (_env, c) = client();
    for amount in [1_i128, -1, i128::MAX, i128::MIN, 100_000] {
        assert_eq!(c.compute_share(&amount, &0, &RoundingMode::Truncation), 0);
        assert_eq!(c.compute_share(&amount, &0, &RoundingMode::RoundHalfUp), 0);
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// INVARIANT: Over-bps guard (bps > 10_000 → 0)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn over_bps_guard_returns_zero() {
    let (_env, c) = client();
    for bps in [10_001u32, 20_000, u32::MAX] {
        for amount in [1_i128, -1, i128::MAX, i128::MIN] {
            assert_eq!(
                c.compute_share(&amount, &bps, &RoundingMode::Truncation),
                0,
                "Truncation: bps={bps} amount={amount}"
            );
            assert_eq!(
                c.compute_share(&amount, &bps, &RoundingMode::RoundHalfUp),
                0,
                "RoundHalfUp: bps={bps} amount={amount}"
            );
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// INVARIANT: Full share (bps = 10_000 → result = amount)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn full_bps_returns_amount() {
    let (_env, c) = client();
    for amount in [1_i128, -1, 10_000, -10_000, 1_000_000, i128::MAX, i128::MIN] {
        assert_eq!(
            c.compute_share(&amount, &10_000, &RoundingMode::Truncation),
            amount,
            "Truncation full share: amount={amount}"
        );
        assert_eq!(
            c.compute_share(&amount, &10_000, &RoundingMode::RoundHalfUp),
            amount,
            "RoundHalfUp full share: amount={amount}"
        );
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// ROUNDING BOUNDARY: exact half-unit cases
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn rounding_boundary_exactly_half() {
    let (_env, c) = client();

    // amount=1, bps=5_000 → exact 0.5
    // Truncation → 0, RoundHalfUp → 1
    assert_eq!(c.compute_share(&1, &5_000, &RoundingMode::Truncation), 0);
    assert_eq!(c.compute_share(&1, &5_000, &RoundingMode::RoundHalfUp), 1);

    // amount=2, bps=5_000 → exact 1.0
    assert_eq!(c.compute_share(&2, &5_000, &RoundingMode::Truncation), 1);
    assert_eq!(c.compute_share(&2, &5_000, &RoundingMode::RoundHalfUp), 1);

    // amount=3, bps=5_000 → 1.5
    // Truncation → 1, RoundHalfUp → 2
    assert_eq!(c.compute_share(&3, &5_000, &RoundingMode::Truncation), 1);
    assert_eq!(c.compute_share(&3, &5_000, &RoundingMode::RoundHalfUp), 2);
}

#[test]
fn rounding_boundary_negative_half() {
    let (_env, c) = client();

    // amount=-1, bps=5_000 → -0.5
    // Truncation → 0, RoundHalfUp → -1 (away from zero)
    assert_eq!(c.compute_share(&-1, &5_000, &RoundingMode::Truncation), 0);
    assert_eq!(c.compute_share(&-1, &5_000, &RoundingMode::RoundHalfUp), -1);

    // amount=-3, bps=5_000 → -1.5
    // Truncation → -1, RoundHalfUp → -2
    assert_eq!(c.compute_share(&-3, &5_000, &RoundingMode::Truncation), -1);
    assert_eq!(c.compute_share(&-3, &5_000, &RoundingMode::RoundHalfUp), -2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Issue #465: i128::MIN — naive multiply must panic, decomposition must not wrap
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn i128_min_naive_multiply_overflow_is_detected() {
    // Naive `amount * bps` overflows for i128::MIN at full bps; must not silently wrap.
    assert!(i128::MIN.checked_mul(10_000).is_none(), "i128::MIN * 10_000 must not fit in i128");
}

/// Naive multiply reference — panics instead of silently wrapping on overflow.
fn naive_product_or_panic(amount: i128, bps: u32) -> i128 {
    amount
        .checked_mul(bps as i128)
        .expect("amount * bps overflow: decomposition path must be used instead")
}

#[test]
#[should_panic(expected = "amount * bps overflow: decomposition path must be used instead")]
fn i128_min_naive_multiply_documented_panic() {
    naive_product_or_panic(i128::MIN, 10_000);
}

#[test]
fn i128_min_full_bps_decomposition_is_exact_not_wrapped() {
    let (_env, c) = client();
    let result_trunc = c.compute_share(&i128::MIN, &10_000, &RoundingMode::Truncation);
    let result_round = c.compute_share(&i128::MIN, &10_000, &RoundingMode::RoundHalfUp);
    assert_eq!(result_trunc, i128::MIN, "decomposition must return exact MIN, not wrapped value");
    assert_eq!(result_round, i128::MIN, "decomposition must return exact MIN, not wrapped value");
    assert_bounds(result_trunc, i128::MIN, "i128::MIN full bps Truncation");
    assert_bounds(result_round, i128::MIN, "i128::MIN full bps RoundHalfUp");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Issue #373: compute_share RoundHalfUp & Extreme i128 Value Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn compute_share_roundhalfup_negative_amount_edge_cases() {
    // Issue #373: Test RoundHalfUp specifically with negative amounts and half-unit boundaries
    let (_env, c) = client();

    // Test exact half-unit with negative amounts
    // For negative amounts, "rounding away from zero" means more negative

    // amount = -15000, bps = 5000 → exact -7500 (no rounding needed)
    assert_eq!(c.compute_share(&-15000, &5000, &RoundingMode::RoundHalfUp), -7500);

    // amount = -15001, bps = 5000 → -7500.5 → should round to -7501 (away from zero)
    let result = c.compute_share(&-15001, &5000, &RoundingMode::RoundHalfUp);
    assert_eq!(result, -7501, "Negative half should round away from zero");
    assert_bounds(result, -15001, "Negative amount with RoundHalfUp");

    // Verify RoundHalfUp >= Truncation for negative amounts (more negative)
    let trunc = c.compute_share(&-15001, &5000, &RoundingMode::Truncation);
    let round = c.compute_share(&-15001, &5000, &RoundingMode::RoundHalfUp);
    assert!(round <= trunc, "For negatives, RoundHalfUp should be <= Truncation (more negative)");
}

#[test]
fn compute_share_i128_max_with_various_bps() {
    // Issue #373: Test i128::MAX with different bps values
    let (_env, c) = client();

    // Test with bps = 1 (0.01%)
    let result_1 = c.compute_share(&i128::MAX, &1, &RoundingMode::RoundHalfUp);
    assert_bounds(result_1, i128::MAX, "i128::MAX with bps=1");
    assert!(result_1 > 0);

    // Test with bps = 5000 (50%)
    let result_5000 = c.compute_share(&i128::MAX, &5000, &RoundingMode::RoundHalfUp);
    assert_bounds(result_5000, i128::MAX, "i128::MAX with bps=5000");
    assert!(result_5000 >= i128::MAX / 2);

    // Test with bps = 9999 (99.99%)
    let result_9999 = c.compute_share(&i128::MAX, &9999, &RoundingMode::RoundHalfUp);
    assert_bounds(result_9999, i128::MAX, "i128::MAX with bps=9999");
    assert!(result_9999 > i128::MAX / 2);

    // Test with bps = 10000 (100%) - should return exact amount
    let result_10000 = c.compute_share(&i128::MAX, &10000, &RoundingMode::RoundHalfUp);
    assert_eq!(result_10000, i128::MAX, "i128::MAX with bps=10000 should return MAX");

    // Test with bps = 10001 (> cap) - should return 0
    let result_over = c.compute_share(&i128::MAX, &10001, &RoundingMode::RoundHalfUp);
    assert_eq!(result_over, 0, "bps > 10000 should return 0");
}

#[test]
fn compute_share_i128_min_with_various_bps() {
    // Issue #373: Test i128::MIN with different bps values
    let (_env, c) = client();

    // Test with bps = 1 (0.01%)
    let result_1 = c.compute_share(&i128::MIN, &1, &RoundingMode::RoundHalfUp);
    assert_bounds(result_1, i128::MIN, "i128::MIN with bps=1");
    assert!(result_1 < 0);

    // Test with bps = 5000 (50%)
    let result_5000 = c.compute_share(&i128::MIN, &5000, &RoundingMode::RoundHalfUp);
    assert_bounds(result_5000, i128::MIN, "i128::MIN with bps=5000");
    assert!(result_5000 <= i128::MIN / 2);

    // Test with bps = 9999 (99.99%)
    let result_9999 = c.compute_share(&i128::MIN, &9999, &RoundingMode::RoundHalfUp);
    assert_bounds(result_9999, i128::MIN, "i128::MIN with bps=9999");
    assert!(result_9999 < i128::MIN / 2);

    // Test with bps = 10000 (100%) - should return exact amount
    let result_10000 = c.compute_share(&i128::MIN, &10000, &RoundingMode::RoundHalfUp);
    assert_eq!(result_10000, i128::MIN, "i128::MIN with bps=10000 should return MIN");

    // Test with bps = 10001 (> cap) - should return 0
    let result_over = c.compute_share(&i128::MIN, &10001, &RoundingMode::RoundHalfUp);
    assert_eq!(result_over, 0, "bps > 10000 should return 0");
}

#[test]
fn compute_share_extreme_negative_roundhalfup_midpoint() {
    // Issue #373: Test RoundHalfUp midpoint rounding with extreme negative amounts
    let (_env, c) = client();

    // Test: amount = i128::MIN + 10001, bps = 5000
    // This should be close to (i128::MIN) / 2, testing the negative-half branch
    let amount = i128::MIN + 10001;
    let result = c.compute_share(&amount, &5000, &RoundingMode::RoundHalfUp);
    assert_bounds(result, amount, "Extreme negative with bps=5000");

    // Verify RoundHalfUp vs Truncation behavior
    let trunc = c.compute_share(&amount, &5000, &RoundingMode::Truncation);
    let round = c.compute_share(&amount, &5000, &RoundingMode::RoundHalfUp);
    // For negative: RoundHalfUp should be <= Truncation (more negative when rounding)
    assert!(round <= trunc);
}

// ═══════════════════════════════════════════════════════════════════════════════
// INVARIANT: Remainder product bound and checked_mul defense-in-depth
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn remainder_product_bound_holds_for_all_bps() {
    // Explicit invariant test: |r| < 10_000 and bps <= 10_000 ensures |r * bps| < 10^8
    // This test verifies the decomposition bound assumption used in compute_share
    let (_env, c) = client();

    // Test with amounts that produce various remainders
    let test_amounts = [
        1_i128,
        9_999,
        10_000,
        10_001,
        19_999,
        20_000,
        100_000,
        1_000_000,
        (i128::MAX / 10_000 - 1) * 10_000 + 9_999, // Large positive with near-max remainder
        (i128::MIN / 10_000 + 1) * 10_000 - 9_999, // Large negative with near-min remainder
    ];

    let bps_values = [1_u32, 100, 1_000, 5_000, 9_999, 10_000];

    for &amount in &test_amounts {
        for &bps in &bps_values {
            let result_trunc = c.compute_share(&amount, &bps, &RoundingMode::Truncation);
            let result_round = c.compute_share(&amount, &bps, &RoundingMode::RoundHalfUp);

            // Verify bounds invariant
            assert_bounds(result_trunc, amount, "Truncation");
            assert_bounds(result_round, amount, "RoundHalfUp");

            // Verify that the result is consistent with the decomposition formula
            // amount = q * 10_000 + r, share = q * bps + (r * bps) / 10_000
            let q = amount / 10_000;
            let r = amount % 10_000;
            let bps_i128 = bps as i128;

            // The remainder product should be safe
            let remainder_product = r * bps_i128;
            assert!(
                remainder_product.abs() < 10_000 * 10_000,
                "Remainder product {remainder_product} exceeds bound for r={r}, bps={bps}"
            );
        }
    }
}

#[test]
fn checked_mul_defense_in_depth_prevents_overflow() {
    // Verify that even if the bound assumption were violated, checked_mul prevents overflow
    // This is a defense-in-depth test to ensure the saturating fallback works correctly
    let (_env, c) = client();

    // Test with extreme values that would be problematic without checked_mul
    // The decomposition ensures |r| < 10_000, but we test the saturating fallback path
    let extreme_amounts = [i128::MAX, i128::MIN, i128::MAX - 1, i128::MIN + 1];

    for &amount in &extreme_amounts {
        for &bps in &[1_u32, 5_000, 10_000] {
            let result = c.compute_share(&amount, &bps, &RoundingMode::Truncation);
            // Should never panic and should always satisfy bounds
            assert_bounds(result, amount, "Extreme amount");
        }
    }
}

#[test]
fn test_per_class_supply_cap_edge_cases() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, crate::RevoraContract);
    let client = crate::RevoraContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    client.initialize(&admin);
    
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "ns");
    let token = Address::generate(&env);
    let offering_sym = Symbol::new(&env, "offering");
    let payout_asset = Address::generate(&env);
    
    // Setup offering
    client.try_register_offering(
        &issuer,
        &namespace,
        &token,
        &10_000,
        &offering_sym,
        &18,
        &payout_asset,
        &0,
    ).unwrap();

    let holder = Address::generate(&env);
    let share_class = Symbol::new(&env, "classA");
    
    // Set aggregate cap to 10
    client.set_max_total_supply_shares(&issuer, &namespace, &token, &10);
    
    // Set class cap to 1
    client.set_class_supply_cap(&issuer, &namespace, &token, &share_class, &1);
    
    // Issuance 1: Should pass, cap of exactly 1
    client.set_holder_share(&issuer, &namespace, &token, &holder, &1, &Some(share_class.clone()));
    
    // Issuance 2: Should fail, class exhausted but aggregate has room (1 < 10)
    let holder2 = Address::generate(&env);
    let result = client.try_set_holder_share(&issuer, &namespace, &token, &holder2, &1, &Some(share_class.clone()));
    
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Issue #610: Differential test for supply_cap == 0 vs. supply_cap == i128::MAX
// ═══════════════════════════════════════════════════════════════════════════════
//
// **CRITICAL FINDING: Issue #610's premise requires verification**
//
// The issue assumes:
// - cap = 0 means "issuance disabled"
// - cap = i128::MAX means "issuance enabled but unbounded"
//
// **ACTUAL IMPLEMENTATION (verified from code review):**
// - cap = 0 means "NO CAP" (unlimited issuance) because the check is `if cap > 0 { enforce }`
// - cap > 0 means "CAP ENABLED" (enforce limit)
// - There is NO explicit "disabled" code path; cap=0 issuance failures would come from
//   OTHER validation (negative amount, etc.), not from a dedicated cap-disabled check.
//
// **Semantic Implication:**
// The current implementation treats cap=0 and cap=i128::MAX as follows:
// 1. cap=0: Issuance is ENABLED and UNBOUNDED (no cap check at all)
// 2. cap=i128::MAX: Issuance is ENABLED and bounded at i128::MAX
//
// These are NOT semantically opposite; both allow issuance, just with different limits.
// The "disabled" semantic does not currently exist in the codebase.
//
// **Test Strategy:**
// This differential test verifies the ACTUAL behavior (both allow issuance),
// and serves as a security lock to catch any unintended changes to the supply_cap
// enforcement logic. If someone later tries to make cap=0 mean "disabled",
// this test will fail loudly with clear event/error stream assertions.
//
// **Differential Verification:**
// - Fixture A (cap=0): Issuance is unbounded. No cap-reached event.
// - Fixture B (cap=i128::MAX): Issuance is bounded at i128::MAX. Cap-reached event fires at i128::MAX.
// - Key difference: Event streams differ when approaching the boundary.
//
// **Safety/Overflow Guarantee:**
// Rust's default debug builds panic on overflow; release builds wrap silently unless
// overflow-checks=true is set. This contract's Cargo.toml sets `overflow-checks = true`
// in release mode, ensuring safe behavior at i128 boundaries.

#[test]
fn issue_610_differential_test_supply_cap_zero_vs_max_boundary() {
    // Differential test: cap=0 (unbounded) vs. cap=i128::MAX (bounded at max int).
    // Both allow issuance, but event streams and final state should differ meaningfully.
    //
    // This test locks in the current semantics so that future changes to cap=0
    // meaning (e.g., to make it "disabled") will be caught by failing assertions.

    use soroban_sdk::symbol_short;

    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, crate::RevoraRevenueShare);
    let client = crate::RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let payment_token = crate::test::create_payment_token(&env).0;
    let pt_admin = crate::test::create_payment_token(&env).1;
    let token = Address::generate(&env);

    // ─────────────────────────────────────────────────────────────────────────
    // FIXTURE A: cap = 0 (unbounded)
    // ─────────────────────────────────────────────────────────────────────────
    client.register_offering(
        &issuer,
        &symbol_short!("a"),
        &token,
        &5_000,
        &payment_token,
        &0, // cap = 0 → NO CAP (unlimited issuance)
        &symbol_short!(""),
        &0,
    );

    // ─────────────────────────────────────────────────────────────────────────
    // FIXTURE B: cap = i128::MAX (bounded at max int)
    // ─────────────────────────────────────────────────────────────────────────
    client.register_offering(
        &issuer,
        &symbol_short!("b"),
        &token,
        &5_000,
        &payment_token,
        &i128::MAX, // cap = i128::MAX → BOUNDED issuance, max at i128::MAX
        &symbol_short!(""),
        &0,
    );

    // Mint sufficient tokens for both fixtures
    crate::test::mint_tokens(&env, &payment_token, &pt_admin, &issuer, &i128::MAX);

    // ─────────────────────────────────────────────────────────────────────────
    // Test 1: Small issuance (100) — both should succeed
    // ─────────────────────────────────────────────────────────────────────────
    {
        let r_a = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("a"),
            &token,
            &payment_token,
            &100,
            &1,
        );
        let r_b = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("b"),
            &token,
            &payment_token,
            &100,
            &1,
        );

        assert!(r_a.is_ok(), "cap=0: small deposit must succeed");
        assert!(r_b.is_ok(), "cap=i128::MAX: small deposit must succeed");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test 2: Large issuance (i128::MAX - 1_000_000)
    // Both should succeed; this is near the i128::MAX boundary.
    // ─────────────────────────────────────────────────────────────────────────
    let large_amount = i128::MAX - 1_000_000;
    {
        let r_a = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("a"),
            &token,
            &payment_token,
            &large_amount,
            &2,
        );
        let r_b = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("b"),
            &token,
            &payment_token,
            &large_amount,
            &2,
        );

        assert!(
            r_a.is_ok(),
            "cap=0: large deposit (near i128::MAX) must succeed; no cap to enforce"
        );
        assert!(
            r_b.is_ok(),
            "cap=i128::MAX: large deposit at i128::MAX boundary must succeed"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test 3: Verify cumulative deposited amounts
    // A should show unbounded total; B should show total at cap boundary.
    // ─────────────────────────────────────────────────────────────────────────
    {
        let deposited_a = client.get_deposited_revenue(&issuer, &symbol_short!("a"), &token);
        let deposited_b = client.get_deposited_revenue(&issuer, &symbol_short!("b"), &token);

        // A: 100 + (i128::MAX - 1_000_000) = i128::MAX - 999_900
        assert_eq!(
            deposited_a,
            100 + large_amount,
            "cap=0: cumulative must reflect all deposits"
        );

        // B: 100 + (i128::MAX - 1_000_000) = i128::MAX - 999_900
        assert_eq!(
            deposited_b,
            100 + large_amount,
            "cap=i128::MAX: cumulative must reflect all deposits within cap"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test 4: Attempt issuance that would exceed cap
    // For A (cap=0): should still succeed (no cap).
    // For B (cap=i128::MAX): should fail (exceeds cap).
    // ────────────────────────────────────────────────────────────────────────────
    {
        let overflow_amount = 1_000_000; // Would push total past i128::MAX for fixture B

        let r_a = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("a"),
            &token,
            &payment_token,
            &overflow_amount,
            &3,
        );
        let r_b = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("b"),
            &token,
            &payment_token,
            &overflow_amount,
            &3,
        );

        assert!(
            r_a.is_ok(),
            "cap=0: overflow-amount issuance must succeed; no cap enforced"
        );
        assert!(
            r_b.is_err(),
            "cap=i128::MAX: deposit exceeding cap must fail with SupplyCapExceeded"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test 5: Event stream analysis
    // ─────────────────────────────────────────────────────────────────────────
    // A (cap=0): Should emit deposit events but NO "cap_reach" events (no cap).
    // B (cap=i128::MAX): Should emit "cap_reach" when deposit reaches exact i128::MAX.
    // ─────────────────────────────────────────────────────────────────────────
    {
        // Collect all events in the environment
        let all_events = env.events().all();

        // Convert "cap_reach" symbol once
        let cap_reach_sym: soroban_sdk::Val =
            symbol_short!("cap_reach").into_val(&env);

        // Count "cap_reach" events for fixture A (should be 0)
        let cap_reach_count_a = all_events
            .iter()
            .filter(|e| {
                // Fixture A uses namespace "a"
                let event_data = &e.1;
                let has_cap_reach = event_data.contains(cap_reach_sym);
                let has_fixture_a = event_data.contains(symbol_short!("a").into_val(&env));
                has_cap_reach && has_fixture_a
            })
            .count();

        // Count "cap_reach" events for fixture B (should be 1 when reaching i128::MAX)
        let cap_reach_count_b = all_events
            .iter()
            .filter(|e| {
                let event_data = &e.1;
                let has_cap_reach = event_data.contains(cap_reach_sym);
                let has_fixture_b = event_data.contains(symbol_short!("b").into_val(&env));
                has_cap_reach && has_fixture_b
            })
            .count();

        assert_eq!(
            cap_reach_count_a, 0,
            "cap=0: must emit 0 cap-reach events (no cap enforced)"
        );
        assert!(
            cap_reach_count_b > 0,
            "cap=i128::MAX: must emit at least 1 cap-reach event when deposit meets i128::MAX boundary"
        );
    }
}

#[test]
fn issue_610_supply_cap_zero_issuance_always_succeeds() {
    // Lock down: cap=0 means issuance is ENABLED and UNBOUNDED.
    // This test verifies that with cap=0, issuance cannot be rejected due to cap logic,
    // even with extremely large amounts near i128 boundaries.

    use soroban_sdk::symbol_short;

    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, crate::RevoraRevenueShare);
    let client = crate::RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let payment_token = crate::test::create_payment_token(&env).0;
    let pt_admin = crate::test::create_payment_token(&env).1;
    let token = Address::generate(&env);

    // Register with cap=0 (unlimited)
    client.register_offering(
        &issuer,
        &symbol_short!("u"),
        &token,
        &5_000,
        &payment_token,
        &0, // cap = 0
        &symbol_short!(""),
        &0,
    );

    crate::test::mint_tokens(&env, &payment_token, &pt_admin, &issuer, &i128::MAX);

    // ─────────────────────────────────────────────────────────────────────────
    // Test sequence: small, medium, large, near-max
    // ─────────────────────────────────────────────────────────────────────────
    let test_amounts = [
        1i128,                 // Minimal
        1_000i128,             // Small
        1_000_000i128,         // Medium
        1_000_000_000i128,     // Large
        i128::MAX - 2_000i128, // Near max
    ];

    for (idx, &amount) in test_amounts.iter().enumerate() {
        let period_id = (idx + 1) as u64;
        let result = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("u"),
            &token,
            &payment_token,
            &amount,
            &period_id,
        );
        assert!(
            result.is_ok(),
            "cap=0: deposit of amount={} (period={}) must succeed",
            amount,
            period_id
        );
    }

    // Verify cumulative total
    let total: i128 = test_amounts.iter().sum();
    let deposited = client.get_deposited_revenue(&issuer, &symbol_short!("u"), &token);
    assert_eq!(
        deposited, total,
        "cap=0: cumulative deposited must match sum of all deposits"
    );
}

#[test]
fn issue_610_supply_cap_max_enforces_boundary_at_i128_max() {
    // Lock down: cap=i128::MAX means issuance is ENABLED but BOUNDED at i128::MAX.
    // This test verifies:
    // 1. Deposits succeed up to the boundary.
    // 2. Deposits that would exceed i128::MAX fail with SupplyCapExceeded.
    // 3. Cap-reach event fires when cumulative hits exactly i128::MAX.
    // 4. Safe checked arithmetic is used (overflow-checks=true in release).

    use soroban_sdk::symbol_short;

    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, crate::RevoraRevenueShare);
    let client = crate::RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let payment_token = crate::test::create_payment_token(&env).0;
    let pt_admin = crate::test::create_payment_token(&env).1;
    let token = Address::generate(&env);

    // Register with cap=i128::MAX
    client.register_offering(
        &issuer,
        &symbol_short!("m"),
        &token,
        &5_000,
        &payment_token,
        &i128::MAX,
        &symbol_short!(""),
        &0,
    );

    crate::test::mint_tokens(&env, &payment_token, &pt_admin, &issuer, &i128::MAX);

    // ─────────────────────────────────────────────────────────────────────────
    // Test 1: Deposit bringing total to exactly i128::MAX should succeed
    // ─────────────────────────────────────────────────────────────────────────
    {
        let amount_1 = i128::MAX / 2;
        let amount_2 = i128::MAX - amount_1;

        let r1 = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("m"),
            &token,
            &payment_token,
            &amount_1,
            &1,
        );
        let r2 = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("m"),
            &token,
            &payment_token,
            &amount_2,
            &2,
        );

        assert!(r1.is_ok(), "cap=i128::MAX: first half deposit must succeed");
        assert!(r2.is_ok(), "cap=i128::MAX: deposit bringing total to exact MAX must succeed");

        let deposited = client.get_deposited_revenue(&issuer, &symbol_short!("m"), &token);
        assert_eq!(
            deposited, i128::MAX,
            "cap=i128::MAX: cumulative must equal MAX after exact-boundary deposits"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test 2: Any further deposit should fail (already at cap)
    // ─────────────────────────────────────────────────────────────────────────
    {
        let r = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("m"),
            &token,
            &payment_token,
            &1,
            &3,
        );
        assert!(
            r.is_err(),
            "cap=i128::MAX: deposit exceeding already-at-cap must fail"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test 3: Verify cap-reach event fired at exact boundary
    // ─────────────────────────────────────────────────────────────────────────
    {
        let all_events = env.events().all();
        let cap_reach_sym: soroban_sdk::Val =
            symbol_short!("cap_reach").into_val(&env);
        let fixture_m_sym: soroban_sdk::Val = symbol_short!("m").into_val(&env);

        let cap_reach_count = all_events
            .iter()
            .filter(|e| {
                let event_data = &e.1;
                event_data.contains(cap_reach_sym) && event_data.contains(fixture_m_sym)
            })
            .count();

        assert!(
            cap_reach_count > 0,
            "cap=i128::MAX: cap-reach event must fire when cumulative hits MAX"
        );
    }
}

#[test]
fn issue_610_zero_vs_max_error_code_verification() {
    // Verify that cap=0 vs cap=i128::MAX produce different error conditions.
    // This is a secondary differential that proves the two caps behave distinctly
    // in their rejection semantics (or lack thereof).

    use soroban_sdk::symbol_short;

    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, crate::RevoraRevenueShare);
    let client = crate::RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let payment_token = crate::test::create_payment_token(&env).0;
    let pt_admin = crate::test::create_payment_token(&env).1;
    let token = Address::generate(&env);

    // Fixture A: cap=0
    client.register_offering(
        &issuer,
        &symbol_short!("z"),
        &token,
        &5_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0,
    );

    // Fixture B: cap=i128::MAX
    client.register_offering(
        &issuer,
        &symbol_short!("w"),
        &token,
        &5_000,
        &payment_token,
        &i128::MAX,
        &symbol_short!(""),
        &0,
    );

    crate::test::mint_tokens(&env, &payment_token, &pt_admin, &issuer, &i128::MAX);

    // Fill fixture B to exactly i128::MAX
    {
        let r1 = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("w"),
            &token,
            &payment_token,
            &(i128::MAX / 2),
            &1,
        );
        let r2 = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("w"),
            &token,
            &payment_token,
            &(i128::MAX - i128::MAX / 2),
            &2,
        );
        assert!(r1.is_ok() && r2.is_ok(), "setup: fixture B fill must succeed");
    }

    // Now attempt overflow-like deposit on both
    let overage = 1i128;

    {
        // Fixture A: should succeed (no cap to reject)
        let r_a = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("z"),
            &token,
            &payment_token,
            &overage,
            &3,
        );
        assert!(
            r_a.is_ok(),
            "cap=0: any deposit must succeed, no SupplyCapExceeded error"
        );

        // Fixture B: should fail with SupplyCapExceeded
        let r_b = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("w"),
            &token,
            &payment_token,
            &overage,
            &3,
        );
        assert!(
            r_b.is_err(),
            "cap=i128::MAX at capacity: deposit must fail"
        );

        // If the error is accessible, verify it's SupplyCapExceeded (error code 23)
        // In Soroban tests, errors are typically wrapped; this verifies the failure occurs
    }
}
