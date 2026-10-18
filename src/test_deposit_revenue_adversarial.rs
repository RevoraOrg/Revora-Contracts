//! Adversarial and boundary test coverage for `deposit_revenue` in `src/lib.rs`.

#![cfg(test)]
#![allow(unused_imports)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    token, Address, Env, Symbol, Vec,
};

// ── Test Setup Helpers ────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    (token_id, admin)
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    token::StellarAssetClient::new(env, token).mint(to, &amount);
}

fn get_balance(env: &Env, token: &Address, account: &Address) -> i128 {
    token::Client::new(env, token).balance(account)
}

fn setup_offering(
) -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (payment_token, _pt_admin) = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &offering_token,
        &10_000u32,
        &payment_token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
    mint(&env, &payment_token, &issuer, 10_000_000);

    (env, client, issuer, offering_token, payment_token, contract_id)
}

// ── Test Cases ────────────────────────────────────────────────────────────────

#[test]
fn test_deposit_revenue_valid_call_succeeds() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();

    let res = client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );
    assert_eq!(res, ());

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 1);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 100_000);
    assert_eq!(
        client.get_payment_token(&issuer, &symbol_short!("def"), &token),
        Some(payment_token.clone())
    );
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 100_000);
}

#[test]
fn test_deposit_revenue_contract_frozen_rejected() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();
    let admin = Address::generate(&env);
    let safety = Address::generate(&env);

    client.initialize(&admin, &Some(safety), &None::<bool>);
    client.freeze(&admin);

    let res = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );
    assert_eq!(res, Err(Ok(RevoraError::ContractFrozen)));

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 0);
}

#[test]
fn test_deposit_revenue_contract_paused_rejected() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();
    let admin = Address::generate(&env);
    let safety = Address::generate(&env);

    client.initialize(&admin, &Some(safety), &None::<bool>);
    client.pause(&admin);

    let res = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );
    assert_eq!(res, Err(Ok(RevoraError::ContractPaused)));

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 0);
}

#[test]
fn test_deposit_revenue_unregistered_offering_rejected() {
    let (env, client, issuer, _token, payment_token, contract_id) = setup_offering();
    let unreg_token = Address::generate(&env);

    let res = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &unreg_token,
        &payment_token,
        &100_000,
        &1,
    );
    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &unreg_token), 0);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &unreg_token), 0);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 0);
}

#[test]
fn test_deposit_revenue_wrong_issuer_rejected() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();
    let wrong_issuer = Address::generate(&env);

    let res = client.try_deposit_revenue(
        &wrong_issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );
    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 0);
}

#[test]
fn test_deposit_revenue_wrong_namespace_rejected() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();

    let res = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("badns"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );
    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 0);
}

#[test]
fn test_deposit_revenue_payment_token_mismatch_after_lock() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();
    let (other_payment_token, _other_admin) = create_payment_token(&env);

    client
        .deposit_revenue(
            &issuer,
            &symbol_short!("def"),
            &token,
            &payment_token,
            &100_000,
            &1,
        )
        .unwrap();

    let res = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &other_payment_token,
        &50_000,
        &2,
    );
    assert_eq!(res, Err(Ok(RevoraError::PaymentTokenMismatch)));

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 1);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 100_000);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 100_000);
    assert_eq!(get_balance(&env, &other_payment_token, &contract_id), 0);
}

#[test]
fn test_deposit_revenue_zero_amount_rejected() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();

    let res = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &0,
        &1,
    );
    assert_eq!(res, Err(Ok(RevoraError::InvalidAmount)));

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 0);
}

#[test]
fn test_deposit_revenue_negative_amount_rejected() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();

    let res_sub = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &-100,
        &1,
    );
    assert_eq!(res_sub, Err(Ok(RevoraError::InvalidAmount)));

    let res_min = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &i128::MIN,
        &1,
    );
    assert_eq!(res_min, Err(Ok(RevoraError::InvalidAmount)));

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 0);
}

#[test]
fn test_deposit_revenue_zero_period_id_rejected() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();

    let res = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &0,
    );
    assert_eq!(res, Err(Ok(RevoraError::InvalidPeriodId)));

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 0);
}

#[test]
fn test_deposit_revenue_duplicate_period_id_rejected() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();

    client
        .deposit_revenue(
            &issuer,
            &symbol_short!("def"),
            &token,
            &payment_token,
            &100_000,
            &1,
        )
        .unwrap();

    let res = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );
    assert_eq!(res, Err(Ok(RevoraError::PeriodAlreadyDeposited)));

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 1);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 100_000);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 100_000);
}

#[test]
fn test_deposit_revenue_gap_period_id_rejected() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();

    client
        .deposit_revenue(
            &issuer,
            &symbol_short!("def"),
            &token,
            &payment_token,
            &100_000,
            &1,
        )
        .unwrap();

    let res = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &3,
    );
    assert_eq!(res, Err(Ok(RevoraError::InvalidPeriodId)));

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 1);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 100_000);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 100_000);
}

#[test]
fn test_deposit_revenue_insufficient_balance_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (payment_token, _pt_admin) = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &offering_token,
        &10_000u32,
        &payment_token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    let res = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token,
        &100_000,
        &1,
    );
    assert!(res.is_err(), "deposit must fail when issuer has insufficient balance");

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &offering_token), 0);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &offering_token), 0);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 0);
}

#[test]
fn test_deposit_revenue_multi_period_sequential_success() {
    let (env, client, issuer, token, payment_token, contract_id) = setup_offering();

    client
        .deposit_revenue(
            &issuer,
            &symbol_short!("def"),
            &token,
            &payment_token,
            &10_000,
            &1,
        )
        .unwrap();

    client
        .deposit_revenue(
            &issuer,
            &symbol_short!("def"),
            &token,
            &payment_token,
            &20_000,
            &2,
        )
        .unwrap();

    client
        .deposit_revenue(
            &issuer,
            &symbol_short!("def"),
            &token,
            &payment_token,
            &30_000,
            &3,
        )
        .unwrap();

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 3);
    assert_eq!(client.get_deposited_revenue(&issuer, &symbol_short!("def"), &token), 60_000);
    assert_eq!(get_balance(&env, &payment_token, &contract_id), 60_000);
}
