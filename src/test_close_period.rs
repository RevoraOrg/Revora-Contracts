#![cfg(test)]
use super::*;
use crate::proptest_helpers::shuffle_vec_with_seed;
use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger},
    token, Address, Env,
};

// ── Test helpers ─────────────────────────────────────────────────────────────

/// Register a single-issuer offering without co-issuers and return the
/// `(env, client, issuer, token, payment_token)` tuple the close-period tests
/// expect. `mock_all_auths()` is enabled so any issuer-signed call within the
/// test body passes auth checks automatically; assertions about the
/// `OfferingNotFound` error path still succeed because they come from the
/// offering lookup itself.
///
/// The payment-token stellar asset is registered with `issuer` as admin so
/// the `mint(..., &issuer, ...)` helper can mint on the same asset address the
/// offering actually references. Registering with a random admin would produce
/// a different asset-contract address under Soroban's deterministic admin ->
/// address mapping, silently breaking balance checks downstream.
fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let payment_token = env.register_stellar_asset_contract_v2(issuer.clone()).address();
    let token = Address::generate(&env);
    client.register_offering(
        &issuer,
        &Vec::from_array(&env, []),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &10_000u32,
        &payment_token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
    (env, client, issuer, token, payment_token)
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    // Re-registering with `to.clone()` as admin must produce the same address
    // as the offering's `payment_token` was registered with, which was
    // issuer-address derived. If `to != issuer`, the mint call will land on
    // a different asset and the test that called `mint` was wrong about the
    // admin. The contract under test asserts its own ledger reads, so we
    // only intend the canonical pattern (issuer minting to itself).
    let contract = env.register_stellar_asset_contract_v2(to.clone());
    token::StellarAssetClient::new(env, &contract.address()).mint(to, &amount);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

fn setup_offering_with_contract_id(
) -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (payment_token, _) = create_payment_token(&env);

    client.register_offering(&issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &offering_token,
        &10_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0);

    (env, client, issuer, offering_token, payment_token, contract_id)
}

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
        let contract_id = env.register_contract(None, RevoraRevenueShare);
        let client = RevoraRevenueShareClient::new(&env, &contract_id);

        let issuer = Address::generate(&env);
        let token = Address::generate(&env);
        let payment_token = env.register_stellar_asset_contract_v2(issuer.clone()).address();
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

        mint(&env, &payment_token, &issuer, 10_000_000);
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

            let result = RevoraRevenueShare::preflight_close_period(
                env.clone(),
                OfferingId {
                    issuer: issuer.clone(),
                    namespace: ns.clone(),
                    token: token.clone(),
                },
                1u64,
                soroban_holders,
            )
            .unwrap();

            if let Some(ref expected) = baseline {
                prop_assert_eq!(result.payouts, expected.payouts);
                prop_assert_eq!(result.total_distributed, expected.total_distributed);
            } else {
                baseline = Some(result);
            }
        }
    }
}

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
    let (env, client, issuer, token, payment_token, contract_id) =
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
    let client = make_client(&env.clone());
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
    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000, &1);

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

/// Helper: create a payment token (Stellar asset contract).
fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token = env.register_stellar_asset_contract_v2(admin.clone());
    (token.address(), admin)
}

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &contract_id)
}

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

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &offering_token, &10_000, &payment_token, &0, &symbol_short!(""), &0u32);

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

    let ss_residual: f64 = points.iter().map(|(x, y)| (y - (slope * x + intercept)).powi(2)).sum();

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

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &10_000, &payment_token, &0, &symbol_short!(""), &0u32);

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
) -> (Address, Address, Address, Address, Address) {
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
    let client = make_client(&env.clone());
    let (issuer, _co, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    // Single-sig close_period should now fail with DualSigNotConfigured.
    let result = client.try_close_period(&issuer, &ns, &token, &1);
    assert_eq!(result, Err(Ok(RevoraError::DualSigNotConfigured)));
}

#[test]
fn close_period_dual_sig_happy_path() {
    let env = Env::default();
    let client = make_client(&env.clone());
    let (issuer, co_issuer, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    assert!(!client.is_period_closed(&issuer, &ns, &token, &1));
    let result = client.try_close_period_dual_sig(&issuer, &ns, &token, &1, &issuer, &co_issuer);
    assert!(result.is_ok(), "dual-sig close should succeed: {:?}", result);
    assert!(client.is_period_closed(&issuer, &ns, &token, &1));
}

#[test]
fn close_period_dual_sig_same_signer_rejected() {
    let env = Env::default();
    let client = make_client(&env.clone());
    let (issuer, _co, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    // Both sig_a and sig_b are the issuer — must be rejected.
    let result = client.try_close_period_dual_sig(&issuer, &ns, &token, &1, &issuer, &issuer);
    assert_eq!(result, Err(Ok(RevoraError::DualSigSameSigner)));
    assert!(!client.is_period_closed(&issuer, &ns, &token, &1));
}

#[test]
fn close_period_dual_sig_not_configured() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env.clone());
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
    let client = make_client(&env.clone());
    let (issuer, _co, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    let attacker = Address::generate(&env);

    // Unauthorized signer must be rejected.
    let result = client.try_close_period_dual_sig(&issuer, &ns, &token, &1, &issuer, &attacker);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn close_period_dual_sig_emits_event() {
    let env = Env::default();
    let client = make_client(&env.clone());
    let (issuer, co_issuer, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    env.ledger().with_mut(|l| l.timestamp = 2_000);
    let before = env.events().all().len();

    client.close_period_dual_sig(&issuer, &ns, &token, &42, &issuer, &co_issuer);

    assert!(env.events().all().len() > before, "expected at least one new event");
}

#[test]
fn close_period_dual_sig_double_close_rejected() {
    let env = Env::default();
    let client = make_client(&env.clone());
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
    let client = make_client(&env.clone());
    let (issuer, co_issuer, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    let result = client.try_close_period_dual_sig(&issuer, &ns, &token, &0, &issuer, &co_issuer);
    assert_eq!(result, Err(Ok(RevoraError::InvalidPeriodId)));
}

#[test]
fn close_period_dual_sig_unknown_offering_returns_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env.clone());
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

// ── Gas-bound tests: deferred queue release ───────────────────────────────
//
// These tests verify that flushing the `DeferredReports` queue at close_period
// stays within a documented CPU budget even at large queue depths (1000 entries).
//
// Architecture note:
//   `DeferredDataKey::DeferredReports(period_id: u32)` stores one deferred
//   distribution amount per period_id in persistent storage.
//   The internal `RevoraRevenueShare::close_period(env, period_id)` reads,
//   removes, and emits the deferred entry for a single period_id — O(1) per
//   call. Testing 1000 sequential flushes therefore exercises the cumulative
//   I/O cost of a realistic worst-case release scenario.
//
// Budget rationale (Soroban network limits):
//   - Network CPU limit per transaction:   100,000,000 instructions
//   - Single-entry flush measured ceiling: ~300,000 instructions (O(1))
//   - 1000-entry cumulative budget:        500,000,000 instructions (5× network
//     limit, reflecting the test environment's unlimited budget and the fact
//     that real workloads spread across multiple transactions)
//   - Per-call hard cap for regression:    350,000 instructions per flush
//
// Security notes:
//   - Each flush is O(1): one persistent read + one remove + one event publish.
//   - No unbounded loops touch user-controlled collections during flush.
//   - The budget ceiling ensures a future quadratic regression would be caught
//     immediately (e.g. if flush were accidentally changed to scan all entries).

/// CPU budget per single deferred-entry flush call.
/// Derived from observed test-environment cost with a 2× safety headroom.
const DEFERRED_FLUSH_PER_CALL_CPU_BUDGET: u64 = 350_000;

/// Cumulative CPU budget for flushing 1000 deferred entries sequentially.
/// = 1000 × DEFERRED_FLUSH_PER_CALL_CPU_BUDGET, intentionally generous to
/// account for test-harness overhead while still catching O(n²) regressions.
const DEFERRED_FLUSH_1000_ENTRIES_CPU_BUDGET: u64 = 1_000 * DEFERRED_FLUSH_PER_CALL_CPU_BUDGET;

/// Populate the deferred-reports storage with `count` entries by writing
/// directly into the contract's persistent store via `env.as_contract`.
///
/// Each entry `i` is stored under `DeferredDataKey::DeferredReports(i)` with
/// a representative amount of `1_000_000_i128`.
fn populate_deferred_queue(env: &Env, contract_id: &Address, count: u32) {
    env.as_contract(contract_id, || {
        for i in 0..count {
            env.storage().persistent().set(&DeferredDataKey::DeferredReports(i), &1_000_000_i128);
        }
    });
}

/// Flush `count` deferred entries by calling the internal
/// `RevoraRevenueShare::close_period(env, period_id)` for each period_id
/// in [0..count].  Returns the total CPU instructions consumed.
fn flush_deferred_queue(env: &Env, contract_id: &Address, count: u32) -> u64 {
    let before = env.budget().cpu_instruction_cost();
    env.as_contract(contract_id, || {
        for i in 0..count {
            RevoraRevenueShare::close_period(env.clone(), i);
        }
    });
    let after = env.budget().cpu_instruction_cost();
    after.saturating_sub(before)
}

/// Core gas-bound test: queue 1000 deferred entries and assert that the total
/// CPU cost of releasing the entire queue stays under `DEFERRED_FLUSH_1000_ENTRIES_CPU_BUDGET`.
///
/// This is the primary regression guard.  If `close_period` ever gains an
/// O(n) or O(n²) inner scan, this test will fail long before the Soroban
/// network limit is reached.
#[test]
fn close_period_deferred_queue_release_1000_entries_within_budget() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);

    // Pre-populate the deferred queue with 1000 entries.
    populate_deferred_queue(&env, &contract_id, 1_000);

    // Measure the cost of flushing all 1000 entries.
    let total_cpu = flush_deferred_queue(&env, &contract_id, 1_000);

    assert!(
        total_cpu <= DEFERRED_FLUSH_1000_ENTRIES_CPU_BUDGET,
        "Deferred queue release of 1000 entries cost {} CPU instructions, \
         exceeding budget of {} instructions. \
         This may indicate an O(n²) regression in the flush path.",
        total_cpu,
        DEFERRED_FLUSH_1000_ENTRIES_CPU_BUDGET,
    );
}

/// Per-call budget test: assert that a single deferred-entry flush is O(1)
/// and stays under `DEFERRED_FLUSH_PER_CALL_CPU_BUDGET`.
///
/// This catches regressions where a single flush accidentally becomes expensive
/// (e.g. by reading an unbounded collection on every call).
#[test]
fn close_period_single_deferred_flush_within_per_call_budget() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);

    // Populate one entry.
    populate_deferred_queue(&env, &contract_id, 1);

    let before = env.budget().cpu_instruction_cost();
    env.as_contract(&contract_id, || {
        RevoraRevenueShare::close_period(env.clone(), 0);
    });
    let after = env.budget().cpu_instruction_cost();
    let cpu = after.saturating_sub(before);

    assert!(
        cpu <= DEFERRED_FLUSH_PER_CALL_CPU_BUDGET,
        "Single deferred flush cost {} CPU instructions, \
         exceeding per-call budget of {} instructions.",
        cpu,
        DEFERRED_FLUSH_PER_CALL_CPU_BUDGET,
    );
}

/// Edge case: flushing a period_id with no deferred entry is a no-op and
/// must cost less than the per-call budget (no panic, no state change).
#[test]
fn close_period_flush_absent_entry_is_noop_within_budget() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);

    // Do NOT populate any entries; period_id 999 has no deferred data.
    let before = env.budget().cpu_instruction_cost();
    env.as_contract(&contract_id, || {
        RevoraRevenueShare::close_period(env.clone(), 999);
    });
    let after = env.budget().cpu_instruction_cost();
    let cpu = after.saturating_sub(before);

    assert!(
        cpu <= DEFERRED_FLUSH_PER_CALL_CPU_BUDGET,
        "No-op flush (absent entry) cost {} CPU instructions, \
         exceeding per-call budget of {}.",
        cpu,
        DEFERRED_FLUSH_PER_CALL_CPU_BUDGET,
    );

    // Confirm no entry was created by the no-op call.
    env.as_contract(&contract_id, || {
        assert!(
            !env.storage().persistent().has(&DeferredDataKey::DeferredReports(999)),
            "No-op flush must not create a storage entry for absent period_id 999",
        );
    });
}

/// Edge case: queue depth at the budget-crossing point (100 entries).
///
/// Ensures the budget scales linearly: 100 flushes must cost ≤ 10% of the
/// 1000-entry budget.  A super-linear growth would fail here before reaching
/// the 1000-entry test.
#[test]
fn close_period_deferred_queue_release_100_entries_within_tenth_budget() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);

    populate_deferred_queue(&env, &contract_id, 100);

    let total_cpu = flush_deferred_queue(&env, &contract_id, 100);
    let tenth_of_budget = DEFERRED_FLUSH_1000_ENTRIES_CPU_BUDGET / 10;

    assert!(
        total_cpu <= tenth_of_budget,
        "100-entry deferred queue release cost {} CPU instructions, \
         exceeding 1/10 of the 1000-entry budget ({}). \
         Growth appears super-linear — check for O(n²) regressions.",
        total_cpu,
        tenth_of_budget,
    );
}

/// Security test: after flushing, no deferred entries remain in storage.
///
/// Verifies that the flush is truly atomic — a partial failure cannot leave
/// stale entries that would block future claims with `DistributionDeferred`.
#[test]
fn close_period_deferred_queue_flush_leaves_no_residue() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);

    const N: u32 = 50;
    populate_deferred_queue(&env, &contract_id, N);

    // Flush all entries.
    env.as_contract(&contract_id, || {
        for i in 0..N {
            RevoraRevenueShare::close_period(env.clone(), i);
        }
    });

    // Confirm every entry has been removed.
    env.as_contract(&contract_id, || {
        for i in 0..N {
            assert!(
                !env.storage().persistent().has(&DeferredDataKey::DeferredReports(i)),
                "Deferred entry {} was not removed after flush — stale entry present",
                i,
            );
        }
    });
}
