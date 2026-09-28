//! Adversarial coverage for the platform-level per-asset fee getter (#1047).

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, MAX_PLATFORM_FEE_BPS};
use soroban_sdk::{testutils::Address as _, Address, Env};

struct Context {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    admin: Address,
    contract_id: Address,
}

fn setup() -> Context {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    Context { env, client, admin, contract_id }
}

#[test]
fn getter_returns_zero_for_unconfigured_assets_before_initialization() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let generated_asset = Address::generate(&env);

    assert_eq!(client.get_platform_fee_per_asset(&generated_asset), 0);
    assert_eq!(client.get_platform_fee_per_asset(&contract_id), 0);
}

#[test]
fn getter_returns_configured_zero_and_maximum_values_per_asset() {
    let c = setup();
    let zero_fee_asset = Address::generate(&c.env);
    let maximum_fee_asset = Address::generate(&c.env);

    c.client.set_platform_fee_per_asset(&zero_fee_asset, &0);
    c.client.set_platform_fee_per_asset(&maximum_fee_asset, &MAX_PLATFORM_FEE_BPS);

    assert_eq!(c.client.get_platform_fee_per_asset(&zero_fee_asset), 0);
    assert_eq!(c.client.get_platform_fee_per_asset(&maximum_fee_asset), MAX_PLATFORM_FEE_BPS);
}

#[test]
fn getter_isolated_by_address_including_contract_address() {
    let c = setup();
    let asset = Address::generate(&c.env);

    c.client.set_platform_fee_per_asset(&asset, &1);

    assert_eq!(c.client.get_platform_fee_per_asset(&asset), 1);
    assert_eq!(c.client.get_platform_fee_per_asset(&c.contract_id), 0);
    assert_eq!(c.client.get_platform_fee_per_asset(&c.admin), 0);
}

#[test]
fn rejected_fee_update_does_not_change_existing_getter_value() {
    let c = setup();
    let asset = Address::generate(&c.env);
    c.client.set_platform_fee_per_asset(&asset, &MAX_PLATFORM_FEE_BPS);

    let result = c.client.try_set_platform_fee_per_asset(&asset, &5_001);

    assert_eq!(result, Err(Ok(RevoraError::InvalidRevenueShareBps)));
    assert_eq!(c.client.get_platform_fee_per_asset(&asset), MAX_PLATFORM_FEE_BPS);
}

#[test]
fn rejected_uninitialized_update_leaves_getter_at_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let asset = Address::generate(&env);

    let result = client.try_set_platform_fee_per_asset(&asset, &1);

    assert_eq!(result, Err(Ok(RevoraError::NotInitialized)));
    assert_eq!(client.get_platform_fee_per_asset(&asset), 0);
}