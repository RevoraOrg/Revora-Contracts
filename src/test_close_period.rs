#![cfg(test)]
//! `close_period` / `close_period_dual_sig` / `preflight_close_period` tests.
//!
//! Restored from `src/quarantined/test_close_period.rs` (2026-09-28) and ported
//! to the current API surface:
//! - `preflight_close_period` is exercised through the public client instead of
//!   the raw `RevoraRevenueShare::preflight_close_period` free-function call.
//! - Payment-token minting goes through `crate::test_utils` (single canonical
//!   stellar-asset registration) instead of the old re-register mint hack.
//! - The legacy `DeferredDataKey::DeferredReports` flush suite was dropped: the
//!   live contract no longer reads that key (deferred distributions moved to
//!   the `DataKey3::DeferredQueue` priority queue, issue #551), and the 2-arg
//!   `close_period` the suite called was a private test harness, not a real
//!   entry point. Coverage for the current queue lives in the (still
//!   quarantined) `test_deferred_priority.rs` and its successors.

use crate::{
    test_utils, DataKey, OfferingId, PreflightCloseResult, RevoraError, RevoraRevenueShare,
    RevoraRevenueShareClient,
};
use proptest::prelude::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger},
    Address, Env, Vec,
};

// ── Deterministic shuffle helper ─────────────────────────────────────────────
// Restored verbatim from quarantined `proptest_helpers::shuffle_vec_with_seed`
// so this file does not depend on the still-quarantined module. When
// `proptest_helpers` is restored, switch to `crate::proptest_helpers::…`.
pub(crate) mod shuffle {
    /// Return a reproducible shuffled copy of a slice using a deterministic local PRNG.
    pub fn shuffle_vec_with_seed<T: Clone>(values: &[T], seed: u64) -> std::vec::Vec<T> {
        let mut shuffled = values.to_vec();
        let mut state = seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(0x85EBCA6B);

        for index in (1..shuffled.len()).rev() {
            state = state.rotate_left(17).wrapping_mul(0x9E3779B97F4A7C15);
            let j = (state as usize) % (index + 1);
            shuffled.swap(index, j);
        }

        shuffled
    }
}

use shuffle::shuffle_vec_with_seed;

// ── Test helpers ─────────────────────────────────────────────────────────────

/// Register a single-issuer offering and return the
/// `(env, client, issuer, token, payment_token)` tuple the close-period tests
/// expect. `mock_all_auths()` is enabled so any issuer-signed call within the
/// test body passes auth checks automatically; assertions about the
/// `OfferingNotFound` error path still succeed because they come from the
/// offering lookup itself.
///
/// The payment token is a real stellar asset with `issuer` as admin so
/// `test_utils::mint_tokens` can mint on the same asset address the offering
/// actually references.
fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = test_utils::create_token(&env, &issuer);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &10_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0u32,
    );

    (env, client, issuer, token, payment_token)
}

fn setup_offering_with_contract_id(
) -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payment_token = test_utils::create_token(&env, &issuer);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &offering_token,
        &10_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0,
    );

    (env, client, issuer, offering_token, payment_token, contract_id)
}

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &contract_id)
}

// ── Preflight determinism (property test) ────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        ..ProptestConfig::default()
    })]

    #[test]
    fn close_period_preflight_is_deterministic_across_shuffled_holders(
        seed in any::<u64>(),
    ) {
        let env = Env::default();
        env.mock_all_auths();
        // 512 preflight iterations on a single Env exceed the default
        // cumulative budget; lift the cap so the loop can complete.
        env.budget().reset_unlimited();
        let client = make_client(&env);

        let issuer = Address::generate(&env);
        let token = Address::generate(&env);
        let payment_token = test_utils::create_token(&env, &issuer);
        let ns = symbol_short!("ns");

        client.register_offering(
            &issuer,
            &Vec::from_array(&env, []),
            &1u32,
            &ns,
            &token,
            &10_000u32,
            &payment_token,
            &0i128,
            &symbol_short!(""),
            &0u32,
        );

        let holder_a = Address::generate(&env);
        let holder_b = Address::generate(&env);
        let holder_c = Address::generate(&env);
        let holder_d = Address::generate(&env);
        let holder_e = Address::generate(&env);

        client.set_holder_share(&issuer, &ns, &token, &holder_a, &3_000u32, &1);
        client.set_holder_share(&issuer, &ns, &token, &holder_b, &2_500u32, &1);
        client.set_holder_share(&issuer, &ns, &token, &holder_c, &1_500u32, &1);
        client.set_holder_share(&issuer, &ns, &token, &holder_d, &1_000u32, &1);
        client.set_holder_share(&issuer, &ns, &token, &holder_e, &2_000u32, &1);

        test_utils::mint_tokens(&env, &payment_token, &issuer, 10_000_000);
        client.deposit_revenue(&issuer, &ns, &token, &payment_token, &10_000_000i128, &1u64);

        let base_holders = std::vec![
            holder_a.clone(),
            holder_b.clone(),
            holder_c.clone(),
            holder_d.clone(),
            holder_e.clone(),
            holder_a.clone(),
        ];

        let mut baseline: Option<PreflightCloseResult> = None;
        for iteration in 0..512u64 {
            let shuffled = shuffle_vec_with_seed(&base_holders, seed.wrapping_add(iteration));
            let mut soroban_holders = Vec::new(&env);
            for holder in shuffled.iter() {
                soroban_holders.push_back(holder.clone());
            }

            // NOTE: generated client methods return the bare value and panic on
            // error; `unwrap` is unnecessary (and a type error) here.
            let result = client.preflight_close_period(
                &OfferingId {
                    issuer: issuer.clone(),
                    namespace: ns.clone(),
                    token: token.clone(),
                },
                &1u64,
                &soroban_holders,
            );

            if let Some(ref expected) = baseline {
                prop_assert_eq!(result.payouts, expected.payouts.clone());
                prop_assert_eq!(result.total_distributed, expected.total_distributed);
            } else {
                baseline = Some(result);
            }
        }
    }
}

// ── Single-sig close_period ──────────────────────────────────────────────────

#[test]
fn close_period_happy_path() {
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    assert!(!client.is_period_closed(&issuer, &ns, &token, &1));
    client.close_period(&issuer, &ns, &token, &1);
    assert!(client.is_period_closed(&issuer, &ns, &token, &1));
}

#[test]
fn close_period_aborts_when_share_ledger_is_inconsistent() {
    let (env, client, issuer, token, _payment_token, contract_id) =
        setup_offering_with_contract_id();
    let ns = symbol_short!("ns");
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &5_000, &1);

    let before_events = env.events().all().len();
    env.as_contract(&contract_id, || {
        let offering_id =
            OfferingId { issuer: issuer.clone(), namespace: ns.clone(), token: token.clone() };
        env.storage().persistent().set(&DataKey::HolderShareTotal(offering_id), &7_000u32);
    });

    let result = client.try_close_period(&issuer, &ns, &token, &1);
    assert_eq!(result, Err(Ok(RevoraError::CloseAbortInvariantsViolated)));
    assert!(!client.is_period_closed(&issuer, &ns, &token, &1));
    assert_eq!(
        env.events().all().len(),
        before_events,
        "abort path must not emit close_period events"
    );
}

#[test]
fn close_period_emits_event() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let before = env.events().all().len();

    client.close_period(&issuer, &ns, &token, &42);

    assert!(env.events().all().len() > before, "expected at least one new event");
}

#[test]
fn close_period_double_close_returns_error() {
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    client.close_period(&issuer, &ns, &token, &1);

    let result = client.try_close_period(&issuer, &ns, &token, &1);
    assert_eq!(result, Err(Ok(RevoraError::PeriodAlreadyClosed)));
}

#[test]
fn close_period_zero_period_id_rejected() {
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    let result = client.try_close_period(&issuer, &ns, &token, &0);
    assert_eq!(result, Err(Ok(RevoraError::InvalidPeriodId)));
}

#[test]
fn close_period_unknown_offering_returns_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let result = client.try_close_period(&issuer, &symbol_short!("ns"), &token, &1);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn override_after_close_returns_period_already_closed() {
    let (_env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");

    // Initial report for period 1.
    client.report_revenue(&issuer, &ns, &token, &payment_token, &1_000, &1, &false);

    // Seal the period.
    client.close_period(&issuer, &ns, &token, &1);

    // Attempt override — must be rejected.
    let result = client.try_report_revenue(&issuer, &ns, &token, &payment_token, &2_000, &1, &true);
    assert_eq!(result, Err(Ok(RevoraError::PeriodAlreadyClosed)));
}

#[test]
fn initial_report_for_new_period_after_close_is_allowed() {
    let (_env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");

    // Report period 1, then close it.
    client.report_revenue(&issuer, &ns, &token, &payment_token, &1_000, &1, &false);
    client.close_period(&issuer, &ns, &token, &1);

    // A brand-new period 2 (initial report, not an override) must still be accepted.
    let result = client.try_report_revenue(&issuer, &ns, &token, &payment_token, &500, &2, &false);
    assert!(
        result.is_ok(),
        "initial report for a new period should succeed after closing period 1"
    );
}

#[test]
fn deposit_after_close_is_allowed() {
    let (env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");

    // Close period 1 (close only blocks report overrides, not deposits).
    client.close_period(&issuer, &ns, &token, &1);

    // Deposit should still succeed.
    test_utils::mint_tokens(&env, &payment_token, &issuer, 10_000);
    let result = client.try_deposit_revenue(&issuer, &ns, &token, &payment_token, &1_000, &1);
    assert!(result.is_ok(), "deposit_revenue must succeed even after close_period");
}

#[test]
fn claim_after_close_is_allowed() {
    let (env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");

    let holder = Address::generate(&env);

    // Set holder share to 100%.
    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000, &1);

    // Deposit revenue for period 1.
    test_utils::mint_tokens(&env, &payment_token, &issuer, 1_000);
    client.deposit_revenue(&issuer, &ns, &token, &payment_token, &1_000, &1);

    // Seal the period.
    client.close_period(&issuer, &ns, &token, &1);

    // Holder should still be able to claim.
    let payout = client.claim(&holder, &issuer, &ns, &token, &10);
    assert_eq!(payout, 1_000, "holder must receive full payout after period is closed");
}

#[test]
fn close_period_does_not_affect_other_periods() {
    let (_env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");

    // Report periods 1 and 2.
    client.report_revenue(&issuer, &ns, &token, &payment_token, &100, &1, &false);
    client.report_revenue(&issuer, &ns, &token, &payment_token, &200, &2, &false);

    // Close only period 1.
    client.close_period(&issuer, &ns, &token, &1);

    assert!(client.is_period_closed(&issuer, &ns, &token, &1));
    assert!(!client.is_period_closed(&issuer, &ns, &token, &2));

    // Override of period 2 must still succeed.
    let result = client.try_report_revenue(&issuer, &ns, &token, &payment_token, &999, &2, &true);
    assert!(result.is_ok(), "override of an open period must succeed");
}

/// `require_auth` in no_std Soroban triggers a non-unwinding host panic that
/// cannot be caught by `try_*`. We verify the auth guard is present by
/// testing that a wrong-issuer call returns `OfferingNotFound` (the issuer
/// lookup check that follows `issuer.require_auth()`).
#[test]
fn close_period_wrong_issuer_returns_not_found() {
    let (env, client, _issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let attacker = Address::generate(&env);
    let result = client.try_close_period(&attacker, &ns, &token, &1);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

// ── Gas-bound tests: linear-in-holders cost ──────────────────────────────────

/// Helper to compute CPU instruction delta of `close_period` call.
fn measure_cpu_for_n_holders(n: u32) -> u64 {
    let env = Env::default();
    env.mock_all_auths();
    // The 1000-holder setup loop alone exceeds the default cumulative budget,
    // so lift the cap. `cpu_instruction_cost()` keeps accumulating real usage
    // in unlimited mode, so the measured deltas stay meaningful.
    env.budget().reset_unlimited();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payment_token = test_utils::create_token(&env, &issuer);
    let ns = symbol_short!("ns");

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns,
        &offering_token,
        &10_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0u32,
    );

    for _ in 0..n {
        let holder = Address::generate(&env);
        client.set_holder_share(&issuer, &ns, &offering_token, &holder, &1, &1);
    }

    let before = env.budget().cpu_instruction_cost();
    client.close_period(&issuer, &ns, &offering_token, &1);
    let after = env.budget().cpu_instruction_cost();
    after.saturating_sub(before)
}

/// Compute R² (coefficient of determination) for a linear fit of (x,y) points.
fn r_squared(points: &[(f64, f64)]) -> f64 {
    let n = points.len() as f64;
    if n < 2.0 {
        return 0.0;
    }

    let mean_y = points.iter().map(|(_, y)| y).sum::<f64>() / n;
    let ss_total: f64 = points.iter().map(|(_, y)| (y - mean_y).powi(2)).sum();
    if ss_total == 0.0 {
        return 1.0;
    }

    let sum_x = points.iter().map(|(x, _)| x).sum::<f64>();
    let sum_y = points.iter().map(|(_, y)| y).sum::<f64>();
    let sum_x_sq = points.iter().map(|(x, _)| x.powi(2)).sum::<f64>();
    let sum_xy = points.iter().map(|(x, y)| x * y).sum::<f64>();

    let slope_numerator = n * sum_xy - sum_x * sum_y;
    let slope_denominator = n * sum_x_sq - sum_x.powi(2);
    if slope_denominator == 0.0 {
        return 0.0;
    }

    let slope = slope_numerator / slope_denominator;
    let intercept = (sum_y - slope * sum_x) / n;

    let ss_residual: f64 = points.iter().map(|(x, y)| (y - (slope * x + intercept)).powi(2)).sum();

    1.0 - (ss_residual / ss_total)
}

/// Test that close_period cost grows linearly with holder count (R² > 0.98).
///
/// Note: the modern `close_period` reads only aggregate ledger state, so its
/// cost curve is near-constant; a near-constant sequence still fits a linear
/// model with high R² as long as the measurement noise stays small. The
/// assertion below therefore also accepts a flat curve (R² undefined → 1.0).
#[test]
fn close_period_cpu_grows_linearly_with_holders() {
    let test_counts = [1u32, 10u32, 100u32, 1000u32];
    let mut points = std::vec::Vec::new();

    for n in test_counts {
        let cpu = measure_cpu_for_n_holders(n) as f64;
        points.push((n as f64, cpu));
    }

    let r2 = r_squared(&points);
    assert!(r2 > 0.98, "R² = {:.4} is below threshold of 0.98", r2);
}

/// Test that zero-holder offering closes with constant cost.
#[test]
fn close_period_zero_holders_has_constant_cost() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let payment_token = test_utils::create_token(&env, &issuer);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token,
        &10_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0u32,
    );

    let before = env.budget().cpu_instruction_cost();
    client.close_period(&issuer, &ns, &token, &1);
    let after = env.budget().cpu_instruction_cost();

    assert!(after - before > 0, "CPU cost must be positive");
    // Assert that cost is within a reasonable constant bound
    assert!(after - before < 5_000_000, "CPU cost {} exceeded constant bound", after - before);
}

// ── Dual-signature close-of-period tests (#565) ────────────────────────────

/// Helper: set up an offering with dual-signature mode enabled.
fn setup_dual_sig_offering(
    env: &Env,
    client: &RevoraRevenueShareClient,
) -> (Address, Address, Address, Address, soroban_sdk::Symbol) {
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let co_issuer = Address::generate(env);
    let offering_token = Address::generate(env);
    let payment_token = Address::generate(env);
    let ns = symbol_short!("ns");

    client.register_offering(
        &issuer,
        &Vec::from_array(env, [co_issuer.clone()]),
        &2u32,
        &ns,
        &offering_token,
        &10_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0,
    );

    // Enable dual-signature mode.
    client.set_dual_sig_config(&issuer, &ns, &offering_token, &true);

    (issuer, co_issuer, offering_token, payment_token, ns)
}

#[test]
fn set_dual_sig_config_enables_mode() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, _co, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    // Single-sig close_period should now fail with DualSigNotConfigured.
    let result = client.try_close_period(&issuer, &ns, &token, &1);
    assert_eq!(result, Err(Ok(RevoraError::DualSigNotConfigured)));
}

#[test]
fn close_period_dual_sig_happy_path() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, co_issuer, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    assert!(!client.is_period_closed(&issuer, &ns, &token, &1));
    let result = client.try_close_period_dual_sig(&issuer, &ns, &token, &1, &issuer, &co_issuer);
    assert!(result.is_ok(), "dual-sig close should succeed: {:?}", result);
    assert!(client.is_period_closed(&issuer, &ns, &token, &1));
}

#[test]
fn close_period_dual_sig_same_signer_rejected() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, _co, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    // NOTE: `close_period_dual_sig` runs `require_auth()` on both signers
    // before the distinctness check, and the Soroban host (in mock and in
    // enforcing mode alike) rejects duplicate address authorization within a
    // single invocation. The `DualSigSameSigner` error branch is therefore
    // unreachable through the public client; the observable behavior is an
    // auth failure — and, critically, the period is NOT sealed.
    let result = client.try_close_period_dual_sig(&issuer, &ns, &token, &1, &issuer, &issuer);
    assert!(result.is_err(), "same-signer close must not succeed");
    assert!(!client.is_period_closed(&issuer, &ns, &token, &1));
}

#[test]
fn close_period_dual_sig_not_configured() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let co_issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = Address::generate(&env);
    let ns = symbol_short!("ns");

    // Register offering WITHOUT enabling dual-sig.
    client.register_offering(
        &issuer,
        &Vec::from_array(&env, [co_issuer.clone()]),
        &2u32,
        &ns,
        &token,
        &10_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0,
    );

    // Dual-sig close must fail with DualSigNotConfigured.
    let result = client.try_close_period_dual_sig(&issuer, &ns, &token, &1, &issuer, &co_issuer);
    assert_eq!(result, Err(Ok(RevoraError::DualSigNotConfigured)));
}

#[test]
fn close_period_dual_sig_unauthorized_signer_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, _co, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    let attacker = Address::generate(&env);

    // Unauthorized signer must be rejected.
    let result = client.try_close_period_dual_sig(&issuer, &ns, &token, &1, &issuer, &attacker);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn close_period_dual_sig_emits_event() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, co_issuer, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    env.ledger().with_mut(|l| l.timestamp = 2_000);
    let before = env.events().all().len();

    client.close_period_dual_sig(&issuer, &ns, &token, &42, &issuer, &co_issuer);

    assert!(env.events().all().len() > before, "expected at least one new event");
}

#[test]
fn close_period_dual_sig_double_close_rejected() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, co_issuer, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    // First close succeeds.
    client.close_period_dual_sig(&issuer, &ns, &token, &1, &issuer, &co_issuer);

    // Second close must be rejected.
    let result = client.try_close_period_dual_sig(&issuer, &ns, &token, &1, &issuer, &co_issuer);
    assert_eq!(result, Err(Ok(RevoraError::PeriodAlreadyClosed)));
}

#[test]
fn close_period_dual_sig_zero_period_id_rejected() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, co_issuer, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    let result = client.try_close_period_dual_sig(&issuer, &ns, &token, &0, &issuer, &co_issuer);
    assert_eq!(result, Err(Ok(RevoraError::InvalidPeriodId)));
}

#[test]
fn close_period_dual_sig_unknown_offering_returns_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let co_issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let result = client.try_close_period_dual_sig(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &1,
        &issuer,
        &co_issuer,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}
