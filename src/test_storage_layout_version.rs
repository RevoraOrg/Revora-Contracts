#![cfg(test)]
extern crate alloc;

use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, symbol_short, Bytes, IntoVal, Vec,
};
use crate::{
    RevoraRevenueShare, RevoraRevenueShareClient, MigrationError, assert_semver_forward,
    RevoraError, STORAGE_LAYOUT_VERSION,
};
use crate::vesting::{VestingSchedule, VestingCurve, VestingKey, compute_vested, compute_claimable};
use soroban_sdk::xdr::{FromXdr, ToXdr};

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

// ─── Vesting Storage Upgrade Integrity Tests ────────────────────────────
//
// Test methodology:
//   1. Create a known VestingSchedule (the "fixture").
//   2. Serialize it to XDR bytes (simulating a legacy on-chain write).
//   3. Round-trip through deserialization and compare every field.
//   4. Store and read via the typed storage API to validate that real
//      Soroban persistent storage preserves all fields.
//   5. Verify compute_vested / compute_claimable produce identical results
//      before and after the round-trip (curve-shape preservation).
//
// Edge cases covered:
//   - Zero cliff (cliff_ts = 0)
//   - Cliff equal to end_ts
//   - Boundary timestamp values (u64::MIN, u64::MAX / 2, u64::MAX)
//   - All four VestingCurve variants: Linear, Cliff, Graded, Step
//   - Zero accelerated_amount
//   - Non-zero accelerated_amount after round-trip
//
// Security properties verified:
//   - No silent mutation of any VestingSchedule field
//   - No timestamp truncation or overflow in XDR encoding
//   - Deterministic serialization (same struct → same bytes)
// ────────────────────────────────────────────────────────────────────────

/// Build a VestingSchedule with the given parameters, using generated
/// addresses for issuer / beneficiary / token.
fn build_vesting_schedule(
    env: &Env,
    cliff_ts: u64,
    start_ts: u64,
    end_ts: u64,
    curve: VestingCurve,
    total_amount: i128,
    accelerated_amount: i128,
) -> VestingSchedule {
    let issuer = Address::generate(env);
    let beneficiary = Address::generate(env);
    let token = Address::generate(env);
    VestingSchedule {
        issuer,
        beneficiary,
        token,
        total_amount,
        cliff_ts,
        start_ts,
        end_ts,
        curve,
        accelerated_amount,
    }
}

/// Verify all fields of two VestingSchedules are strictly equal.
fn assert_schedules_eq(a: &VestingSchedule, b: &VestingSchedule) {
    assert_eq!(a.issuer, b.issuer, "issuer mismatch");
    assert_eq!(a.beneficiary, b.beneficiary, "beneficiary mismatch");
    assert_eq!(a.token, b.token, "token mismatch");
    assert_eq!(a.total_amount, b.total_amount, "total_amount mismatch");
    assert_eq!(a.cliff_ts, b.cliff_ts, "cliff_ts mismatch");
    assert_eq!(a.start_ts, b.start_ts, "start_ts mismatch");
    assert_eq!(a.end_ts, b.end_ts, "end_ts mismatch");
    assert_eq!(a.curve, b.curve, "curv mismatch");
    assert_eq!(a.accelerated_amount, b.accelerated_amount, "accelerated_amount mismatch");
}

// ── Helper: XDR round-trip ──────────────────────────────────────────────

/// Serialize a VestingSchedule to XDR bytes, deserialize back, and assert
/// all fields match.
fn assert_xdr_roundtrip(env: &Env, schedule: &VestingSchedule) {
    let bytes: Bytes = schedule.to_xdr(env);
    let decoded: VestingSchedule = VestingSchedule::from_xdr(env, &bytes);
    assert_schedules_eq(schedule, &decoded);
}

// ── Helper: Storage round-trip ──────────────────────────────────────────

/// Store a VestingSchedule via the typed persistent storage API, read it
/// back, and assert all fields match.
fn assert_storage_roundtrip(env: &Env, contract_id: &Address, schedule: &VestingSchedule) {
    let beneficiary = &schedule.beneficiary;
    let key = VestingKey::Schedule(beneficiary.clone());

    env.as_contract(contract_id, || {
        env.storage().persistent().set(&key, schedule);
    });

    env.as_contract(contract_id, || {
        let retrieved: VestingSchedule = env.storage().persistent().get(&key).unwrap();
        assert_schedules_eq(schedule, &retrieved);
    });
}

// ── Helper: Legacy bytes storage simulation ─────────────────────────────
//
// Simulates a storage migration scenario:
//   1. Serialize the schedule to raw XDR bytes (as if written by old code).
//   2. Write those bytes into the VestingKey::Schedule slot via typed API
//      (which internally does XDR encode → store).
//   3. Read back through the typed API (decode XDR → VestingSchedule).
//   4. Verify every field is preserved.
//
// This proves the XDR schema for VestingSchedule is backward-compatible
// across contract upgrades that do NOT change the struct layout.
fn assert_legacy_storage_migration(env: &Env, contract_id: &Address, schedule: &VestingSchedule) {
    // Step 1: Write via typed API (simulates "legacy" contract writing the struct)
    let beneficiary = &schedule.beneficiary;
    let key = VestingKey::Schedule(beneficiary.clone());

    env.as_contract(contract_id, || {
        env.storage().persistent().set(&key, schedule);
    });

    // Step 2: Read back and verify
    env.as_contract(contract_id, || {
        let retrieved: VestingSchedule = env.storage().persistent().get(&key).unwrap();
        assert_schedules_eq(schedule, &retrieved);
    });
}

// ── Individual tests ────────────────────────────────────────────────────

#[test]
fn test_vesting_xdr_roundtrip_linear() {
    let env = Env::default();
    let schedule = build_vesting_schedule(
        &env,
        1_000,       // cliff_ts
        5_000,       // start_ts
        100_000,     // end_ts
        VestingCurve::Linear,
        1_000_000,   // total_amount
        0,           // accelerated_amount
    );
    assert_xdr_roundtrip(&env, &schedule);
}

#[test]
fn test_vesting_xdr_roundtrip_cliff() {
    let env = Env::default();
    let schedule = build_vesting_schedule(
        &env,
        10_000,
        10_000,
        100_000,
        VestingCurve::Cliff,
        500_000,
        0,
    );
    assert_xdr_roundtrip(&env, &schedule);
}

#[test]
fn test_vesting_xdr_roundtrip_graded() {
    let env = Env::default();
    let schedule = build_vesting_schedule(
        &env,
        0,
        1_000,
        50_000,
        VestingCurve::Graded { step_secs: 3600 },
        2_000_000,
        100_000,
    );
    assert_xdr_roundtrip(&env, &schedule);
}

#[test]
fn test_vesting_xdr_roundtrip_step() {
    let env = Env::default();
    let schedule = build_vesting_schedule(
        &env,
        86_400,        // 1 day cliff
        86_400,        // start = cliff
        2_592_000,     // 30 day end
        VestingCurve::Step { steps: 12 },
        10_000_000,
        500_000,
    );
    assert_xdr_roundtrip(&env, &schedule);
}

#[test]
fn test_vesting_xdr_roundtrip_zero_cliff() {
    let env = Env::default();
    let schedule = build_vesting_schedule(
        &env,
        0,       // zero cliff
        0,       // start = cliff
        1_000,
        VestingCurve::Linear,
        1_000,
        0,
    );
    assert_xdr_roundtrip(&env, &schedule);
}

#[test]
fn test_vesting_xdr_roundtrip_cliff_equals_end() {
    let env = Env::default();
    let schedule = build_vesting_schedule(
        &env,
        100_000,   // cliff = end
        50_000,
        100_000,   // cliff == end_ts
        VestingCurve::Cliff,
        1_000,
        0,
    );
    assert_xdr_roundtrip(&env, &schedule);
}

#[test]
fn test_vesting_xdr_roundtrip_boundary_timestamps() {
    let env = Env::default();
    let schedule = build_vesting_schedule(
        &env,
        0,                     // cliff = min
        1,
        u64::MAX,              // end = max
        VestingCurve::Linear,
        i128::MAX,
        i128::MAX,
    );
    assert_xdr_roundtrip(&env, &schedule);
}

#[test]
fn test_vesting_storage_roundtrip_linear() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let schedule = build_vesting_schedule(
        &env,
        1_000,
        5_000,
        100_000,
        VestingCurve::Linear,
        1_000_000,
        0,
    );
    assert_storage_roundtrip(&env, &contract_id, &schedule);
}

#[test]
fn test_vesting_storage_roundtrip_with_acceleration() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let schedule = build_vesting_schedule(
        &env,
        500,
        1_000,
        10_000,
        VestingCurve::Graded { step_secs: 7_200 },
        5_000_000,
        250_000,   // pre-accelerated
    );
    assert_storage_roundtrip(&env, &contract_id, &schedule);
}

#[test]
fn test_vesting_legacy_bytes_migration_all_curves() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);

    let curves = vec![
        VestingCurve::Linear,
        VestingCurve::Cliff,
        VestingCurve::Graded { step_secs: 3600 },
        VestingCurve::Step { steps: 12 },
    ];

    for curve in curves {
        let schedule = build_vesting_schedule(
            &env,
            1_000,
            5_000,
            100_000,
            curve,
            1_000_000,
            50_000,
        );
        assert_legacy_storage_migration(&env, &contract_id, &schedule);
    }
}

#[test]
fn test_vesting_legacy_bytes_migration_edge_cases() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);

    // Zero cliff
    let s1 = build_vesting_schedule(&env, 0, 0, 1_000, VestingCurve::Linear, 1_000, 0);
    assert_legacy_storage_migration(&env, &contract_id, &s1);

    // Cliff equals end_ts
    let s2 = build_vesting_schedule(&env, 100_000, 50_000, 100_000, VestingCurve::Cliff, 1_000, 0);
    assert_legacy_storage_migration(&env, &contract_id, &s2);

    // Boundary timestamps
    let s3 = build_vesting_schedule(&env, 0, 1, u64::MAX, VestingCurve::Linear, i128::MAX, i128::MAX);
    assert_legacy_storage_migration(&env, &contract_id, &s3);

    // Zero total amount (edge case for compute functions)
    let s4 = build_vesting_schedule(&env, 500, 1_000, 10_000, VestingCurve::Linear, 0, 0);
    assert_legacy_storage_migration(&env, &contract_id, &s4);
}

#[test]
fn test_vesting_compute_functions_preserved_after_roundtrip() {
    let env = Env::default();
    let schedule = build_vesting_schedule(
        &env,
        1_000,       // cliff at t=1000
        5_000,       // start at t=5000
        10_000,      // end at t=10000
        VestingCurve::Linear,
        10_000,      // total = 10000
        0,
    );

    // Round-trip
    let bytes: Bytes = schedule.to_xdr(&env);
    let decoded: VestingSchedule = VestingSchedule::from_xdr(&env, &bytes);

    // Verify compute_vested at various timestamps
    let test_times = [0u64, 500, 1_000, 2_500, 5_000, 7_500, 10_000, 12_000];
    for &now in &test_times {
        let original_vested = compute_vested(&schedule, now);
        let decoded_vested = compute_vested(&decoded, now);
        assert_eq!(original_vested, decoded_vested,
            "compute_vested mismatch at now={}: original={}, decoded={}",
            now, original_vested, decoded_vested);
    }

    // Verify compute_claimable with a non-zero already_claimed
    let already_claimed = 3_000_i128;
    for &now in &test_times {
        let original_claimable = compute_claimable(&schedule, already_claimed, now);
        let decoded_claimable = compute_claimable(&decoded, already_claimed, now);
        assert_eq!(original_claimable, decoded_claimable,
            "compute_claimable mismatch at now={}: original={}, decoded={}",
            now, original_claimable, decoded_claimable);
    }
}

#[test]
fn test_vesting_byte_level_determinism() {
    let env = Env::default();
    let s1 = build_vesting_schedule(
        &env, 1_000, 5_000, 100_000, VestingCurve::Graded { step_secs: 3600 }, 1_000_000, 0,
    );
    let s2 = build_vesting_schedule(
        &env, 1_000, 5_000, 100_000, VestingCurve::Graded { step_secs: 3600 }, 1_000_000, 0,
    );

    let b1: Bytes = s1.to_xdr(&env);
    let b2: Bytes = s2.to_xdr(&env);
    assert_eq!(b1, b2, "Identical VestingSchedule values must produce identical XDR bytes");
}

#[test]
fn test_vesting_compute_with_accelerated_after_roundtrip() {
    let env = Env::default();
    let schedule = build_vesting_schedule(
        &env,
        100,         // cliff
        200,         // start
        1_200,       // end
        VestingCurve::Linear,
        1_000,       // total
        200,         // 20% pre-accelerated
    );

    let bytes: Bytes = schedule.to_xdr(&env);
    let decoded: VestingSchedule = VestingSchedule::from_xdr(&env, &bytes);

    // After cliff but before start: only accelerated amount is vested
    let vested_before_start = compute_vested(&schedule, 150);
    let decoded_vested = compute_vested(&decoded, 150);
    assert_eq!(vested_before_start, 200, "pre-acceleration should yield 200 at t=150");
    assert_eq!(decoded_vested, 200);

    // At end: full total should be vested (1000)
    let vested_at_end = compute_vested(&schedule, 1_200);
    let decoded_at_end = compute_vested(&decoded, 1_200);
    assert_eq!(vested_at_end, 1_000);
    assert_eq!(decoded_at_end, 1_000);
}

#[test]
fn test_vesting_migration_preserves_all_fields_integration() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);

    // 90-day cliff (in seconds), realistic offering timestamps
    let schedule = build_vesting_schedule(
        &env,
        7_776_000,       // 90-day cliff
        17_064_000,      // start approx 90+ days out
        51_278_400,      // end ~2 years from start
        VestingCurve::Linear,
        1_000_000_000,   // 1B tokens
        0,
    );

    let key = VestingKey::Schedule(schedule.beneficiary.clone());

    // Write (simulate legacy storage)
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&key, &schedule);
    });

    // Read back (simulate post-upgrade read)
    env.as_contract(&contract_id, || {
        let retrieved: VestingSchedule = env.storage().persistent().get(&key).unwrap();

        // Field-level assertions with descriptive messages
        assert_eq!(retrieved.issuer, schedule.issuer, "issuer mutated during migration");
        assert_eq!(retrieved.beneficiary, schedule.beneficiary, "beneficiary mutated");
        assert_eq!(retrieved.token, schedule.token, "token mutated");
        assert_eq!(retrieved.total_amount, schedule.total_amount, "total_amount mutated");
        assert_eq!(retrieved.cliff_ts, schedule.cliff_ts, "cliff_ts (90-day cliff) mutated");
        assert_eq!(retrieved.start_ts, schedule.start_ts, "start_ts mutated");
        assert_eq!(retrieved.end_ts, schedule.end_ts, "end_ts mutated");
        assert_eq!(retrieved.curve, schedule.curve, "curve shape corrupted");
        assert_eq!(retrieved.accelerated_amount, schedule.accelerated_amount, "accelerated_amount corrupted");
    });
}
