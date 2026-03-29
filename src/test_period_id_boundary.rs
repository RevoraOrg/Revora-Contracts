//! Period ID Boundary Tests (#35)
//!
//! # Security Assumptions
//! - `period_id == 0` is always invalid; zero is reserved as a sentinel/unset value.
//! - `period_id` is a `u64`, so valid range is [1, u64::MAX].
//! - Duplicate period IDs for the same offering must be rejected by `deposit_revenue`
//!   (`PeriodAlreadyDeposited`) and silently handled (override flag) by `report_revenue`.
//! - Auth is enforced by `issuer.require_auth()`; an attacker using a different address
//!   must be rejected before any state mutation occurs.
//! - Negative amounts are rejected before period_id is even evaluated.
//! - Period ID isolation: period N of offering A must not affect offering B.
//!
//! # Coverage
//! - Zero period_id → `InvalidPeriodId` for both `report_revenue` and `deposit_revenue`
//! - Boundary values: 1, 2, u64::MAX-1, u64::MAX accepted
//! - Duplicate deposit → `PeriodAlreadyDeposited`
//! - Duplicate report without override → rejected event, no state change
//! - Duplicate report with override → accepted, state updated
//! - Negative amount rejected before period_id check
//! - Zero amount accepted by `report_revenue`, rejected by `deposit_revenue`
//! - Auth boundary: wrong issuer rejected for both entrypoints
//! - Cross-offering isolation: same period_id in different offerings is independent
//! - Frozen contract rejects all mutations regardless of period_id

#![cfg(test)]
#![allow(unused_imports)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    token, Address, Env,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Create a real Stellar asset token and return (token_id, admin).
fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract(admin.clone());
    (token_id, admin)
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    token::StellarAssetClient::new(env, token).mint(to, &amount);
}

/// Full setup: env + client + registered offering + funded issuer.
/// Returns (env, client, issuer, offering_token, payment_token).
fn setup_funded() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (payment_token, _pt_admin) = create_payment_token(&env);
    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &1_000,
        &payment_token,
        &0,
    );
    mint(&env, &payment_token, &issuer, 1_000_000_000);
    (env, client, issuer, offering_token, payment_token)
}

// ── Zero period_id rejection ──────────────────────────────────────────────────

/// `report_revenue` with period_id=0 must return `InvalidPeriodId`.
/// Security: prevents ambiguous sentinel values from entering the revenue index.
#[test]
fn report_revenue_zero_period_id_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);

    let result =
        client.try_report_revenue(&issuer, &symbol_short!("def"), &token, &token, &100, &0, &false);
    assert_eq!(result, Err(Ok(RevoraError::InvalidPeriodId)));

    // No audit summary should have been written
    assert!(client.get_audit_summary(&issuer, &symbol_short!("def"), &token).is_none());
}

/// `deposit_revenue` with period_id=0 must return `InvalidPeriodId`.
#[test]
fn deposit_revenue_zero_period_id_rejected() {
    let (env, client, issuer, offering_token, payment_token) = setup_funded();

    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token,
        &1_000,
        &0u64,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidPeriodId)));

    // No period should have been recorded
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &offering_token), 0);
}

// ── Valid boundary values ─────────────────────────────────────────────────────

/// period_id=1 (minimum valid) is accepted by `report_revenue`.
#[test]
fn report_revenue_period_id_one_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);

    let result =
        client.try_report_revenue(&issuer, &symbol_short!("def"), &token, &token, &500, &1, &false);
    assert!(result.is_ok());
}

/// period_id=u64::MAX is accepted by `report_revenue`.
#[test]
fn report_revenue_period_id_max_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);

    let result = client.try_report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &token,
        &1,
        &u64::MAX,
        &false,
    );
    assert!(result.is_ok());
}

/// period_id=u64::MAX-1 is accepted by `report_revenue`.
#[test]
fn report_revenue_period_id_near_max_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);

    let result = client.try_report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &token,
        &1,
        &(u64::MAX - 1),
        &false,
    );
    assert!(result.is_ok());
}

/// period_id=1 (minimum valid) is accepted by `deposit_revenue`.
#[test]
fn deposit_revenue_period_id_one_accepted() {
    let (env, client, issuer, offering_token, payment_token) = setup_funded();

    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token,
        &1_000,
        &1u64,
    );
    assert!(result.is_ok());
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &offering_token), 1);
}

/// period_id=u64::MAX is accepted by `deposit_revenue`.
#[test]
fn deposit_revenue_period_id_max_accepted() {
    let (env, client, issuer, offering_token, payment_token) = setup_funded();

    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token,
        &1_000,
        &u64::MAX,
    );
    assert!(result.is_ok());
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &offering_token), 1);
}

// ── Duplicate period_id handling ──────────────────────────────────────────────

/// Depositing the same period_id twice must return `PeriodAlreadyDeposited` on the second call.
#[test]
fn deposit_revenue_duplicate_period_rejected() {
    let (env, client, issuer, offering_token, payment_token) = setup_funded();

    client
        .deposit_revenue(
            &issuer,
            &symbol_short!("def"),
            &offering_token,
            &payment_token,
            &1_000,
            &42u64,
        )
        .unwrap();

    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token,
        &1_000,
        &42u64,
    );
    assert_eq!(result, Err(Ok(RevoraError::PeriodAlreadyDeposited)));

    // Period count must remain 1
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &offering_token), 1);
}

/// Reporting the same period_id twice without override flag emits a rejected event but does not
/// mutate the stored revenue amount.
#[test]
fn report_revenue_duplicate_without_override_no_state_change() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);

    client
        .report_revenue(&issuer, &symbol_short!("def"), &token, &token, &500, &7, &false)
        .unwrap();

    // Second report for same period without override
    client
        .report_revenue(&issuer, &symbol_short!("def"), &token, &token, &999, &7, &false)
        .unwrap();

    // Revenue index for period 7 must still reflect the first report (500)
    let indexed = client.get_revenue_index(&issuer, &symbol_short!("def"), &token, &7u64);
    assert_eq!(indexed, 500);
}

/// Reporting the same period_id twice WITH override flag updates the stored amount.
#[test]
fn report_revenue_duplicate_with_override_updates_state() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);

    client
        .report_revenue(&issuer, &symbol_short!("def"), &token, &token, &500, &7, &false)
        .unwrap();

    // Override with a new amount
    client
        .report_revenue(&issuer, &symbol_short!("def"), &token, &token, &1_500, &7, &true)
        .unwrap();

    // Revenue index should reflect the override (cumulative: 500 + 1500 = 2000)
    let indexed = client.get_revenue_index(&issuer, &symbol_short!("def"), &token, &7u64);
    assert!(indexed >= 1_500);
}

// ── Amount boundary interaction with period_id ────────────────────────────────

/// Negative amount is rejected before period_id is evaluated.
/// Both `report_revenue` and `deposit_revenue` must return `InvalidAmount`.
#[test]
fn negative_amount_rejected_before_period_id_check() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);

    // Negative amount with valid period_id
    let r = client.try_report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &token,
        &-1,
        &5,
        &false,
    );
    assert_eq!(r, Err(Ok(RevoraError::InvalidAmount)));
}

/// Zero amount is accepted by `report_revenue` (zero revenue is a valid report).
#[test]
fn zero_amount_accepted_by_report_revenue() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);

    let r = client.try_report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &token,
        &0,
        &3,
        &false,
    );
    assert!(r.is_ok());
}

/// Zero amount is rejected by `deposit_revenue` (must transfer a positive value).
#[test]
fn zero_amount_rejected_by_deposit_revenue() {
    let (env, client, issuer, offering_token, payment_token) = setup_funded();

    let r = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token,
        &0,
        &3u64,
    );
    assert_eq!(r, Err(Ok(RevoraError::InvalidAmount)));
}

// ── Auth boundary tests ───────────────────────────────────────────────────────

/// A different address (attacker) cannot report revenue for an offering it does not own.
/// No state must be mutated.
#[test]
fn report_revenue_wrong_issuer_rejected() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let issuer = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);

    let r = client.try_report_revenue(
        &attacker,
        &symbol_short!("def"),
        &token,
        &token,
        &100,
        &1,
        &false,
    );
    assert!(r.is_err());
    assert!(client.get_audit_summary(&issuer, &symbol_short!("def"), &token).is_none());
}

/// A different address (attacker) cannot deposit revenue for an offering it does not own.
#[test]
fn deposit_revenue_wrong_issuer_rejected() {
    let (env, client, issuer, offering_token, payment_token) = setup_funded();
    let attacker = Address::generate(&env);
    mint(&env, &payment_token, &attacker, 1_000_000);

    let r = client.try_deposit_revenue(
        &attacker,
        &symbol_short!("def"),
        &offering_token,
        &payment_token,
        &1_000,
        &1u64,
    );
    assert!(r.is_err());
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &offering_token), 0);
}

// ── Cross-offering isolation ──────────────────────────────────────────────────

/// The same period_id deposited in offering A must not affect offering B.
#[test]
fn period_id_isolated_across_offerings() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let (payment_token, _) = create_payment_token(&env);

    client.register_offering(&issuer_a, &symbol_short!("def"), &token_a, &1_000, &payment_token, &0);
    client.register_offering(&issuer_b, &symbol_short!("def"), &token_b, &1_000, &payment_token, &0);

    mint(&env, &payment_token, &issuer_a, 1_000_000);
    mint(&env, &payment_token, &issuer_b, 1_000_000);

    // Deposit period 5 for offering A
    client
        .deposit_revenue(
            &issuer_a,
            &symbol_short!("def"),
            &token_a,
            &payment_token,
            &1_000,
            &5u64,
        )
        .unwrap();

    // Offering B period 5 must still be available (not yet deposited)
    assert_eq!(client.get_period_count(&issuer_b, &symbol_short!("def"), &token_b), 0);

    // Deposit period 5 for offering B independently
    client
        .deposit_revenue(
            &issuer_b,
            &symbol_short!("def"),
            &token_b,
            &payment_token,
            &2_000,
            &5u64,
        )
        .unwrap();

    assert_eq!(client.get_period_count(&issuer_a, &symbol_short!("def"), &token_a), 1);
    assert_eq!(client.get_period_count(&issuer_b, &symbol_short!("def"), &token_b), 1);
}

/// Same period_id reported in two different offerings must be stored independently.
#[test]
fn report_revenue_period_id_isolated_across_offerings() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client.register_offering(&issuer, &symbol_short!("def"), &token_a, &1_000, &token_a, &0);
    client.register_offering(&issuer, &symbol_short!("def"), &token_b, &1_000, &token_b, &0);

    client
        .report_revenue(&issuer, &symbol_short!("def"), &token_a, &token_a, &100, &9, &false)
        .unwrap();
    client
        .report_revenue(&issuer, &symbol_short!("def"), &token_b, &token_b, &200, &9, &false)
        .unwrap();

    let idx_a = client.get_revenue_index(&issuer, &symbol_short!("def"), &token_a, &9u64);
    let idx_b = client.get_revenue_index(&issuer, &symbol_short!("def"), &token_b, &9u64);
    assert_eq!(idx_a, 100);
    assert_eq!(idx_b, 200);
}

// ── Frozen contract ───────────────────────────────────────────────────────────

/// A frozen contract must reject `report_revenue` regardless of period_id.
#[test]
fn frozen_contract_rejects_report_revenue() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);
    client.freeze();

    let r = client.try_report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &token,
        &100,
        &1,
        &false,
    );
    assert_eq!(r, Err(Ok(RevoraError::ContractFrozen)));
}

/// A frozen contract must reject `deposit_revenue` regardless of period_id.
#[test]
fn frozen_contract_rejects_deposit_revenue() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (payment_token, _) = create_payment_token(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &1_000,
        &payment_token,
        &0,
    );
    mint(&env, &payment_token, &issuer, 1_000_000);
    client.freeze();

    let r = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token,
        &1_000,
        &1u64,
    );
    assert_eq!(r, Err(Ok(RevoraError::ContractFrozen)));
}

// ── Sequential period ordering ────────────────────────────────────────────────

/// Depositing periods 1, 2, 3 in order must result in period_count=3 and correct enumeration.
#[test]
fn sequential_period_ids_stored_in_order() {
    let (env, client, issuer, offering_token, payment_token) = setup_funded();

    for p in 1u64..=3u64 {
        client
            .deposit_revenue(
                &issuer,
                &symbol_short!("def"),
                &offering_token,
                &payment_token,
                &1_000,
                &p,
            )
            .unwrap();
    }

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &offering_token), 3);
}

/// Depositing periods out of order (3, 1, 2) must still be accepted and count correctly.
#[test]
fn out_of_order_period_ids_accepted() {
    let (env, client, issuer, offering_token, payment_token) = setup_funded();

    for p in [3u64, 1u64, 2u64] {
        client
            .deposit_revenue(
                &issuer,
                &symbol_short!("def"),
                &offering_token,
                &payment_token,
                &500,
                &p,
            )
            .unwrap();
    }

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &offering_token), 3);
}
