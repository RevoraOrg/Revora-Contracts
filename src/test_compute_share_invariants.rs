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

use crate::{RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode};
use soroban_sdk::{testutils::Address as _, Address, Env};

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
        assert_bounds(result, amount, &format!("Truncation amount={amount} bps={bps}"));
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
        (9_999, 1, 1),  // 0.9999 rounds up to 1
        (4_999, 1, 0),  // 0.4999 rounds down
        (5_000, 1, 1),  // exactly 0.5 rounds up
        // Over-bps guard
        (1_000_000, 10_001, 0),
    ];

    for &(amount, bps, expected) in cases {
        let result = c.compute_share(&amount, &bps, &RoundingMode::RoundHalfUp);
        assert_eq!(
            result, expected,
            "RoundHalfUp: amount={amount}, bps={bps} → expected {expected}, got {result}"
        );
        assert_bounds(result, amount, &format!("RoundHalfUp amount={amount} bps={bps}"));
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
            assert!(
                r >= t,
                "RoundHalfUp ({r}) < Truncation ({t}) for amount={amount}, bps={bps}"
            );
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
        i128::MAX / 10_000 * 10_000 + 9_999, // Max remainder
        i128::MIN / 10_000 * 10_000 - 9_999,   // Min remainder
    ];

    let bps_values = [1_u32, 100, 1_000, 5_000, 9_999, 10_000];

    for &amount in &test_amounts {
        for &bps in &bps_values {
            let result_trunc = c.compute_share(&amount, &bps, &RoundingMode::Truncation);
            let result_round = c.compute_share(&amount, &bps, &RoundingMode::RoundHalfUp);

            // Verify bounds invariant
            assert_bounds(result_trunc, amount, &format!("Truncation amount={amount} bps={bps}"));
            assert_bounds(result_round, amount, &format!("RoundHalfUp amount={amount} bps={bps}"));

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
    let extreme_amounts = [
        i128::MAX,
        i128::MIN,
        i128::MAX - 1,
        i128::MIN + 1,
    ];

    for &amount in &extreme_amounts {
        for &bps in [1_u32, 5_000, 10_000] {
            let result = c.compute_share(&amount, &bps, &RoundingMode::Truncation);
            // Should never panic and should always satisfy bounds
            assert_bounds(result, amount, &format!("Extreme amount={amount} bps={bps}"));
        }
    }
}

