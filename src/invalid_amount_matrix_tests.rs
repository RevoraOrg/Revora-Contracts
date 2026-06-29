//! Invalid amount matrix coverage for all amount-bearing Revora entrypoints.
//!
//! Security assumptions:
//! - Signed `i128` inputs coming from SDK wrappers are untrusted until validated on-chain.
//! - Rejected negative or zero deposit paths must not mutate offering state.
//! - Failed threshold and stake updates must preserve the previously stored configuration.
//! - Fee calculation is intentionally excluded from `InvalidAmount` rejection because the
//!   current public fee surface in this branch is a pure quote helper, not a mutating entrypoint.


use crate::{InvestmentConstraintsConfig, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, token, Address, Env};

fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract(admin.clone());
    (token_id, admin)
}

fn mint(env: &Env, payment_token: &Address, recipient: &Address, amount: i128) {
    token::StellarAssetClient::new(env, payment_token).mint(recipient, &amount);
}

fn setup_offering() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payout_asset,
        &0,
    );

    (env, contract_id, issuer, token, payout_asset)
}

fn setup_funded_offering() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let (payment_token, _payment_admin) = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payment_token,
        &0,
    );
    mint(&env, &payment_token, &issuer, 1_000_000);

    (env, contract_id, issuer, token, payment_token)
}

fn setup_snapshot_offering() -> (Env, Address, Address, Address, Address) {
    let (env, contract_id, issuer, token, payment_token) = setup_funded_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);
    (env, contract_id, issuer, token, payment_token)
}

#[test]
fn register_offering_rejects_negative_supply_cap_values() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    for invalid_cap in [-1_i128, i128::MIN] {
        let result = client.try_register_offering(
            &issuer,
            &symbol_short!("def"),
            &token,
            &1_000,
            &payout_asset,
            &invalid_cap,
        );
        assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
        assert_eq!(client.get_offering_count(&issuer, &symbol_short!("def")), 0);
        assert!(client.get_offering(&issuer, &symbol_short!("def"), &token).is_none());
    }
}

#[test]
fn report_revenue_rejects_negative_amount_boundaries_without_audit_mutation() {
    let (env, contract_id, issuer, token, payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    for invalid_amount in [-1_i128, i128::MIN] {
        let result = client.try_report_revenue(
            &issuer,
            &symbol_short!("def"),
            &token,
            &payout_asset,
            &invalid_amount,
            &1,
            &false,
        );
        assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
        assert!(client
            .get_audit_summary(&issuer, &symbol_short!("def"), &token)
            .is_none());
    }
}

#[test]
fn deposit_revenue_rejects_non_positive_amounts_without_mutating_period_state() {
    let (env, contract_id, issuer, token, payment_token) = setup_funded_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    for invalid_amount in [0_i128, -1_i128, i128::MIN] {
        let result = client.try_deposit_revenue(
            &issuer,
            &symbol_short!("def"),
            &token,
            &payment_token,
            &invalid_amount,
            &1,
        );
        assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
        assert_eq!(client.get_offering_count(&issuer, &symbol_short!("def")), 1);
    }
}

#[test]
fn deposit_revenue_with_snapshot_rejects_non_positive_amounts_without_state_changes() {
    let (env, contract_id, issuer, token, payment_token) = setup_snapshot_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    for invalid_amount in [0_i128, -1_i128, i128::MIN] {
        let result = client.try_deposit_revenue_with_snapshot(
            &issuer,
            &symbol_short!("def"),
            &token,
            &payment_token,
            &invalid_amount,
            &1,
            &1,
        );
        assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
        assert_eq!(client.get_offering_count(&issuer, &symbol_short!("def")), 1);
        assert_eq!(client.get_last_snapshot_ref(&issuer, &symbol_short!("def"), &token), 0);
    }
}

#[test]
fn deposit_revenue_with_snapshot_rejects_zero_snapshot_reference_without_state_changes() {
    let (env, contract_id, issuer, token, payment_token) = setup_snapshot_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100,
        &1,
        &0,
    );

    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
    assert_eq!(client.get_offering_count(&issuer, &symbol_short!("def")), 1);
    assert_eq!(client.get_last_snapshot_ref(&issuer, &symbol_short!("def"), &token), 0);
}

#[test]
fn set_investment_constraints_rejects_negative_min_stake() {
    let (env, contract_id, issuer, token, _payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let result =
        client.try_set_investment_constraints(&issuer, &symbol_short!("def"), &token, &-1, &100);

    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
    assert!(client
        .get_investment_constraints(&issuer, &symbol_short!("def"), &token)
        .is_none());
}

#[test]
fn set_investment_constraints_rejects_negative_max_stake() {
    let (env, contract_id, issuer, token, _payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let result =
        client.try_set_investment_constraints(&issuer, &symbol_short!("def"), &token, &100, &-1);

    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
    assert!(client
        .get_investment_constraints(&issuer, &symbol_short!("def"), &token)
        .is_none());
}

#[test]
fn set_investment_constraints_rejects_invalid_range_without_overwriting_previous_config() {
    let (env, contract_id, issuer, token, _payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &100, &500);

    let result =
        client.try_set_investment_constraints(&issuer, &symbol_short!("def"), &token, &600, &500);

    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
    assert_eq!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token),
        Some(InvestmentConstraintsConfig { min_stake: 100, max_stake: 500 })
    );
}

#[test]
fn set_min_revenue_threshold_rejects_negative_transition_without_overwriting_previous_value() {
    let (env, contract_id, issuer, token, _payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &250);

    let result = client.try_set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &-1);

    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
    assert_eq!(
        client.get_min_revenue_threshold(&issuer, &symbol_short!("def"), &token),
        250
    );
}
