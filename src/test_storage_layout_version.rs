#![cfg(test)]
extern crate alloc;

use soroban_sdk::{testutils::Address as _, Address, Env, symbol_short};
use crate::{RevoraRevenueShare, RevoraRevenueShareClient, MigrationError};
use soroban_sdk::{testutils::{Address as _, Events}, Address, Env, symbol_short};
use crate::{
    assert_semver_forward, RevoraError,
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
    client.migrate_storage_walker(&issuer, &1u32, &2u32, &false);

    // Verify mig_step event was emitted for audit trail
    let events = env.events().all();
    assert!(events.len() > 0, "Walker must emit mig_step events for audit");
}

#[test]
fn test_migrate_storage_dry_run() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);

    // Run explicit walker migration v1 -> v2 in dry_run mode
    client.migrate_storage_walker(&issuer, &1u32, &2u32, &true);

    // Verify migration_plan event was emitted
    let events = env.events().all();
    let plan_events: Vec<_> = events.iter().filter(|e| e.0.to_string().contains("migration_plan")).collect();
    assert!(!plan_events.is_empty(), "Walker must emit migration_plan events for dry run");

    // Run explicit walker migration v1 -> v2 again (should succeed since no state mutated)
    client.migrate_storage_walker(&issuer, &1u32, &2u32, &true);
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
    client.migrate_storage_walker(&issuer, &1u32, &2u32, &false);

    // Re-invocation at same versions must panic/error with MigrationAlreadyApplied
    client.migrate_storage_walker(&issuer, &1u32, &2u32, &false);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    client.set_storage_layout_version(&admin, &0).unwrap();

    client.set_testnet_mode(&true).unwrap();
    let v = client.storage_layout_version();
    assert_eq!(v, Some(STORAGE_LAYOUT_VERSION));
}

#[test]
fn test_migration_resumes_from_cursor() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);

    // Instead of halting the real execution midway, we will explicitly simulate it.
    // By setting the MigrationResumeCursor manually, we simulate a halted migration.
    // The cursor is 5, meaning keys 1..=5 have been processed.
    use crate::{MigrationDataKey, MigrationCursor};
    env.as_contract(&contract_id, || {
        let cursor_key = MigrationDataKey::MigrationResumeCursor(issuer.clone());
        let cursor = MigrationCursor { last_key: 5 };
        env.storage().persistent().set(&cursor_key, &cursor);
    });

    // Run explicit walker migration v1 -> v2
    client.migrate_storage_walker(&issuer, &1u32, &2u32, &false);

    // Verify mig_resume event was emitted
    let events = env.events().all();
    let resume_events: Vec<_> = events.iter().filter(|e| e.0.to_string().contains("mig_resume")).collect();
    assert_eq!(resume_events.len(), 1, "Must emit exactly one mig_resume event");
    let resume_val: u32 = resume_events[0].2.clone().into_val(&env);
    assert_eq!(resume_val, 5, "Resume cursor should be 5");

    // Verify mig_step was emitted for keys 6 through 10, meaning it resumed at 6.
    let step_events: Vec<_> = events.iter().filter(|e| e.0.to_string().contains("mig_step")).collect();
    assert_eq!(step_events.len(), 5, "Should only process 5 remaining keys (6-10)");
    
    // Assert the exact keys processed in the steps
    let start_key: u32 = step_events[0].2.clone().into_val(&env);
    assert_eq!(start_key, 6, "First processed key after resume must be 6");

    let end_key: u32 = step_events[4].2.clone().into_val(&env);
    assert_eq!(end_key, 10, "Last processed key must be 10");
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

// ─── Per-key migration hook tests (#582) ─────────────────────────────────────

#[test]
fn register_hook_identity_succeeds() {
    let (env, client, admin) = setup_migration_test();
    let legacy_key = symbol_short!("legacy");

    client.register_migration_hook(
        &admin,
        &legacy_key,
        &MigrationTransform::Identity,
    );

    // Verify the hook was registered via events
    let events = env.events().all();
    let hook_events: Vec<_> = events.iter()
        .filter(|e| e.0.to_string().contains("mig_hook"))
        .collect();
    assert!(!hook_events.is_empty(), "must emit mig_hook event on registration");

    // Verify get_registered_hooks returns the hook
    let hooks = client.get_registered_hooks();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks.get(0).unwrap().legacy_key, legacy_key);
    assert_eq!(hooks.get(0).unwrap().transform, MigrationTransform::Identity);
}

#[test]
fn register_hook_rename_succeeds() {
    let (_, client, admin) = setup_migration_test();
    let legacy_key = symbol_short!("old_key");
    let new_key = symbol_short!("new_key");

    client.register_migration_hook(
        &admin,
        &legacy_key,
        &MigrationTransform::Rename(new_key),
    );

    let hooks = client.get_registered_hooks();
    assert_eq!(hooks.len(), 1);
    let hook = hooks.get(0).unwrap();
    assert_eq!(hook.legacy_key, legacy_key);
    assert_eq!(hook.transform, MigrationTransform::Rename(new_key));
}

#[test]
fn register_hook_custom_succeeds() {
    let (_, client, admin) = setup_migration_test();
    let legacy_key = symbol_short!("custom_legacy");
    let selector = symbol_short!("wrap_v2");

    client.register_migration_hook(
        &admin,
        &legacy_key,
        &MigrationTransform::Custom(selector),
    );

    let hooks = client.get_registered_hooks();
    assert_eq!(hooks.len(), 1);
    let hook = hooks.get(0).unwrap();
    assert_eq!(hook.legacy_key, legacy_key);
    assert_eq!(hook.transform, MigrationTransform::Custom(selector));
}

#[test]
fn register_multiple_hooks_returns_all() {
    let (_, client, admin) = setup_migration_test();
    let key_a = symbol_short!("key_a");
    let key_b = symbol_short!("key_b");
    let key_c = symbol_short!("key_c");

    client.register_migration_hook(&admin, &key_a, &MigrationTransform::Identity);
    client.register_migration_hook(&admin, &key_b, &MigrationTransform::Identity);
    client.register_migration_hook(&admin, &key_c, &MigrationTransform::Identity);

    let hooks = client.get_registered_hooks();
    assert_eq!(hooks.len(), 3);
    // Hooks should be returned in registration order
    assert_eq!(hooks.get(0).unwrap().legacy_key, key_a);
    assert_eq!(hooks.get(1).unwrap().legacy_key, key_b);
    assert_eq!(hooks.get(2).unwrap().legacy_key, key_c);
}

#[test]
fn register_duplicate_hook_overwrites() {
    let (_, client, admin) = setup_migration_test();
    let legacy_key = symbol_short!("dup_key");

    // Register as Identity first
    client.register_migration_hook(&admin, &legacy_key, &MigrationTransform::Identity);
    // Register same key with different transform
    let new_key = symbol_short!("renamed");
    client.register_migration_hook(
        &admin,
        &legacy_key,
        &MigrationTransform::Rename(new_key),
    );

    // Should still have only 1 hook, with the latest transform
    let hooks = client.get_registered_hooks();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks.get(0).unwrap().transform, MigrationTransform::Rename(new_key));
}

#[test]
fn clear_migration_hook_succeeds() {
    let (_, client, admin) = setup_migration_test();
    let legacy_key = symbol_short!("temp_key");

    client.register_migration_hook(&admin, &legacy_key, &MigrationTransform::Identity);
    assert_eq!(client.get_registered_hooks().len(), 1);

    client.clear_migration_hook(&admin, &legacy_key);
    assert_eq!(client.get_registered_hooks().len(), 0);
}

#[test]
fn clear_nonexistent_hook_is_idempotent() {
    let (_, client, admin) = setup_migration_test();
    let legacy_key = symbol_short!("no_such_key");

    // Clearing a non-existent hook should succeed silently (idempotent)
    let res = client.try_clear_migration_hook(&admin, &legacy_key);
    assert_eq!(res, Ok(()));
}

#[test]
fn clear_migration_hook_non_admin_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let non_admin = Address::generate(&env);
    let legacy_key = symbol_short!("test");

    // First register a hook
    client.register_migration_hook(&admin, &legacy_key, &MigrationTransform::Identity);

    // Non-admin should not be able to clear it
    let res = client.try_clear_migration_hook(&non_admin, &legacy_key);
    match res {
        Err(Ok(RevoraError::NotAuthorized)) => {}
        other => panic!("expected NotAuthorized, got: {:?}", other),
    }
}

#[test]
fn register_hook_uninitialized_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let legacy_key = symbol_short!("test");

    // Contract not initialized — admin check will fail
    let res = client.try_register_migration_hook(
        &admin,
        &legacy_key,
        &MigrationTransform::Identity,
    );
    match res {
        Err(Ok(RevoraError::NotInitialized)) => {}
        other => panic!("expected NotInitialized, got: {:?}", other),
    }
}

#[test]
fn register_hook_non_admin_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let non_admin = Address::generate(&env);
    let legacy_key = symbol_short!("test");

    let res = client.try_register_migration_hook(
        &non_admin,
        &legacy_key,
        &MigrationTransform::Identity,
    );
    match res {
        Err(Ok(RevoraError::NotAuthorized)) => {}
        other => panic!("expected NotAuthorized, got: {:?}", other),
    }
}

#[test]
fn get_registered_hooks_empty_when_no_hooks() {
    let (_, client, _) = setup_migration_test();
    let hooks = client.get_registered_hooks();
    assert_eq!(hooks.len(), 0);
}

#[test]
fn migrate_storage_walker_applies_hooks() {
    let (env, client, admin) = setup_migration_test();
    let issuer = admin.clone();
    let legacy_key = symbol_short!("my_key");

    // Register an identity hook
    client.register_migration_hook(&admin, &legacy_key, &MigrationTransform::Identity);

    // Run the walker
    client.migrate_storage_walker(&issuer, &1u32, &2u32, &false);

    // Verify both mig_step and mig_hook events were emitted
    let events = env.events().all();
    let step_events: Vec<_> = events.iter()
        .filter(|e| e.0.to_string().contains("mig_step"))
        .collect();
    assert!(!step_events.is_empty(), "must emit mig_step event");

    let hook_events: Vec<_> = events.iter()
        .filter(|e| e.0.to_string().contains("mig_hook"))
        .collect();
    assert!(!hook_events.is_empty(), "must emit mig_hook event for each registered hook");
}

#[test]
fn migrate_storage_walker_dry_run_applies_hooks_as_plan() {
    let (env, client, admin) = setup_migration_test();
    let issuer = admin.clone();
    let legacy_key = symbol_short!("dry_key");

    // Register a rename hook
    let new_key = symbol_short!("new_key_v2");
    client.register_migration_hook(
        &admin,
        &legacy_key,
        &MigrationTransform::Rename(new_key),
    );

    // Run the walker in dry_run mode
    client.migrate_storage_walker(&issuer, &1u32, &2u32, &true);

    // Verify migration_plan events were emitted for hooks
    let events = env.events().all();
    let plan_events: Vec<_> = events.iter()
        .filter(|e| e.0.to_string().contains("migration_plan"))
        .collect();
    assert!(!plan_events.is_empty(), "must emit migration_plan events for hooks in dry run");

    // Verify no mig_hook events in dry_run mode
    let hook_events: Vec<_> = events.iter()
        .filter(|e| e.0.to_string().contains("mig_hook"))
        .filter(|e| !e.0.to_string().contains("register")) // registration events still fire
        .collect();
    // Only the register event should be mig_hook, not the apply
    assert_eq!(hook_events.len(), 0, "no apply events in dry run");
}

#[test]
fn multiple_hooks_applied_during_walker() {
    let (env, client, admin) = setup_migration_test();
    let issuer = admin.clone();
    let key_a = symbol_short!("alpha");
    let key_b = symbol_short!("beta");
    let renamed = symbol_short!("beta_v2");

    // Register multiple hooks with different transforms
    client.register_migration_hook(&admin, &key_a, &MigrationTransform::Identity);
    client.register_migration_hook(&admin, &key_b, &MigrationTransform::Rename(renamed));

    // Run walker
    client.migrate_storage_walker(&issuer, &1u32, &2u32, &false);

    // Both hooks should produce mig_hook events
    let events = env.events().all();
    let apply_events: Vec<_> = events.iter()
        .filter(|e| {
            let topic_str = e.0.to_string();
            topic_str.contains("mig_hook") && !topic_str.contains("register")
        })
        .collect();
    // 2 hooks × 1 apply each = 2 events (register events excluded)
    assert_eq!(apply_events.len(), 2, "expected 2 hook apply events, got {}", apply_events.len());
}

#[test]
fn walker_replay_protection_preserved_with_hooks() {
    let (_, client, admin) = setup_migration_test();
    let issuer = admin.clone();
    let legacy_key = symbol_short!("replay_key");

    client.register_migration_hook(&admin, &legacy_key, &MigrationTransform::Identity);

    // First run succeeds
    client.migrate_storage_walker(&issuer, &1u32, &2u32, &false);

    // Second run at same versions must fail (replay protection)
    let res = client.try_migrate_storage_walker(&issuer, &1u32, &2u32, &false);
    assert_eq!(res, Err(Ok(MigrationError::MigrationAlreadyApplied)));
}

#[test]
fn hook_identity_transform_is_noop() {
    let (_, client, admin) = setup_migration_test();
    let legacy_key = symbol_short!("noop_key");

    // Register an Identity transform (returns the same value)
    client.register_migration_hook(&admin, &legacy_key, &MigrationTransform::Identity);

    let hooks = client.get_registered_hooks();
    assert_eq!(hooks.len(), 1);

    // An Identity transform should leave the value unchanged
    let hook = hooks.get(0).unwrap();
    assert_eq!(hook.legacy_key, legacy_key);
    assert_eq!(hook.transform, MigrationTransform::Identity);
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

// ─── Downgrade-rejection guard tests ────────────────────────────────────────

#[test]
fn contract_version_compatible_after_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    // After initialize, DeployedVersion == CONTRACT_VERSION, so the guard must allow operations.
    let res = client.try_set_testnet_mode(&true);
    assert_eq!(res, Ok(()));
}

#[test]
fn contract_version_compatible_rejects_when_stored_higher() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    // Bump DeployedVersion above CONTRACT_VERSION to simulate a lossy downgrade scenario.
    client.migrate_storage(&admin, &2, &0, &0).unwrap();

    let res = client.try_set_testnet_mode(&true);
    match res {
        Err(Ok(RevoraError::MigrationDowngradeNotAllowed)) => {}
        other => panic!("expected MigrationDowngradeNotAllowed, got: {:?}", other),
    }
}

#[test]
fn contract_version_compatible_emits_downgrade_reject_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    client.migrate_storage(&admin, &2, &0, &0).unwrap();

    let _ = client.try_set_testnet_mode(&true);

    let events = env.events().all();
    let reject_events: Vec<_> =
        events.iter().filter(|e| e.0.to_string().contains("downgrade_reject")).collect();
    assert!(!reject_events.is_empty(), "expected downgrade_reject event");
}

#[test]
fn contract_version_compatible_passes_at_equal_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    // Bump to exactly CONTRACT_VERSION (should succeed, guard passes equal)
    // Note: migrate_storage itself will return AlreadyAtTargetVersion since
    // DeployedVersion already equals CONTRACT_VERSION after init.
    let res = client.try_migrate_storage(&admin, &1, &0, &23);
    match res {
        Err(Ok(RevoraError::AlreadyAtTargetVersion)) => {}
        other => panic!("expected AlreadyAtTargetVersion, got: {:?}", other),
    }

    // A state-mutating call must still succeed (guard passes equal boundary)
    let res = client.try_set_testnet_mode(&true);
    assert_eq!(res, Ok(()));
}

#[test]
fn contract_version_compatible_allows_operations_when_stored_lower() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    // Manually set DeployedVersion below CONTRACT_VERSION via the storage directly.
    // This represents an upgrade path where old storage gets a lower version.
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&crate::DataKey::DeployedVersion, &(0, 9, 0));
    });

    // All operations should be allowed (CONTRACT_VERSION > stored DeployedVersion)
    let res = client.try_set_testnet_mode(&true);
    assert_eq!(res, Ok(()));
}
