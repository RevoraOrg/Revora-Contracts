#![cfg(test)]
//! Adversarial coverage for `compute_share` — the pure share-math helper.
//!
//! Complements `test_compute_share_invariants` by targeting the *contract*
//! surface rather than the invariant sweep:
//!   - the `revenue_share_bps > 10_000` guard, including `u32::MAX`;
//!   - deterministic divergence between `Truncation` and `RoundHalfUp` at the
//!     exact half boundary;
//!   - sign symmetry under truncation;
//!   - monotonicity in `bps` for positive amounts;
//!   - and the authorization/state contract: `compute_share` is a pure query
//!     that requires no auth and mutates no state.

extern crate std;

use super::*;
use crate::{RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode};
use soroban_sdk::Env;

fn client() -> (Env, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let c = RevoraRevenueShareClient::new(&env, &id);
    (env, c)
}

// ── Over-bps guard ────────────────────────────────────────────────────────────

/// Any `bps` above 100% must collapse to 0 for every amount and rounding mode.
#[test]
fn over_bps_guard_returns_zero_including_u32_max() {
    let (_env, c) = client();

    for bps in [10_001_u32, 10_002, 65_535, u32::MAX] {
        for amount in [0_i128, 1, -1, 10_000, -10_000, i128::MAX, i128::MIN] {
            assert_eq!(
                c.compute_share(&amount, &bps, &RoundingMode::Truncation),
                0,
                "truncation amount={amount} bps={bps}"
            );
            assert_eq!(
                c.compute_share(&amount, &bps, &RoundingMode::RoundHalfUp),
                0,
                "round-half-up amount={amount} bps={bps}"
            );
        }
    }
}

// ── Boundary: exact half ──────────────────────────────────────────────────────

/// `amount = 10_001`, `bps = 5_000` is the smallest amount whose 50% share has a
/// fractional `.5`; truncation drops it, round-half-up keeps it.
#[test]
fn rounding_modes_diverge_at_exact_half_boundary() {
    let (_env, c) = client();

    assert_eq!(c.compute_share(&10_001, &5_000, &RoundingMode::Truncation), 5_000);
    assert_eq!(c.compute_share(&10_001, &5_000, &RoundingMode::RoundHalfUp), 5_001);

    // One unit below the boundary both modes agree.
    assert_eq!(c.compute_share(&10_000, &5_000, &RoundingMode::Truncation), 5_000);
    assert_eq!(c.compute_share(&10_000, &5_000, &RoundingMode::RoundHalfUp), 5_000);
}

/// Negative half boundaries mirror the positive ones, rounding away from zero.
#[test]
fn negative_half_boundaries_round_away_from_zero() {
    let (_env, c) = client();

    assert_eq!(c.compute_share(&-10_001, &5_000, &RoundingMode::Truncation), -5_000);
    assert_eq!(c.compute_share(&-10_001, &5_000, &RoundingMode::RoundHalfUp), -5_001);
    assert_eq!(c.compute_share(&-10_000, &5_000, &RoundingMode::RoundHalfUp), -5_000);
}

// ── Zero identity & full share ───────────────────────────────────────────────

/// Zero amount or zero bps is always zero, for both modes.
#[test]
fn zero_identity_holds_for_both_modes() {
    let (_env, c) = client();

    for mode in [RoundingMode::Truncation, RoundingMode::RoundHalfUp] {
        for bps in [0_u32, 1, 5_000, 10_000] {
            assert_eq!(c.compute_share(&0, &bps, &mode), 0, "amount=0 bps={bps}");
        }
        for amount in [0_i128, 1, -1, i128::MAX, i128::MIN] {
            assert_eq!(c.compute_share(&amount, &0, &mode), 0, "bps=0 amount={amount}");
        }
    }
}

/// A 100% (`10_000` bps) share returns the amount unchanged for both modes.
#[test]
fn full_share_returns_amount_unchanged() {
    let (_env, c) = client();

    for amount in [1_i128, -1, 7, -7, 999_999, i128::MAX, i128::MIN] {
        assert_eq!(c.compute_share(&amount, &10_000, &RoundingMode::Truncation), amount);
        assert_eq!(c.compute_share(&amount, &10_000, &RoundingMode::RoundHalfUp), amount);
    }
}

// ── Determinism, symmetry, monotonicity ──────────────────────────────────────

/// Repeated calls with identical inputs return identical results (pure function).
#[test]
fn results_are_deterministic_across_calls() {
    let (_env, c) = client();

    for (amount, bps) in [(10_001_i128, 5_000_u32), (-10_001, 3_333), (i128::MAX, 9_999)] {
        let first = c.compute_share(&amount, &bps, &RoundingMode::RoundHalfUp);
        let second = c.compute_share(&amount, &bps, &RoundingMode::RoundHalfUp);
        assert_eq!(first, second, "amount={amount} bps={bps}");
    }
}

/// Under truncation the helper is odd-symmetric: `f(-a) == -f(a)`.
#[test]
fn truncation_is_odd_symmetric() {
    let (_env, c) = client();

    for amount in [0_i128, 1, 3, 10_000, 10_001, 12_345_678, 99_999_999] {
        for bps in [1_u32, 2_500, 5_000, 7_777, 9_999, 10_000] {
            let positive = c.compute_share(&amount, &bps, &RoundingMode::Truncation);
            let negative = c.compute_share(&-amount, &bps, &RoundingMode::Truncation);
            assert_eq!(negative, -positive, "amount={amount} bps={bps}");
        }
    }
}

/// For a positive amount the share never decreases as `bps` grows.
#[test]
fn share_is_monotonic_non_decreasing_in_bps() {
    let (_env, c) = client();

    for amount in [1_i128, 9_999, 10_000, 10_001, 1_000_000] {
        let mut previous = i128::MIN;
        for bps in [0_u32, 1, 100, 1_000, 5_000, 9_999, 10_000] {
            let value = c.compute_share(&amount, &bps, &RoundingMode::Truncation);
            assert!(value >= previous, "amount={amount} bps={bps} value={value} prev={previous}");
            previous = value;
        }
    }
}

// ── Authorization & state contract ───────────────────────────────────────────

/// `compute_share` is a pure query: it must work on a fresh, un-initialized
/// contract with no auth mocking, and must not touch pause state.
#[test]
fn requires_no_authorization_and_leaves_state_untouched() {
    // Deliberately NO `mock_all_auths()` — a pure helper must not need auth.
    let env = Env::default();
    let id = env.register_contract(None, RevoraRevenueShare);
    let c = RevoraRevenueShareClient::new(&env, &id);

    let before = c.is_paused();
    let share = c.compute_share(&1_000_000, &5_000, &RoundingMode::Truncation);
    let after = c.is_paused();

    assert_eq!(share, 500_000);
    assert!(!before);
    assert_eq!(before, after, "compute_share must not mutate pause state");
}
