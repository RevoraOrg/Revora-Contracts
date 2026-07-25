#![cfg(test)]
extern crate alloc;

use crate::{RevoraRevenueShare, RevoraRevenueShareClient, MigrationError};
use soroban_sdk::{testutils::{Address as _, Events}, Address, Env, symbol_short};

#[test]
fn test_migrate_storage_success() {

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{
    assert_semver_forward, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient,
    STORAGE_LAYOUT_VERSION,
};

// ─── Existing layout-stamp tests ──────────────────────────────────────────────

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

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    client.set_storage_layout_version(&admin, &(STORAGE_LAYOUT_VERSION + 1)).unwrap();

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
    let issuer = Address::generate(&env);

    // Initial migration should succeed
    client.migrate_storage(&issuer, &1u32, &2u32);

    // Re-invocation at same versions must panic/error with MigrationAlreadyApplied
    client.migrate_storage(&issuer, &1u32, &2u32);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    client.set_storage_layout_version(&admin, &0).unwrap();

    client.set_testnet_mode(&true).unwrap();
    let v = client.storage_layout_version();
    assert_eq!(v, Some(STORAGE_LAYOUT_VERSION));
}

// ─── assert_semver_forward unit tests ─────────────────────────────────────────

#[test]
fn assert_semver_forward_major_upgrade_ok() {
    assert_eq!(assert_semver_forward((1, 0, 0), (2, 0, 0)), Ok(()));
    assert_eq!(assert_semver_forward((1, 5, 3), (2, 0, 0)), Ok(()));
    assert_eq!(assert_semver_forward((1, 0, 23), (2, 0, 0)), Ok(()));
}

#[test]
fn assert_semver_forward_minor_upgrade_ok() {
    assert_eq!(assert_semver_forward((1, 0, 0), (1, 1, 0)), Ok(()));
    assert_eq!(assert_semver_forward((1, 0, 5), (1, 2, 0)), Ok(()));
}

#[test]
fn assert_semver_forward_patch_upgrade_ok() {
    assert_eq!(assert_semver_forward((1, 0, 0), (1, 0, 1)), Ok(()));
    assert_eq!(assert_semver_forward((1, 0, 5), (1, 0, 10)), Ok(()));
}

#[test]
fn assert_semver_forward_multiple_steps() {
    assert_eq!(assert_semver_forward((1, 0, 0), (1, 1, 1)), Ok(()));
    assert_eq!(assert_semver_forward((1, 2, 3), (2, 0, 0)), Ok(()));
    assert_eq!(assert_semver_forward((0, 9, 99), (1, 0, 0)), Ok(()));
}

#[test]
fn assert_semver_forward_noop_rejected() {
    let res = assert_semver_forward((1, 0, 0), (1, 0, 0));
    assert_eq!(res, Err(RevoraError::AlreadyAtTargetVersion));

    let res = assert_semver_forward((2, 5, 10), (2, 5, 10));
    assert_eq!(res, Err(RevoraError::AlreadyAtTargetVersion));
}

#[test]
fn assert_semver_forward_major_downgrade_rejected() {
    let res = assert_semver_forward((2, 0, 0), (1, 0, 0));
    assert_eq!(res, Err(RevoraError::MigrationDowngradeNotAllowed));

    let res = assert_semver_forward((5, 0, 0), (4, 99, 99));
    assert_eq!(res, Err(RevoraError::MigrationDowngradeNotAllowed));
}

#[test]
fn assert_semver_forward_minor_downgrade_rejected() {
    let res = assert_semver_forward((1, 5, 0), (1, 4, 0));
    assert_eq!(res, Err(RevoraError::MigrationDowngradeNotAllowed));

    let res = assert_semver_forward((1, 10, 0), (1, 9, 99));
    assert_eq!(res, Err(RevoraError::MigrationDowngradeNotAllowed));
}

#[test]
fn assert_semver_forward_patch_downgrade_rejected() {
    let res = assert_semver_forward((1, 0, 10), (1, 0, 5));
    assert_eq!(res, Err(RevoraError::MigrationDowngradeNotAllowed));
}

// ─── migrate_storage integration tests ────────────────────────────────────────

fn setup_migration_test() -> (Env, RevoraRevenueShareClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    (env, client, admin)
}

#[test]
fn migrate_storage_major_upgrade_succeeds() {
    let (_, client, admin) = setup_migration_test();
    let res = client.try_migrate_storage(&admin, &2, &0, &0);
    assert_eq!(res, Ok(()));
}

#[test]
fn migrate_storage_minor_upgrade_succeeds() {
    let (_, client, admin) = setup_migration_test();
    let res = client.try_migrate_storage(&admin, &1, &1, &0);
    assert_eq!(res, Ok(()));
}

#[test]
fn migrate_storage_patch_upgrade_succeeds() {
    let (_, client, admin) = setup_migration_test();
    let res = client.try_migrate_storage(&admin, &1, &0, &24);
    assert_eq!(res, Ok(()));
}

#[test]
fn migrate_storage_noop_rejected() {
    let (_, client, admin) = setup_migration_test();
    let res = client.try_migrate_storage(&admin, &1, &0, &23);
    match res {
        Err(Ok(RevoraError::AlreadyAtTargetVersion)) => {}
        other => panic!("expected AlreadyAtTargetVersion, got: {:?}", other),
    }
}

#[test]
fn migrate_storage_major_downgrade_rejected() {
    let (_, client, admin) = setup_migration_test();
    let res = client.try_migrate_storage(&admin, &0, &0, &0);
    match res {
        Err(Ok(RevoraError::MigrationDowngradeNotAllowed)) => {}
        other => panic!("expected MigrationDowngradeNotAllowed, got: {:?}", other),
    }
}

#[test]
fn migrate_storage_minor_downgrade_rejected() {
    let (_, client, admin) = setup_migration_test();
    let res = client.try_migrate_storage(&admin, &1, &0, &22);
    match res {
        Err(Ok(RevoraError::MigrationDowngradeNotAllowed)) => {}
        other => panic!("expected MigrationDowngradeNotAllowed, got: {:?}", other),
    }
}

#[test]
fn migrate_storage_patch_revert_rejected() {
    let (_, client, admin) = setup_migration_test();
    client.migrate_storage(&admin, &1, &0, &30).unwrap();
    let res = client.try_migrate_storage(&admin, &1, &0, &25);
    match res {
        Err(Ok(RevoraError::MigrationDowngradeNotAllowed)) => {}
        other => panic!("expected MigrationDowngradeNotAllowed, got: {:?}", other),
    }
}

#[test]
fn migrate_storage_upgrade_then_upgrade_again() {
    let (_, client, admin) = setup_migration_test();
    client.migrate_storage(&admin, &1, &1, &0).unwrap();
    let res = client.try_migrate_storage(&admin, &2, &0, &0);
    assert_eq!(res, Ok(()));
}

#[test]
fn migrate_storage_uninitialized_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let res = client.try_migrate_storage(&admin, &2, &0, &0);
    match res {
        Err(Ok(RevoraError::NotInitialized)) => {}
        other => panic!("expected NotInitialized, got: {:?}", other),
    }
}

#[test]
fn migrate_storage_non_admin_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let non_admin = Address::generate(&env);

    let res = client.try_migrate_storage(&non_admin, &2, &0, &0);
    match res {
        Err(Ok(RevoraError::NotAuthorized)) => {}
        other => panic!("expected NotAuthorized, got: {:?}", other),
    }
}

#[test]
fn migrate_storage_frozen_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    client.freeze(&admin).unwrap();
    let res = client.try_migrate_storage(&admin, &2, &0, &0);
    match res {
        Err(Ok(RevoraError::ContractFrozen)) => {}
        other => panic!("expected ContractFrozen, got: {:?}", other),
    }
}

#[test]
fn get_version_returns_triple() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let v = client.get_version();
    assert_eq!(v, crate::CONTRACT_VERSION);
    assert_eq!(v.0, 1);
    assert_eq!(v.1, 0);
    assert_eq!(v.2, 23);
}

#[test]
fn migrate_storage_from_version_above_compiled_permits_downgrade_to_compiled() {
    let (_, client, admin) = setup_migration_test();
    client.migrate_storage(&admin, &3, &0, &0).unwrap();
    let res = client.try_migrate_storage(&admin, &2, &0, &0);
    match res {
        Err(Ok(RevoraError::MigrationDowngradeNotAllowed)) => {}
        other => panic!("expected MigrationDowngradeNotAllowed, got: {:?}", other),
    }
}

#[test]
fn migrate_storage_emits_event() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    env.mock_all_auths();
    client.migrate_storage(&admin, &2, &0, &0).unwrap();

    let events = env.events().all();
    let migrate_events: Vec<_> =
        events.iter().filter(|e| e.0.to_string().contains("migrate")).collect();
    assert!(!migrate_events.is_empty(), "expected migrate event to be emitted");
}
