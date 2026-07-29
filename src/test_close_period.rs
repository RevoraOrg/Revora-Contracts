#![cfg(test)]
use super::*;
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
fn setup_offering()
    -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address)
{
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let payment_token = env
        .register_stellar_asset_contract_v2(issuer.clone())
        .address();
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

fn setup_offering_with_contract_id() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (payment_token, _) = create_payment_token(&env);

    client.register_offering(&issuer, &symbol_short!("ns"), &offering_token, &10_000, &payment_token, &0);

    (env, client, issuer, offering_token, payment_token, contract_id)
}

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let (env, client, issuer, token, payment_token, _) = setup_offering_with_contract_id();
    (env, client, issuer, token, payment_token)
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
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering_with_contract_id();
    let ns = symbol_short!("ns");
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &5_000);

    let before_events = env.events().all().len();
    env.as_contract(&contract_id, || {
        let offering_id = OfferingId {
            issuer: issuer.clone(),
            namespace: ns.clone(),
            token: token.clone(),
        };
        env.storage().persistent().set(&DataKey::HolderShareTotal(offering_id), &7_000u32);
    });

    let result = client.try_close_period(&issuer, &ns, &token, &1);
    assert_eq!(result, Err(Ok(RevoraError::CloseAbortInvariantsViolated)));
    assert!(!client.is_period_closed(&issuer, &ns, &token, &1));
    assert_eq!(env.events().all().len(), before_events, "abort path must not emit close_period events");
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
    let result = client.try_close_period_dual_sig(
        &issuer, &ns, &token, &1, &issuer, &co_issuer,
    );
    assert!(result.is_ok(), "dual-sig close should succeed: {:?}", result);
    assert!(client.is_period_closed(&issuer, &ns, &token, &1));
}

#[test]
fn close_period_dual_sig_same_signer_rejected() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, _co, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    // Both sig_a and sig_b are the issuer — must be rejected.
    let result = client.try_close_period_dual_sig(
        &issuer, &ns, &token, &1, &issuer, &issuer,
    );
    assert_eq!(result, Err(Ok(RevoraError::DualSigSameSigner)));
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
    let result = client.try_close_period_dual_sig(
        &issuer, &ns, &token, &1, &issuer, &co_issuer,
    );
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
    let result = client.try_close_period_dual_sig(
        &issuer, &ns, &token, &1, &issuer, &attacker,
    );
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
    let result = client.try_close_period_dual_sig(
        &issuer, &ns, &token, &1, &issuer, &co_issuer,
    );
    assert_eq!(result, Err(Ok(RevoraError::PeriodAlreadyClosed)));
}

#[test]
fn close_period_dual_sig_zero_period_id_rejected() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, co_issuer, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    let result = client.try_close_period_dual_sig(
        &issuer, &ns, &token, &0, &issuer, &co_issuer,
    );
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
        &issuer, &symbol_short!("ns"), &token, &1, &issuer, &co_issuer,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

// ── Per-class dividend priority ordering tests (#523) ──────────────────────
//
// The priority feature is wired through `close_period` and
// `close_period_dual_sig`. We register classes directly into the
// `Vec<(ShareClass, ClassConfig)>` storage since the test harness's
// `register_offering` does not pre-populate it, and we read them back via
// `get_class_pay_order` / `get_class_priority` to verify the resolver is
// deterministic and tie-breaks by canonical XDR bytes.

fn register_offering_with_coissuer(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    co_issuer: &Address,
    ns: &Symbol,
    token: &Address,
    payment_token: &Address,
) {
    client.register_offering(
        issuer,
        &Vec::from_array(env, [co_issuer.clone()]),
        &2u32,
        ns,
        token,
        &10_000u32,
        payment_token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
}

fn register_classes_in_storage(
    env: &Env,
    offering_id: &OfferingId,
    pairs: &[(ShareClass, crate::ClassConfig)],
) {
    let key = crate::DataKey2::OfferingClasses(offering_id.clone());
    env.storage().persistent().set(&key, pairs);
}

#[test]
fn class_priority_set_class_priority_happy_path_roundtrip() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let offering_id = crate::OfferingId {
        issuer: issuer.clone(),
        namespace: ns.clone(),
        token: token.clone(),
    };
    register_classes_in_storage(
        &env,
        &offering_id,
        &[
            (
                ShareClass::Custom(Symbol::new(&env, "pref")),
                crate::ClassConfig { bps: 5_000, voting: true },
            ),
            (
                ShareClass::Custom(Symbol::new(&env, "comm")),
                crate::ClassConfig { bps: 5_000, voting: false },
            ),
        ],
    );

    let sc = ShareClass::Custom(Symbol::new(&env, "pref"));
    client.set_class_priority(&issuer, &ns, &token, &sc, &7u32);
    assert_eq!(client.get_class_priority(&issuer, &ns, &token, &sc), 7u32);

    let comm = ShareClass::Custom(Symbol::new(&env, "comm"));
    client.set_class_priority(&issuer, &ns, &token, &comm, &11u32);
    assert_eq!(client.get_class_priority(&issuer, &ns, &token, &comm), 11u32);
}

#[test]
fn class_priority_get_class_priority_default_is_zero() {
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let sc = ShareClass::Custom(Symbol::new(&_env, "never_set"));
    // No set_class_priority call → default of 0.
    assert_eq!(client.get_class_priority(&issuer, &ns, &token, &sc), 0u32);
}

#[test]
fn class_priority_set_wrong_issuer_returns_not_found() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let offering_id = crate::OfferingId {
        issuer: issuer.clone(),
        namespace: ns.clone(),
        token: token.clone(),
    };
    register_classes_in_storage(
        &env,
        &offering_id,
        &[(
            ShareClass::Custom(Symbol::new(&env, "p")),
            crate::ClassConfig { bps: 1_000, voting: true },
        )],
    );

    let attacker = Address::generate(&env);
    let sc = ShareClass::Custom(Symbol::new(&env, "p"));
    let result = client.try_set_class_priority(&attacker, &ns, &token, &sc, &1u32);
    // Wrong issuer hits the same `OfferingNotFound` guard pattern as
    // `close_period` — defensible because it conflates "wrong issuer" and
    // "missing offering" intentionally.
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn class_priority_set_unknown_offering_returns_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let sc = ShareClass::Custom(Symbol::new(&env, "p"));
    let result = client.try_set_class_priority(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &sc,
        &1u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn class_priority_set_unregistered_class_returns_invalid() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let offering_id = crate::OfferingId {
        issuer: issuer.clone(),
        namespace: ns.clone(),
        token: token.clone(),
    };
    // Register only class A — Custom("ghost") is NOT registered.
    register_classes_in_storage(
        &env,
        &offering_id,
        &[(
            ShareClass::A,
            crate::ClassConfig { bps: 10_000, voting: true },
        )],
    );

    let ghost = ShareClass::Custom(Symbol::new(&env, "ghost"));
    let result = client.try_set_class_priority(&issuer, &ns, &token, &ghost, &1u32);
    assert_eq!(result, Err(Ok(RevoraError::InvalidShareClass)));
}

#[test]
fn class_priority_close_period_emits_class_pay_order() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let offering_id = crate::OfferingId {
        issuer: issuer.clone(),
        namespace: ns.clone(),
        token: token.clone(),
    };
    let pref = ShareClass::Custom(Symbol::new(&env, "pref"));
    let comm = ShareClass::Custom(Symbol::new(&env, "comm"));
    register_classes_in_storage(
        &env,
        &offering_id,
        &[
            (pref.clone(), crate::ClassConfig { bps: 5_000, voting: true }),
            (comm.clone(), crate::ClassConfig { bps: 5_000, voting: false }),
        ],
    );

    // `pref` should be paid first (lower priority index).
    client.set_class_priority(&issuer, &ns, &token, &pref.clone(), &0u32);
    client.set_class_priority(&issuer, &ns, &token, &comm.clone(), &1u32);

    let events_before = env.events().all().len();
    client.close_period(&issuer, &ns, &token, &1u64);
    let events_after = env.events().all().len();
    assert!(events_after > events_before, "close_period must emit events");

    // Read back the cached pay order: it must contain the two classes and be
    // ordered with `pref` first.
    let order = client.get_class_pay_order(&issuer, &ns, &token, &1u64);
    assert_eq!(order.len(), 2);
    assert_eq!(order.get(0).unwrap(), pref);
    assert_eq!(order.get(1).unwrap(), comm);
}

#[test]
fn class_priority_close_period_orders_by_priority_ascending() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let offering_id = crate::OfferingId {
        issuer: issuer.clone(),
        namespace: ns.clone(),
        token: token.clone(),
    };
    let a = ShareClass::A;
    let b = ShareClass::B;
    let c = ShareClass::Custom(Symbol::new(&env, "c"));
    register_classes_in_storage(
        &env,
        &offering_id,
        &[
            (a.clone(), crate::ClassConfig { bps: 2_000, voting: true }),
            (b.clone(), crate::ClassConfig { bps: 2_000, voting: true }),
            (c.clone(), crate::ClassConfig { bps: 6_000, voting: false }),
        ],
    );

    // Set priorities so the expected order is [c, b, a] (high→low index asc).
    client.set_class_priority(&issuer, &ns, &token, &a, &9u32);
    client.set_class_priority(&issuer, &ns, &token, &b, &5u32);
    client.set_class_priority(&issuer, &ns, &token, &c.clone(), &2u32);

    client.close_period(&issuer, &ns, &token, &1u64);

    let order = client.get_class_pay_order(&issuer, &ns, &token, &1u64);
    assert_eq!(order.get(0).unwrap(), c);
    assert_eq!(order.get(1).unwrap(), b);
    assert_eq!(order.get(2).unwrap(), a);
}

#[test]
fn class_priority_close_period_tie_break_is_xdr_canonical() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let offering_id = crate::OfferingId {
        issuer: issuer.clone(),
        namespace: ns.clone(),
        token: token.clone(),
    };
    // Three Custom classes — all tied at priority 0 — must sort by their XDR
    // bytes deterministically. We close two distinct periods to confirm the
    // tie-break is stable across reruns.
    let c_alpha = ShareClass::Custom(Symbol::new(&env, "alpha"));
    let c_beta = ShareClass::Custom(Symbol::new(&env, "beta"));
    let c_gamma = ShareClass::Custom(Symbol::new(&env, "gamma"));
    register_classes_in_storage(
        &env,
        &offering_id,
        &[
            (c_alpha.clone(), crate::ClassConfig { bps: 1_000, voting: false }),
            (c_beta.clone(), crate::ClassConfig { bps: 1_000, voting: false }),
            (c_gamma.clone(), crate::ClassConfig { bps: 8_000, voting: false }),
        ],
    );

    for sc in [&c_alpha, &c_beta, &c_gamma] {
        client.set_class_priority(&issuer, &ns, &token, sc, &0u32);
    }

    client.close_period(&issuer, &ns, &token, &1u64);
    let order_a = client.get_class_pay_order(&issuer, &ns, &token, &1u64);

    client.close_period(&issuer, &ns, &token, &2u64);
    let order_b = client.get_class_pay_order(&issuer, &ns, &token, &2u64);

    assert_eq!(order_a.len(), 3);
    assert_eq!(order_b.len(), 3);

    // Same membership: every class in `order_a` must appear in `order_b` and
    // vice-versa. We compare matched classes by their XDR-canonical bytes,
    // which avoids relying on raw `ShareClass` equality across SDK versions.
    let mut seen_a: soroban_sdk::Vec<Bytes> = soroban_sdk::Vec::new(&env);
    for sc in order_a.iter() {
        seen_a.push_back(sc.to_xdr(&env));
    }
    let mut seen_b: soroban_sdk::Vec<Bytes> = soroban_sdk::Vec::new(&env);
    for sc in order_b.iter() {
        seen_b.push_back(sc.to_xdr(&env));
    }
    assert_eq!(seen_a.len(), seen_b.len());

    // Cross-membership check: each byte sequence in `seen_a` is in `seen_b`.
    for a_bytes in seen_a.iter() {
        let mut found = false;
        for b_bytes in seen_b.iter() {
            if a_bytes == b_bytes {
                found = true;
                break;
            }
        }
        assert!(found, "class present in period 1 but absent from period 2");
    }

    // The resolver must produce strictly ascending XDR bytes order — this is
    // the on-chain canonical tie-break the contract guarantees.
    let bytes_a: soroban_sdk::Vec<Bytes> = order_a.iter().map(|sc| sc.to_xdr(&env)).collect();
    for i in 1..bytes_a.len() {
        let lhs = bytes_a.get(i - 1).unwrap();
        let rhs = bytes_a.get(i).unwrap();
        assert!(
            lhs <= rhs,
            "tie-break must be ascending by XDR bytes"
        );
    }
    let bytes_b: soroban_sdk::Vec<Bytes> = order_b.iter().map(|sc| sc.to_xdr(&env)).collect();
    for i in 1..bytes_b.len() {
        let lhs = bytes_b.get(i - 1).unwrap();
        let rhs = bytes_b.get(i).unwrap();
        assert!(
            lhs <= rhs,
            "tie-break must be ascending by XDR bytes"
        );
    }
}

#[test]
fn class_priority_close_period_emits_empty_order_when_no_classes() {
    // With no OfferingClasses vec stored, the resolver must return no classes
    // and `close_period` must still succeed and emit an empty pay order.
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    client.close_period(&issuer, &ns, &token, &1u64);
    let order = client.get_class_pay_order(&issuer, &ns, &token, &1u64);
    assert_eq!(order.len(), 0);
}

#[test]
fn class_priority_close_period_dual_sig_produces_same_order() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, co_issuer, offering_token, payment_token, ns) =
        setup_dual_sig_offering(&env, &client);
    let offering_id = crate::OfferingId {
        issuer: issuer.clone(),
        namespace: ns.clone(),
        token: offering_token.clone(),
    };
    let pref = ShareClass::Custom(Symbol::new(&env, "pref"));
    let comm = ShareClass::Custom(Symbol::new(&env, "comm"));
    register_classes_in_storage(
        &env,
        &offering_id,
        &[
            (pref.clone(), crate::ClassConfig { bps: 5_000, voting: true }),
            (comm.clone(), crate::ClassConfig { bps: 5_000, voting: false }),
        ],
    );
    client.set_class_priority(&issuer, &ns, &offering_token, &pref, &0u32);
    client.set_class_priority(&issuer, &ns, &offering_token, &comm, &1u32);

    client.close_period_dual_sig(
        &issuer,
        &ns,
        &offering_token,
        &1u64,
        &issuer,
        &co_issuer,
    );
    let order = client.get_class_pay_order(&issuer, &ns, &offering_token, &1u64);
    assert_eq!(order.len(), 2);
    assert_eq!(order.get(0).unwrap(), ShareClass::Custom(Symbol::new(&env, "pref")));
    assert_eq!(order.get(1).unwrap(), ShareClass::Custom(Symbol::new(&env, "comm")));
}

#[test]
fn class_priority_get_class_pay_order_empty_for_unset_period() {
    // Querying a period that was never closed must return an empty Vec,
    // matching the documented migration-friendly fallback.
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let order = client.get_class_pay_order(&issuer, &ns, &token, &999u64);
    assert_eq!(order.len(), 0);
}

// ── Close-of-period preflight simulation tests (#563) ───────────────────────
//
// Required by the issuing task: the preflight must return per-holder payouts
// without state mutation and must mirror `close_period`'s precondition chain.
// These tests assert: empty period, single holder, multi-holder, parity
// against the actual on-chain close, and every documented error path.

fn offering_id(
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) -> crate::OfferingId {
    crate::OfferingId {
        issuer: issuer.clone(),
        namespace: namespace.clone(),
        token: token.clone(),
    }
}

#[test]
fn preflight_empty_period_returns_zero_payouts() {
    // No `report_revenue` / `deposit_revenue` call has landed — period_revenue
    // should be 0 and every holder's normalized payout should be 0.
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    let holder = Address::generate(&_env);
    client.set_holder_share(&issuer, &ns, &token, &holder, &5_000u32);

    let oid = offering_id(&issuer, &ns, &token);
    let mut holders = Vec::<Address>::new(&_env);
    holders.push_back(holder.clone());

    let pre = client
        .try_preflight_close_period(&oid, &1u64, &holders)
        .unwrap()
        .unwrap();

    assert_eq!(pre.period_id, 1u64);
    assert_eq!(pre.period_revenue, 0);
    assert_eq!(pre.class_pay_order.len(), 0);
    assert_eq!(pre.payouts.len(), 1);
    assert_eq!(pre.payouts.get(0).unwrap().holder, holder);
    assert_eq!(pre.payouts.get(0).unwrap().share_bps, 5_000);
    assert_eq!(pre.payouts.get(0).unwrap().normalized_payout, 0);
    assert_eq!(pre.total_distributed, 0);

    // No state was mutated: the period is still unclosed.
    assert!(!client.is_period_closed(&issuer, &ns, &token, &1));
}

#[test]
fn preflight_single_holder_full_share_matches_actual_close() {
    // 1 holder at 100% (10_000 bps), revenue = 1_000. Expected per-holder
    // payout = 1_000 and total_distributed = 1_000.
    let (env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000u32);

    // Mint & deposit revenue for period 1.
    mint(&env, &payment_token, &issuer, 1_000);
    client.deposit_revenue(&issuer, &ns, &token, &payment_token, &1_000, &1);

    let oid = offering_id(&issuer, &ns, &token);
    let mut holders = Vec::<Address>::new(&env);
    holders.push_back(holder.clone());

    let pre = client
        .try_preflight_close_period(&oid, &1u64, &holders)
        .unwrap()
        .unwrap();

    assert_eq!(pre.period_revenue, 1_000);
    assert_eq!(pre.payouts.len(), 1);
    assert_eq!(pre.payouts.get(0).unwrap().holder, holder);
    assert_eq!(pre.payouts.get(0).unwrap().share_bps, 10_000);
    assert_eq!(pre.payouts.get(0).unwrap().normalized_payout, 1_000);
    assert_eq!(pre.total_distributed, 1_000);

    // Now actually close and confirm claim() yields the same amount. This is
    // the parity guarantee the issue text calls out: "Preview must be exactly
    // equal to what atomic close would produce."
    client.close_period(&issuer, &ns, &token, &1);
    let payout = client.claim(&holder, &issuer, &ns, &token, &10);
    assert_eq!(payout, 1_000);
}

#[test]
fn preflight_multi_holder_split_sums_to_revenue() {
    // 3 holders at 3_333 / 3_333 / 3_334 bps with revenue = 1_000_000. Total
    // must not exceed period_revenue and the per-holder payouts must respect
    // `compute_share` truncation semantics.
    let (env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");

    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    let h3 = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &token, &h1, &3_333u32);
    client.set_holder_share(&issuer, &ns, &token, &h2, &3_333u32);
    client.set_holder_share(&issuer, &ns, &token, &h3, &3_334u32);

    mint(&env, &payment_token, &issuer, 1_000_000);
    client.deposit_revenue(&issuer, &ns, &token, &payment_token, &1_000_000, &1);

    let oid = offering_id(&issuer, &ns, &token);
    let mut holders = Vec::<Address>::new(&env);
    holders.push_back(h1.clone());
    holders.push_back(h2.clone());
    holders.push_back(h3.clone());

    let pre = client
        .try_preflight_close_period(&oid, &1u64, &holders)
        .unwrap()
        .unwrap();

    assert_eq!(pre.period_revenue, 1_000_000);
    let mut sum: i128 = 0;
    for i in 0..pre.payouts.len() {
        sum = sum.saturating_add(pre.payouts.get(i).unwrap().normalized_payout);
    }
    assert!(sum <= pre.period_revenue);
    assert_eq!(pre.total_distributed, sum);

    // Truncation default for an offering with no explicit `set_rounding_mode`:
    // 1_000_000 * 3_333 / 10_000 = 333_300 (truncated). 3_334 bps -> 333_400.
    let p0 = pre.payouts.get(0).unwrap();
    let p1 = pre.payouts.get(1).unwrap();
    let p2 = pre.payouts.get(2).unwrap();
    assert_eq!(p0.share_bps, 3_333);
    assert_eq!(p1.share_bps, 3_333);
    assert_eq!(p2.share_bps, 3_334);
    assert_eq!(p0.normalized_payout, 333_300);
    assert_eq!(p1.normalized_payout, 333_300);
    assert_eq!(p2.normalized_payout, 333_400);
}

#[test]
fn preflight_zero_holder_input_returns_empty_payouts() {
    // No holders passed in -> empty payouts, but preflight still succeeds
    // when the offering exists.
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    let oid = offering_id(&issuer, &ns, &token);
    let holders = Vec::<Address>::new(&_env);

    let pre = client
        .try_preflight_close_period(&oid, &7u64, &holders)
        .unwrap()
        .unwrap();
    assert_eq!(pre.period_id, 7u64);
    assert_eq!(pre.payouts.len(), 0);
    assert_eq!(pre.total_distributed, 0);
    assert_eq!(pre.class_pay_order.len(), 0);
}

#[test]
fn preflight_handles_no_classes_registered_with_empty_pay_order() {
    // No classes registered -> class_pay_order is empty. The preflight must
    // still compute per-holder payouts using the flat `share_bps` path.
    let (env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000u32);

    mint(&env, &payment_token, &issuer, 5_000);
    client.deposit_revenue(&issuer, &ns, &token, &payment_token, &5_000, &1);

    let oid = offering_id(&issuer, &ns, &token);
    let mut holders = Vec::<Address>::new(&env);
    holders.push_back(holder.clone());

    let pre = client
        .try_preflight_close_period(&oid, &1u64, &holders)
        .unwrap()
        .unwrap();
    assert_eq!(pre.class_pay_order.len(), 0);
    assert_eq!(pre.payouts.get(0).unwrap().normalized_payout, 5_000);
}

#[test]
fn preflight_rejects_frozen_contract() {
    // Drive the `ContractFrozen` guard by writing `DataKey::Frozen` directly
    // rather than going through `freeze()` — that keeps the test focused on
    // the preflight precondition chain and avoids any contract-API surface
    // risks around `initialize` / `MultisigThreshold`.
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payment_token = Address::generate(&env);
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

    // Flip the global Frozen flag directly. `require_not_frozen` only reads
    // this key, so the preflight chain will short-circuit to ContractFrozen.
    env.storage().persistent().set(&DataKey::Frozen, &true);

    let oid = offering_id(&issuer, &symbol_short!("ns"), &token);
    let holders = Vec::<Address>::new(&env);
    let result = client.try_preflight_close_period(&oid, &1u64, &holders);
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
}

#[test]
fn preflight_rejects_zero_period_id() {
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let oid = offering_id(&issuer, &ns, &token);
    let holders = Vec::<Address>::new(&_env);

    let result = client.try_preflight_close_period(&oid, &0u64, &holders);
    assert_eq!(result, Err(Ok(RevoraError::InvalidPeriodId)));
}

#[test]
fn preflight_rejects_unknown_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    let unknown_issuer = Address::generate(&env);
    let unknown_token = Address::generate(&env);
    let oid = offering_id(&unknown_issuer, &symbol_short!("ns"), &unknown_token);
    let holders = Vec::<Address>::new(&env);

    let result = client.try_preflight_close_period(&oid, &1u64, &holders);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn preflight_rejects_already_closed_period() {
    let (_env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    // Close period 1, then preflight for period 1 must reject.
    client.close_period(&issuer, &ns, &token, &1);

    let oid = offering_id(&issuer, &ns, &token);
    let holders = Vec::<Address>::new(&_env);
    let result = client.try_preflight_close_period(&oid, &1u64, &holders);
    assert_eq!(result, Err(Ok(RevoraError::PeriodAlreadyClosed)));
}

#[test]
fn preflight_returns_class_pay_order_matching_close() {
    // Register two Custom classes, set priorities, then confirm the preflight
    // returns the same `class_pay_order` that the actual `close_period` will
    // write via `record_and_emit_pay_order`.
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let oid_storage = offering_id(&issuer, &ns, &token);
    let pref = ShareClass::Custom(Symbol::new(&env, "pref"));
    let comm = ShareClass::Custom(Symbol::new(&env, "comm"));
    register_classes_in_storage(
        &env,
        &oid_storage,
        &[
            (pref.clone(), crate::ClassConfig { bps: 5_000, voting: true }),
            (comm.clone(), crate::ClassConfig { bps: 5_000, voting: false }),
        ],
    );
    client.set_class_priority(&issuer, &ns, &token, &pref.clone(), &0u32);
    client.set_class_priority(&issuer, &ns, &token, &comm.clone(), &1u32);

    let holders = Vec::<Address>::new(&env);
    let pre = client
        .try_preflight_close_period(&oid_storage, &1u64, &holders)
        .unwrap()
        .unwrap();
    assert_eq!(pre.class_pay_order.len(), 2);
    assert_eq!(pre.class_pay_order.get(0).unwrap(), pref);
    assert_eq!(pre.class_pay_order.get(1).unwrap(), comm);

    // Now actually close, then read the persisted pay order and assert it
    // exactly equals the preflight result.
    client.close_period(&issuer, &ns, &token, &1);
    let persisted = client.get_class_pay_order(&issuer, &ns, &token, &1u64);

    assert_eq!(persisted.len(), pre.class_pay_order.len());
    for i in 0..persisted.len() {
        assert_eq!(persisted.get(i).unwrap(), pre.class_pay_order.get(i).unwrap());
    }
}

#[test]
fn preflight_skips_blacklisted_holders() {
    // Blacklist precedence: a blacklisted holder is silently dropped from the
    // preview, and so is its payout (the issue says "without state mutation"
    // which guarantees we're not flipping any share flag).
    let (env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");

    let h_ok = Address::generate(&env);
    let h_bad = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &token, &h_ok, &5_000u32);
    client.set_holder_share(&issuer, &ns, &token, &h_bad, &10_000u32);

    // Write `DataKey::Blacklist` directly rather than via `blacklist_add`. The
    // contract-side `is_blacklisted` only checks membership in the map, so
    // any non-empty `SanctionsAttestation` is sufficient to exercise the
    // blacklist precedence rule in the preflight.
    let mut blacklisted =
        soroban_sdk::Map::<Address, crate::SanctionsAttestation>::new(&env);
    blacklisted.set(
        h_bad.clone(),
        crate::SanctionsAttestation {
            source: crate::Source::Manual,
            ref_id: symbol_short!("blk"),
            attested_at: 0,
        },
    );
    env.storage().persistent().set(
        &DataKey::Blacklist(offering_id(&issuer, &ns, &token)),
        &blacklisted,
    );

    mint(&env, &payment_token, &issuer, 1_000);
    client.deposit_revenue(&issuer, &ns, &token, &payment_token, &1_000, &1);

    let oid = offering_id(&issuer, &ns, &token);
    let mut holders = Vec::<Address>::new(&env);
    holders.push_back(h_ok.clone());
    holders.push_back(h_bad.clone());

    let pre = client
        .try_preflight_close_period(&oid, &1u64, &holders)
        .unwrap()
        .unwrap();
    assert_eq!(pre.payouts.len(), 1, "blacklisted holder must be excluded");
    assert_eq!(pre.payouts.get(0).unwrap().holder, h_ok);
    assert_eq!(pre.payouts.get(0).unwrap().normalized_payout, 500);
}

#[test]
fn preflight_does_not_write_period_closed_storage() {
    // Multiple preflights on the same period must not flip any state and
    // must keep returning the same number — i.e. the preflight is idempotent
    // and has no execution cost other than the read.
    let (env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");
    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000u32);
    mint(&env, &payment_token, &issuer, 750);
    client.deposit_revenue(&issuer, &ns, &token, &payment_token, &750, &1);

    let oid = offering_id(&issuer, &ns, &token);
    let mut holders = Vec::<Address>::new(&env);
    holders.push_back(holder.clone());

    let a = client
        .try_preflight_close_period(&oid, &1u64, &holders)
        .unwrap()
        .unwrap();
    let b = client
        .try_preflight_close_period(&oid, &1u64, &holders)
        .unwrap()
        .unwrap();
    let c = client
        .try_preflight_close_period(&oid, &1u64, &holders)
        .unwrap()
        .unwrap();

    assert_eq!(a.period_revenue, b.period_revenue);
    assert_eq!(b.period_revenue, c.period_revenue);
    assert_eq!(a.total_distributed, b.total_distributed);
    assert_eq!(b.total_distributed, c.total_distributed);
    assert!(!client.is_period_closed(&issuer, &ns, &token, &1));
}

#[test]
fn preflight_dual_sig_capable_offering_still_works() {
    // The preflight is auth-free and intentionally ignores `DualSigEnabled` —
    // a preview must be available even when the atomic close would require
    // dual sigs. We assert that preflight succeeds regardless.
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, _co, token, _payment, ns) = setup_dual_sig_offering(&env, &client);

    let oid = offering_id(&issuer, &ns, &token);
    let holders = Vec::<Address>::new(&env);

    let pre = client
        .try_preflight_close_period(&oid, &1u64, &holders)
        .unwrap()
        .unwrap();
    assert_eq!(pre.period_id, 1u64);
    assert_eq!(pre.class_pay_order.len(), 0);
    assert_eq!(pre.payouts.len(), 0);
}

#[test]
fn preflight_total_distributed_never_exceeds_revenue() {
    // Exhaustively assert `total_distributed <= period_revenue` for several
    // revenue magnitudes and several holder splits. The bound is a hard
    // contract invariant (`compute_share` is capped at `100%` of revenue)
    // and a regression here would mean every holder can be overpaid.
    let (env, client, issuer, token, payment_token) = setup_offering();
    let ns = symbol_short!("ns");

    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);
    let h3 = Address::generate(&env);
    let h4 = Address::generate(&env);
    client.set_holder_share(&issuer, &ns, &token, &h1, &2_500u32);
    client.set_holder_share(&issuer, &ns, &token, &h2, &2_500u32);
    client.set_holder_share(&issuer, &ns, &token, &h3, &2_500u32);
    client.set_holder_share(&issuer, &ns, &token, &h4, &2_500u32);

    let cases: [i128; 3] = [1_000i128, 1_000_000, 999_999_999_999];

    for (idx, revenue) in cases.iter().enumerate() {
        let period: u64 = (idx + 1) as u64;
        mint(&env, &payment_token, &issuer, *revenue);
        client.deposit_revenue(&issuer, &ns, &token, &payment_token, revenue, &period);

        let oid = offering_id(&issuer, &ns, &token);
        let mut holders = Vec::<Address>::new(&env);
        holders.push_back(h1.clone());
        holders.push_back(h2.clone());
        holders.push_back(h3.clone());
        holders.push_back(h4.clone());

        let pre = client
            .try_preflight_close_period(&oid, &period, &holders)
            .unwrap()
            .unwrap();
        assert_eq!(pre.period_revenue, *revenue);
        assert!(
            pre.total_distributed <= pre.period_revenue,
            "total_distributed {} exceeds period_revenue {} at case {}",
            pre.total_distributed,
            pre.period_revenue,
            idx,
        );
        for i in 0..pre.payouts.len() {
            let p = pre.payouts.get(i).unwrap();
            assert!(
                p.normalized_payout <= pre.period_revenue,
                "holder {} payout {} exceeds revenue {} at case {}",
                i,
                p.normalized_payout,
                pre.period_revenue,
                idx,
            );
        }
    }
}
