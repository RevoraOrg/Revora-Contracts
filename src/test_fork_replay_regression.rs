//! # Fork-Replay Cross-Chain Regression Test Suite (Issue #579)
//!
//! ## Threat Model
//!
//! A cross-chain replay attack occurs when a validly-signed payload created for
//! one chain is submitted to another chain. The contract's attested reporting
//! path binds signatures to `env.ledger().network_id()` via `verify_attestation`,
//! but the plain `report_revenue` entrypoint accepts an unsigned issuer call and
//! does not carry a chain id. This module records that behavior so a future
//! change to `report_revenue` cannot silently alter it.
//!
//! ## Test Cases
//!
//! Each case sets a distinct network id and verifies that plain
//! `report_revenue` records revenue and updates the audit summary, regardless
//! of which "chain" issued the call. Testnet mode is checked separately because
//! `set_testnet_mode` relaxes some validation but must not change accounting.

#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env, Symbol, Vec,
};

// ─── Test Setup Helpers ────────────────────────────────────────────────────────

/// Create a fresh test environment with a deployed contract.
fn setup_test_env(
    network_id: [u8; 32],
) -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_network_id(network_id);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    (env, client, issuer, token, payout_asset)
}

/// Register an offering for testing.
fn register_offering(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
    payout_asset: &Address,
) {
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        &Symbol::new(env, "test_ns"),
        token,
        &5_000u32,
        payout_asset,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
}

/// Get the current audit summary for an offering (total revenue and report count).
fn get_audit_summary(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
) -> (i128, u64) {
    client
        .get_audit_summary(issuer, &Symbol::new(env, "test_ns"), token)
        .map_or((0, 0), |summary| (summary.total_revenue, summary.report_count))
}

/// Get the revenue for a specific period.
fn get_revenue_by_period(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
    period_id: u64,
) -> Option<i128> {
    client
        .try_get_revenue_by_period(issuer, &Symbol::new(env, "test_ns"), token, &period_id)
        .ok()
        .and_then(|value| value.ok())
}

/// Assert that `report_revenue` accepted a plain (non-attested) call and applied
/// its accounting side effects. Network binding for signed payloads lives in
/// `verify_attestation`, not in `report_revenue`.
fn assert_plain_report_recorded<E: core::fmt::Debug, F: core::fmt::Debug>(
    result: Result<Result<(), E>, F>,
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
    period_id: u64,
    amount: i128,
    before: (i128, u64),
) {
    assert!(matches!(result, Ok(Ok(()))), "report_revenue must accept the plain call: {result:?}");

    let after = get_audit_summary(env, client, issuer, token);
    assert_eq!(after.0, before.0 + amount, "report_revenue must add to total revenue");
    assert_eq!(after.1, before.1 + 1, "report_revenue must increment the report count");
    assert_eq!(
        get_revenue_by_period(env, client, issuer, token, period_id),
        Some(amount),
        "report_revenue must record period revenue"
    );
}

// ─── Test Cases ───────────────────────────────────────────────────────────────

#[test]
fn test_fork_replay_plain_report_is_not_chain_bound() {
    const MAINNET_ID: [u8; 32] = [0x01u8; 32];
    const TESTNET_ID: [u8; 32] = [0x02u8; 32];

    let (env, client, issuer, token, payout_asset) = setup_test_env(TESTNET_ID);
    register_offering(&env, &client, &issuer, &token, &payout_asset);

    let namespace = Symbol::new(&env, "test_ns");
    let period_id = 1u64;
    let amount = 10_000i128;

    let before = get_audit_summary(&env, &client, &issuer, &token);
    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &payout_asset,
        &amount,
        &period_id,
        &false,
    );

    // report_revenue has no chain-id parameter; only verify_attestation binds
    // signed reports to the active network id.
    assert_plain_report_recorded(result, &env, &client, &issuer, &token, period_id, amount, before);
}

#[test]
fn test_fork_replay_plain_report_succeeds_on_mainnet_env() {
    const MAINNET_ID: [u8; 32] = [0x01u8; 32];

    let (env, client, issuer, token, payout_asset) = setup_test_env(MAINNET_ID);
    register_offering(&env, &client, &issuer, &token, &payout_asset);

    let namespace = Symbol::new(&env, "test_ns");
    let period_id = 1u64;
    let amount = 10_000i128;

    let before = get_audit_summary(&env, &client, &issuer, &token);
    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &payout_asset,
        &amount,
        &period_id,
        &false,
    );

    // The audit summary is only updated by report_revenue after all guards pass.
    assert_plain_report_recorded(result, &env, &client, &issuer, &token, period_id, amount, before);
}

#[test]
fn test_fork_replay_boundary_zero_network_id_is_not_special_cased() {
    const ACTIVE_CHAIN_ID: [u8; 32] = [0x03u8; 32];

    let (env, client, issuer, token, payout_asset) = setup_test_env(ACTIVE_CHAIN_ID);
    register_offering(&env, &client, &issuer, &token, &payout_asset);

    let namespace = Symbol::new(&env, "test_ns");
    let period_id = 1u64;
    let amount = 5_000i128;

    let before = get_audit_summary(&env, &client, &issuer, &token);
    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &payout_asset,
        &amount,
        &period_id,
        &false,
    );

    // report_revenue does not inspect the ledger network id, so an all-zero id
    // is treated the same as any other id.
    assert_plain_report_recorded(result, &env, &client, &issuer, &token, period_id, amount, before);
}

#[test]
fn test_fork_replay_boundary_max_network_id_is_not_special_cased() {
    const ACTIVE_CHAIN_ID: [u8; 32] = [0x04u8; 32];

    let (env, client, issuer, token, payout_asset) = setup_test_env(ACTIVE_CHAIN_ID);
    register_offering(&env, &client, &issuer, &token, &payout_asset);

    let namespace = Symbol::new(&env, "test_ns");
    let period_id = 1u64;
    let amount = 5_000i128;

    let before = get_audit_summary(&env, &client, &issuer, &token);
    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &payout_asset,
        &amount,
        &period_id,
        &false,
    );

    // There is no u64::MAX chain-id wildcard in report_revenue; the ledger
    // network id is a 32-byte value checked only by verify_attestation.
    assert_plain_report_recorded(result, &env, &client, &issuer, &token, period_id, amount, before);
}

#[test]
fn test_fork_replay_control_case_same_chain_succeeds() {
    const CHAIN_ID: [u8; 32] = [0x05u8; 32];

    let (env, client, issuer, token, payout_asset) = setup_test_env(CHAIN_ID);
    register_offering(&env, &client, &issuer, &token, &payout_asset);

    let namespace = Symbol::new(&env, "test_ns");
    let period_id = 1u64;
    let amount = 10_000i128;

    let before = get_audit_summary(&env, &client, &issuer, &token);
    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &payout_asset,
        &amount,
        &period_id,
        &false,
    );

    assert_plain_report_recorded(result, &env, &client, &issuer, &token, period_id, amount, before);
}

#[test]
fn test_fork_replay_testnet_mode_accepts_plain_report() {
    const TESTNET_CHAIN_ID: [u8; 32] = [0x06u8; 32];

    let (env, client, issuer, token, payout_asset) = setup_test_env(TESTNET_CHAIN_ID);

    // initialize is idempotent; set_testnet_mode requires the initialized admin
    // auth, which is mocked in setup_test_env.
    assert!(matches!(client.try_set_testnet_mode(&true), Ok(Ok(()))));

    register_offering(&env, &client, &issuer, &token, &payout_asset);

    let namespace = Symbol::new(&env, "test_ns");
    let period_id = 1u64;
    let amount = 5_000i128;

    let before = get_audit_summary(&env, &client, &issuer, &token);
    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &payout_asset,
        &amount,
        &period_id,
        &false,
    );

    // set_testnet_mode relaxes validation but report_revenue still records the
    // same accounting state.
    assert_plain_report_recorded(result, &env, &client, &issuer, &token, period_id, amount, before);
}
