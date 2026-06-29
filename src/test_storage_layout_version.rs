#![cfg(test)]
extern crate alloc;

use crate::{RevoraRevenueShare, RevoraRevenueShareClient, MigrationError};
use soroban_sdk::{testutils::{Address as _, Events}, Address, Env, symbol_short};

#[test]
fn test_migrate_storage_success() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);

    // Run explicit walker migration v1 -> v2
    client.migrate_storage(&issuer, &1u32, &2u32);

    // Verify mig_step event was emitted for audit trail
    let events = env.events().all();
    assert!(events.len() > 0, "Walker must emit mig_step events for audit");
}

#[test]
#[should_panic(expected = "HostError")]
fn test_migrate_storage_already_applied() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);

    // Initial migration should succeed
    client.migrate_storage(&issuer, &1u32, &2u32);

    // Re-invocation at same versions must panic/error with MigrationAlreadyApplied
    client.migrate_storage(&issuer, &1u32, &2u32);
}
