//! # Adversarial tests for `get_platform_fee_per_asset` (#1047)
//!
//! The getter is a pure view method: a single persistent read of
//! `DataKey::PlatformFeePerAsset(asset)` that returns `0` when the key is absent.
//! It intentionally performs no authorization, so **any** caller can read the
//! configured per-asset fee. The sibling `set_platform_fee_per_asset` is
//! admin-gated and rejects values above `MAX_PLATFORM_FEE_BPS` (5 000).
//!
//! These tests exercise:
//! - valid reads (configured fee round-trips, default `0` for unset assets);
//! - boundary values for `env` (uninitialized contract) and `asset` (unknown,
//!   multiple, and reset-to-zero assets);
//! - state unchanged after rejected operations (over-max reject, uninitialized
//!   reject) — a failed set must neither write storage nor emit `EVENT_FEE_CONFIG`.

#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _},
    Address, Env,
};

const MAX_PLATFORM_FEE_BPS_VAL: u32 = 5_000;

fn setup(env: &Env) -> (RevoraRevenueShareClient<'static>, Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let asset = Address::generate(env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    (client, admin, asset)
}

#[test]
fn get_platform_fee_per_asset_defaults_to_zero_for_unknown_asset() {
    let env = Env::default();
    let (client, _admin, asset) = setup(&env);
    assert_eq!(client.get_platform_fee_per_asset(&asset), 0);
}

#[test]
fn get_platform_fee_per_asset_returns_configured_fee() {
    let env = Env::default();
    let (client, _admin, asset) = setup(&env);
    client.set_platform_fee_per_asset(&asset, &400);
    assert_eq!(client.get_platform_fee_per_asset(&asset), 400);
}

#[test]
fn get_platform_fee_per_asset_is_a_public_read_without_auth() {
    // The getter performs no `require_auth`; a caller that has never been
    // authorized must still be able to read the configured fee.
    let env = Env::default();
    let (client, _admin, asset) = setup(&env);
    client.set_platform_fee_per_asset(&asset, &250);
    assert_eq!(client.get_platform_fee_per_asset(&asset), 250);
}

#[test]
fn get_platform_fee_per_asset_reads_before_initialize() {
    // Adversarial `env` boundary: before `initialize` the admin key is absent,
    // but the getter must still return `0` rather than panic.
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let asset = Address::generate(&env);
    assert_eq!(client.get_platform_fee_per_asset(&asset), 0);
}

#[test]
fn get_platform_fee_per_asset_max_boundary_round_trips() {
    let env = Env::default();
    let (client, _admin, asset) = setup(&env);
    // Upper boundary is accepted.
    client.set_platform_fee_per_asset(&asset, &MAX_PLATFORM_FEE_BPS_VAL);
    assert_eq!(client.get_platform_fee_per_asset(&asset), MAX_PLATFORM_FEE_BPS_VAL);
    // Reset to zero removes the override.
    client.set_platform_fee_per_asset(&asset, &0);
    assert_eq!(client.get_platform_fee_per_asset(&asset), 0);
}

#[test]
fn get_platform_fee_per_asset_independent_across_assets() {
    let env = Env::default();
    let (client, _admin, asset_a) = setup(&env);
    let asset_b = Address::generate(&env);
    let asset_c = Address::generate(&env);
    client.set_platform_fee_per_asset(&asset_a, &100);
    client.set_platform_fee_per_asset(&asset_b, &200);
    // Configuring one asset must not leak into the others.
    assert_eq!(client.get_platform_fee_per_asset(&asset_a), 100);
    assert_eq!(client.get_platform_fee_per_asset(&asset_b), 200);
    // An asset that was never configured still reads 0.
    assert_eq!(client.get_platform_fee_per_asset(&asset_c), 0);
}

#[test]
fn over_max_set_is_rejected_and_state_is_unchanged() {
    let env = Env::default();
    let (client, _admin, asset) = setup(&env);
    client.set_platform_fee_per_asset(&asset, &400);
    let events_before = env.events().all().len();

    let result = client.try_set_platform_fee_per_asset(&asset, &(MAX_PLATFORM_FEE_BPS_VAL + 1));
    assert_eq!(result, Err(Ok(RevoraError::InvalidRevenueShareBps)));

    // The rejected set must not overwrite the stored fee or emit a config event.
    assert_eq!(client.get_platform_fee_per_asset(&asset), 400);
    assert_eq!(
        env.events().all().len(),
        events_before,
        "rejected set must not emit EVENT_FEE_CONFIG"
    );
}

#[test]
fn set_before_initialize_is_rejected_and_state_is_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let asset = Address::generate(&env);

    let result = client.try_set_platform_fee_per_asset(&asset, &300);
    assert_eq!(result, Err(Ok(RevoraError::NotInitialized)));

    // No admin and no fee state may be written by the rejected call.
    assert_eq!(client.get_platform_fee_per_asset(&asset), 0);
}

#[test]
#[ignore = "not-admin check uses non-unwinding require_auth panic; cannot be caught by try_ in no_std (see src/test_auth.rs)"]
fn unauthorized_caller_cannot_change_fee() {
    // `set_platform_fee_per_asset` calls `admin.require_auth()` as its only
    // access-control layer. In the no_std/WASM runtime that host panic is
    // non-unwinding, so `try_*` cannot capture it as `Result::Err`; this test is
    // ignored for the same reason documented in `src/test_auth.rs`.
    let env = Env::default();
    // NOTE: no `mock_all_auths` — auth must be genuinely enforced.
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let result = client.try_set_platform_fee_per_asset(&asset, &300);
    assert!(result.is_err());
    assert_eq!(client.get_platform_fee_per_asset(&asset), 0);
}
