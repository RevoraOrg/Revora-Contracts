//! Adversarial coverage for `migrate_storage` (revora `src/lib.rs`, #1164).
//!
//! `migrate_storage` is the privileged semver ratchet that moves the persisted
//! `DeployedVersion` forward. Its reject paths must be deterministic *and*
//! side-effect free: a rejected call must not move `DeployedVersion` and must
//! not emit a `migrate` event. This suite pins those guarantees, complementing
//! the happy-path/integration cases in `test_storage_layout_version.rs`.
//!
//! Covered classes:
//! * valid      - a strict forward upgrade persists the target and emits one event.
//! * invalid    - no-op (`AlreadyAtTargetVersion`) and downgrade
//!                (`MigrationDowngradeNotAllowed`) are rejected without side effects.
//! * boundary   - `(0, 0, 0)` is a downgrade; `(u32::MAX, u32::MAX, u32::MAX)` is accepted.
//! * permission - uninitialized -> `NotInitialized`, non-admin -> `NotAuthorized`,
//!                frozen -> `ContractFrozen`, on-chain layout ahead -> `MigrationDowngradeNotAllowed`.

#![cfg(test)]

use crate::{
    DataKey, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, STORAGE_LAYOUT_VERSION,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events},
    Address, Env, IntoVal, Symbol,
};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    (env, client, admin, contract_id)
}

/// Read the persisted `DeployedVersion` floor directly from storage.
fn deployed_version(env: &Env, contract_id: &Address) -> (u32, u32, u32) {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, (u32, u32, u32)>(&DataKey::DeployedVersion)
            .unwrap()
    })
}

/// Count `migrate` events emitted so far.
fn migrate_event_count(env: &Env) -> usize {
    let events = env.events().all();
    let mut count = 0usize;
    for event in events.iter() {
        let topic0: Symbol = event.1.get(0).unwrap().into_val(env);
        if topic0 == symbol_short!("migrate") {
            count += 1;
        }
    }
    count
}

#[test]
fn accepts_patch_bump_and_emits_exactly_one_event() {
    let (env, client, admin, contract_id) = setup();

    // CONTRACT_VERSION is (1, 0, 23); a patch bump is a strict forward upgrade.
    let res = client.try_migrate_storage(&admin, &1, &0, &24);
    assert_eq!(res, Ok(Ok(())));
    assert_eq!(deployed_version(&env, &contract_id), (1, 0, 24));
    assert_eq!(migrate_event_count(&env), 1);
}

#[test]
fn accepts_u32_max_boundary_target() {
    let (env, client, admin, contract_id) = setup();

    let res = client.try_migrate_storage(&admin, &u32::MAX, &u32::MAX, &u32::MAX);
    assert_eq!(res, Ok(Ok(())));
    assert_eq!(
        deployed_version(&env, &contract_id),
        (u32::MAX, u32::MAX, u32::MAX)
    );
}

#[test]
fn rejects_zero_target_as_downgrade_without_side_effects() {
    let (env, client, admin, contract_id) = setup();
    let before = deployed_version(&env, &contract_id);

    let res = client.try_migrate_storage(&admin, &0, &0, &0);
    assert_eq!(res, Err(Ok(RevoraError::MigrationDowngradeNotAllowed)));
    // Rejected migration moves nothing and emits no `migrate` event.
    assert_eq!(deployed_version(&env, &contract_id), before);
    assert_eq!(migrate_event_count(&env), 0);
}

#[test]
fn rejects_equal_target_as_noop_without_side_effects() {
    let (env, client, admin, contract_id) = setup();
    let before = deployed_version(&env, &contract_id);

    let res = client.try_migrate_storage(&admin, &1, &0, &23);
    assert_eq!(res, Err(Ok(RevoraError::AlreadyAtTargetVersion)));
    assert_eq!(deployed_version(&env, &contract_id), before);
    assert_eq!(migrate_event_count(&env), 0);
}

#[test]
fn rejects_patch_downgrade_without_side_effects() {
    let (env, client, admin, contract_id) = setup();
    let before = deployed_version(&env, &contract_id);

    let res = client.try_migrate_storage(&admin, &1, &0, &22);
    assert_eq!(res, Err(Ok(RevoraError::MigrationDowngradeNotAllowed)));
    assert_eq!(deployed_version(&env, &contract_id), before);
    assert_eq!(migrate_event_count(&env), 0);
}

#[test]
fn rejects_uninitialized_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let res = client.try_migrate_storage(&admin, &2, &0, &0);
    assert_eq!(res, Err(Ok(RevoraError::NotInitialized)));
}

#[test]
fn rejects_non_admin_without_side_effects() {
    let (env, client, _admin, contract_id) = setup();
    let before = deployed_version(&env, &contract_id);
    let stranger = Address::generate(&env);

    let res = client.try_migrate_storage(&stranger, &2, &0, &0);
    assert_eq!(res, Err(Ok(RevoraError::NotAuthorized)));
    assert_eq!(deployed_version(&env, &contract_id), before);
    assert_eq!(migrate_event_count(&env), 0);
}

#[test]
fn rejects_when_contract_is_frozen() {
    let (env, client, admin, contract_id) = setup();
    client.freeze();
    let before = deployed_version(&env, &contract_id);

    let res = client.try_migrate_storage(&admin, &2, &0, &0);
    assert_eq!(res, Err(Ok(RevoraError::ContractFrozen)));
    assert_eq!(deployed_version(&env, &contract_id), before);
    assert_eq!(migrate_event_count(&env), 0);
}

#[test]
fn rejects_when_on_chain_layout_is_ahead() {
    let (env, client, admin, contract_id) = setup();
    // Simulate a storage layout written by a newer binary.
    client.set_storage_layout_version(&admin, &(STORAGE_LAYOUT_VERSION + 1));
    let before = deployed_version(&env, &contract_id);

    let res = client.try_migrate_storage(&admin, &2, &0, &0);
    assert_eq!(res, Err(Ok(RevoraError::MigrationDowngradeNotAllowed)));
    assert_eq!(deployed_version(&env, &contract_id), before);
    assert_eq!(migrate_event_count(&env), 0);
}
