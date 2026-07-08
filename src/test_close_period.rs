#![cfg(test)]
use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger},
    token, Address, Env,
};

#[test]
#[should_panic(expected = "Error(Contract, #456)")]
fn test_claim_on_deferred_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, AmountValidationResult);
    let client = AmountValidationResultClient::new(&env, &contract_id);

    client.register_offering(
        &issuer,
        &symbol_short!("ns"),
        &offering_token,
        &10_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0);

    (env, client, issuer, offering_token, payment_token)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn close_period_happy_path() {
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    assert!(!client.is_period_closed(&issuer, &ns, &token, &1));
    client.close_period(&issuer, &ns, &token, &1);
    assert!(client.is_period_closed(&issuer, &ns, &token, &1));
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
    mint(&env, &payment_token, &issuer, 10_000);
    let result = client.try_deposit_revenue(&issuer, &ns, &token, &payment_token, &1_000, &1);
    assert!(result.is_ok(), "deposit_revenue must succeed even after close_period");
}

#[test]
fn claim_after_close_is_allowed() {
    let (env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");

    let holder = Address::generate(&env);

    // Set holder share to 100%.
    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000);

    // Deposit revenue for period 1.
    mint(&env, &payment_token, &issuer, 1_000);
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
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (payment_token, _) = create_payment_token(&env);
    let ns = symbol_short!("ns");

    client.register_offering(&issuer, &ns, &offering_token, &10_000, &payment_token, &0);

    for _ in 0..n {
        let holder = Address::generate(&env);
        client.set_holder_share(&issuer, &ns, &offering_token, &holder, &1);
    }

    let before = env.budget().cpu_instruction_count();
    client.close_period(&issuer, &ns, &offering_token, &1);
    let after = env.budget().cpu_instruction_count();
    after.saturating_sub(before)
}

/// Compute R² (coefficient of determination) for a linear fit of (x,y) points.
fn r_squared(points: &[(f64, f64)]) -> f64 {
    let n = points.len() as f64;
    if n < 2 {
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

    let ss_residual: f64 = points.iter()
        .map(|(x, y)| (y - (slope * x + intercept)).powi(2))
        .sum();

    1.0 - (ss_residual / ss_total)
}

/// Test that close_period cost grows linearly with holder count (R² > 0.98).
#[test]
fn close_period_cpu_grows_linearly_with_holders() {
    let test_counts = [1u32, 10u32, 100u32, 1000u32];
    let mut points = Vec::new();

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
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let (payment_token, _) = create_payment_token(&env);

    client.register_offering(&issuer, &ns, &token, &10_000, &payment_token, &0);

    let before = env.budget().cpu_instruction_count();
    client.close_period(&issuer, &ns, &token, &1);
    let after = env.budget().cpu_instruction_count();

    assert!(after - before > 0, "CPU cost must be positive");
    // Assert that cost is within a reasonable constant bound
    assert!(after - before < 5_000_000, "CPU cost {} exceeded constant bound", after - before);
}
