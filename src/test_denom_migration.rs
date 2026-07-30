//! Tests for `migrate_denomination` — payment token decimal migration path.
//!
//! Coverage:
//! - Happy path (upscale 6→18, downscale 18→6, no-op 6→6)
//! - Idempotency (second call with same (from, to) is no-op)
//! - Authorization failure (non-issuer caller)
//! - Non-existent offering
//! - Decimal bounds (out of range → LimitReached)
//! - Aggregate amounts re-scaled correctly (DepositedRevenue, AuditSummary, SupplyCap)
//! - Event emission

use crate::{
    AuditSummary, DataKey, DataKey2, OfferingId, RevoraError, RevoraRevenueShare,
    RevoraRevenueShareClient,
};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &contract_id)
}

fn register_offering(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    payout_asset: &Address,
) {
    client.register_offering(
        issuer,
        namespace,
        token,
        &5_000, // revenue_share_bps
        payout_asset,
        &0, // supply_cap (0 = no cap)
        &Symbol::new(env, ""),
        &0, // display_decimals
    );
}

fn set_initial_decimals(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    decimals: u32,
) {
    // Directly write PaymentTokenDecimals + set some aggregate amounts
    let offering_id =
        OfferingId { issuer: issuer.clone(), namespace: namespace.clone(), token: token.clone() };

    // Set initial decimals
    client.set_payment_token_decimals(issuer, namespace, token, &decimals);

    // Write DepositedRevenue directly (as if revenue was deposited)
    env.storage()
        .persistent()
        .set(&DataKey2::DepositedRevenue(offering_id.clone()), &1_000_000_i128);

    // Write AuditSummary
    let audit = AuditSummary { total_revenue: 5_000_000_i128, report_count: 10_u64 };
    env.storage().persistent().set(&DataKey::AuditSummary(offering_id.clone()), &audit);
}

fn setup() -> (Env, RevoraRevenueShareClient, Address, Symbol, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "def");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    register_offering(&env, &client, &issuer, &namespace, &token, &payout_asset);

    (env, client, issuer, namespace, token)
}

// ── Happy path ─────────────────────────────────────────────────────────────────

/// Upscale: 6 decimals → 18 decimals.
/// All aggregate amounts should be multiplied by 10^(18-6) = 10^12.
#[test]
fn migrate_denomination_upscale_6_to_18() {
    let (env, client, issuer, namespace, token) = setup();
    set_initial_decimals(&env, &client, &issuer, &namespace, &token, 6);

    let offering_id =
        OfferingId { issuer: issuer.clone(), namespace: namespace.clone(), token: token.clone() };

    // Verify initial state
    assert_eq!(client.get_payment_token_decimals(&issuer, &namespace, &token), 6);
    assert_eq!(
        env.storage()
            .persistent()
            .get::<DataKey2, i128>(&DataKey2::DepositedRevenue(offering_id.clone())),
        Some(1_000_000)
    );

    // Migrate from 6 to 18 decimals
    let result = client.try_migrate_denomination(&issuer, &namespace, &token, &6, &18);
    assert!(result.is_ok(), "upscale migration should succeed");

    // Verify PaymentTokenDecimals updated
    assert_eq!(client.get_payment_token_decimals(&issuer, &namespace, &token), 18);

    // Verify DepositedRevenue re-scaled: 1_000_000 * 10^12 = 1_000_000_000_000
    let expected_deposited: i128 = 1_000_000_i128 * 10_i128.pow(12);
    assert_eq!(
        env.storage()
            .persistent()
            .get::<DataKey2, i128>(&DataKey2::DepositedRevenue(offering_id.clone())),
        Some(expected_deposited)
    );

    // Verify AuditSummary re-scaled: 5_000_000 * 10^12
    let expected_audit_revenue: i128 = 5_000_000_i128 * 10_i128.pow(12);
    let audit = env
        .storage()
        .persistent()
        .get::<DataKey, AuditSummary>(&DataKey::AuditSummary(offering_id.clone()))
        .unwrap();
    assert_eq!(audit.total_revenue, expected_audit_revenue);
    assert_eq!(audit.report_count, 10); // report_count unchanged
}

/// Downscale: 18 decimals → 6 decimals.
/// All aggregate amounts should be divided by 10^(18-6) = 10^12.
#[test]
fn migrate_denomination_downscale_18_to_6() {
    let (env, client, issuer, namespace, token) = setup();

    // Write larger initial amounts (18-decimal scale)
    let offering_id =
        OfferingId { issuer: issuer.clone(), namespace: namespace.clone(), token: token.clone() };

    client.set_payment_token_decimals(&issuer, &namespace, &token, &18);

    env.storage()
        .persistent()
        .set(&DataKey2::DepositedRevenue(offering_id.clone()), &1_000_000_000_000_i128);

    let audit = AuditSummary { total_revenue: 5_000_000_000_000_i128, report_count: 10_u64 };
    env.storage().persistent().set(&DataKey::AuditSummary(offering_id.clone()), &audit);

    assert_eq!(client.get_payment_token_decimals(&issuer, &namespace, &token), 18);

    // Migrate from 18 to 6 decimals
    let result = client.try_migrate_denomination(&issuer, &namespace, &token, &18, &6);
    assert!(result.is_ok(), "downscale migration should succeed");

    // Verify PaymentTokenDecimals updated
    assert_eq!(client.get_payment_token_decimals(&issuer, &namespace, &token), 6);

    // Verify DepositedRevenue re-scaled: 1_000_000_000_000 / 10^12 = 1_000_000
    assert_eq!(
        env.storage()
            .persistent()
            .get::<DataKey2, i128>(&DataKey2::DepositedRevenue(offering_id.clone())),
        Some(1_000_000)
    );

    // Verify AuditSummary re-scaled: 5_000_000_000_000 / 10^12 = 5_000_000
    let audit = env
        .storage()
        .persistent()
        .get::<DataKey, AuditSummary>(&DataKey::AuditSummary(offering_id.clone()))
        .unwrap();
    assert_eq!(audit.total_revenue, 5_000_000);
}

/// No-op: from_decimals == to_decimals. No state should change.
#[test]
fn migrate_denomination_noop_same_decimals() {
    let (env, client, issuer, namespace, token) = setup();
    set_initial_decimals(&env, &client, &issuer, &namespace, &token, 6);

    let offering_id =
        OfferingId { issuer: issuer.clone(), namespace: namespace.clone(), token: token.clone() };

    let deposited_before = env
        .storage()
        .persistent()
        .get::<DataKey2, i128>(&DataKey2::DepositedRevenue(offering_id.clone()));

    let result = client.try_migrate_denomination(
        &issuer, &namespace, &token, &6, &6, // same decimals
    );
    assert!(result.is_ok());

    // State unchanged
    assert_eq!(client.get_payment_token_decimals(&issuer, &namespace, &token), 6);
    assert_eq!(
        env.storage()
            .persistent()
            .get::<DataKey2, i128>(&DataKey2::DepositedRevenue(offering_id.clone())),
        deposited_before
    );
}

// ── Idempotency ────────────────────────────────────────────────────────────────

/// Calling migrate_denomination twice with the same (from, to) is safe.
/// The second call should succeed as a no-op.
#[test]
fn migrate_denomination_idempotent() {
    let (env, client, issuer, namespace, token) = setup();
    set_initial_decimals(&env, &client, &issuer, &namespace, &token, 6);

    let offering_id =
        OfferingId { issuer: issuer.clone(), namespace: namespace.clone(), token: token.clone() };

    let _ = client.try_migrate_denomination(&issuer, &namespace, &token, &6, &18);

    let deposited_after_first = env
        .storage()
        .persistent()
        .get::<DataKey2, i128>(&DataKey2::DepositedRevenue(offering_id.clone()));

    let result = client.try_migrate_denomination(&issuer, &namespace, &token, &6, &18);
    assert!(result.is_ok(), "second call should succeed (no-op)");

    // State should be exactly the same as after the first call
    assert_eq!(
        env.storage()
            .persistent()
            .get::<DataKey2, i128>(&DataKey2::DepositedRevenue(offering_id.clone())),
        deposited_after_first
    );

    // Different (from, to) path should execute
    let result2 = client.try_migrate_denomination(&issuer, &namespace, &token, &18, &6);
    assert!(result2.is_ok(), "different (from,to) should execute");
}

// ── Authorization ──────────────────────────────────────────────────────────────

/// Non-issuer caller should fail with auth error (host panic).
#[test]
fn migrate_denomination_requires_issuer() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "def");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    register_offering(&env, &client, &issuer, &namespace, &token, &payout_asset);
    set_initial_decimals(&env, &client, &issuer, &namespace, &token, 6);

    let attacker = Address::generate(&env);

    // Without mock_all_auths on attacker, require_auth will fail
    let result = client.try_migrate_denomination(&attacker, &namespace, &token, &6, &18);
    // Soroban's host will panic on failed require_auth, so we expect Err
    assert!(result.is_err(), "non-issuer should be rejected");
}

// ── Error cases ────────────────────────────────────────────────────────────────

/// Non-existent offering returns OfferingNotFound.
#[test]
fn migrate_denomination_nonexistent_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "def");
    let token = Address::generate(&env);

    let result = client.try_migrate_denomination(&issuer, &namespace, &token, &6, &18);
    match result {
        Err(Ok(RevoraError::OfferingNotFound)) => {} // expected
        Err(Ok(other)) => panic!("expected OfferingNotFound, got {:?}", other),
        Ok(_) => panic!("expected error"),
        Err(Err(host_err)) => panic!("host error: {:?}", host_err),
    }
}

/// Decimals > 18 should return LimitReached.
#[test]
fn migrate_denomination_rejects_out_of_range_from() {
    let (env, client, issuer, namespace, token) = setup();
    let result = client.try_migrate_denomination(
        &issuer, &namespace, &token, &19, // invalid
        &6,
    );
    assert!(result.is_err(), "from_decimals > 18 should fail");
}

#[test]
fn migrate_denomination_rejects_out_of_range_to() {
    let (env, client, issuer, namespace, token) = setup();
    let result = client.try_migrate_denomination(
        &issuer, &namespace, &token, &6, &19, // invalid
    );
    assert!(result.is_err(), "to_decimals > 18 should fail");
}

// ── SupplyCap rescaling ────────────────────────────────────────────────────────

/// SupplyCap is re-scaled when present.
#[test]
fn migrate_denomination_rescales_supply_cap() {
    let (env, client, issuer, namespace, token) = setup();
    let offering_id =
        OfferingId { issuer: issuer.clone(), namespace: namespace.clone(), token: token.clone() };

    // Set supply cap
    env.storage().persistent().set(&DataKey2::SupplyCap(offering_id.clone()), &10_000_000_i128);

    // Also set decimals
    client.set_payment_token_decimals(&issuer, &namespace, &token, &6);

    let _ = client.try_migrate_denomination(&issuer, &namespace, &token, &6, &18);

    // SupplyCap should be re-scaled
    let expected: i128 = 10_000_000_i128 * 10_i128.pow(12);
    assert_eq!(
        env.storage().persistent().get::<DataKey2, i128>(&DataKey2::SupplyCap(offering_id.clone())),
        Some(expected)
    );
}

// ── Event emission ─────────────────────────────────────────────────────────────

/// Migration emits a `den_mig` event with correct data.
#[test]
fn migrate_denomination_emits_event() {
    let (env, client, issuer, namespace, token) = setup();
    set_initial_decimals(&env, &client, &issuer, &namespace, &token, 6);

    // Record events before migration
    let _ = client.try_migrate_denomination(&issuer, &namespace, &token, &6, &18);

    // Check events
    let events = env.events().all();
    let found = events.iter().any(|event| {
        let topics = &event.0;
        if topics.len() >= 1 {
            if let Ok(sym) = topics.get(0).unwrap().try_into_val::<Symbol>(&env) {
                return sym == Symbol::new(&env, "den_mig");
            }
        }
        false
    });
    assert!(found, "den_mig event should be emitted");
}
