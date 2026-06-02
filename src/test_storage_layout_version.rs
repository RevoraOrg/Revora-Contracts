#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{RevoraRevenueShareClient, RevoraError, RevoraRevenueShare, STORAGE_LAYOUT_VERSION};

#[test]
fn initialize_writes_storage_layout_version() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let v = client.storage_layout_version();
    assert_eq!(v, Some(STORAGE_LAYOUT_VERSION));
}

#[test]
fn downgrade_attempt_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    // Admin sets the on-chain layout to a newer value (simulating a migrated state)
    client.set_storage_layout_version(&admin, &(STORAGE_LAYOUT_VERSION + 1)).unwrap();

    // Any state-changing entrypoint should now be rejected due to newer on-chain layout.
    let res = client.set_testnet_mode(&true);
    match res {
        Err(Ok(RevoraError::MigrationDowngradeNotAllowed)) => {}
        other => panic!("expected MigrationDowngradeNotAllowed, got: {:?}", other),
    }
}

#[test]
fn upgrade_path_allows_operation_and_stamps_layout() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    // Simulate older on-chain layout (0) and ensure operations proceed and storage is stamped.
    client.set_storage_layout_version(&admin, &0).unwrap();

    // This should succeed and cause the contract to stamp the layout to the compiled value.
    client.set_testnet_mode(&true).unwrap();
    let v = client.storage_layout_version();
    assert_eq!(v, Some(STORAGE_LAYOUT_VERSION));
}
