#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract);
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    (env, contract, admin, asset)
}

#[test]
fn stores_valid_fee_and_accepts_maximum_boundary() {
    let (env, contract, _admin, asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract);

    client.set_platform_fee_per_asset(&asset, &1_250);
    assert_eq!(client.get_platform_fee_per_asset(&asset), 1_250);
    client.set_platform_fee_per_asset(&asset, &5_000);
    assert_eq!(client.get_platform_fee_per_asset(&asset), 5_000);
}

#[test]
fn rejects_values_above_maximum_without_mutating_state() {
    let (env, contract, _admin, asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract);
    client.set_platform_fee_per_asset(&asset, &2_000);

    assert_eq!(
        client.try_set_platform_fee_per_asset(&asset, &5_001),
        Err(Ok(RevoraError::InvalidRevenueShareBps))
    );
    assert_eq!(client.get_platform_fee_per_asset(&asset), 2_000);
}

#[test]
fn rejects_uninitialized_contract_without_creating_asset_state() {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract);
    let asset = Address::generate(&env);

    assert_eq!(
        client.try_set_platform_fee_per_asset(&asset, &100),
        Err(Ok(RevoraError::NotInitialized))
    );
    assert_eq!(client.get_platform_fee_per_asset(&asset), 0);
}
