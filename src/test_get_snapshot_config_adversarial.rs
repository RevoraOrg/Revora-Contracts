//! Adversarial coverage for [`RevoraRevenueShare::get_snapshot_config`].
//!
//! `get_snapshot_config` is a read-only flag that gates the whole snapshot
//! workflow.  These tests pin the behaviour that callers key off:
//!
//! * a never-configured offering reports `false`;
//! * enabling and disabling the feature is reflected exactly;
//! * the flag is scoped per offering;
//! * an unknown offering reads as `false` instead of erroring;
//! * repeated reads are pure and deterministic.

#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &token);
    (env, client, issuer, token)
}

fn register_offering(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
) {
    let payout_asset = Address::generate(env);
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        &symbol_short!("def"),
        token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
}

#[test]
fn snapshot_config_defaults_to_disabled() {
    let (_env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    assert!(!client.get_snapshot_config(&issuer, &ns, &token));
}

#[test]
fn snapshot_config_reflects_enable_and_disable() {
    let (_env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_snapshot_config(&issuer, &ns, &token, &true);
    assert!(client.get_snapshot_config(&issuer, &ns, &token));

    client.set_snapshot_config(&issuer, &ns, &token, &false);
    assert!(!client.get_snapshot_config(&issuer, &ns, &token));
}

#[test]
fn snapshot_config_is_scoped_per_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    let issuer_a = Address::generate(&env);
    let token_a = Address::generate(&env);
    register_offering(&env, &client, &issuer_a, &token_a);

    let issuer_b = Address::generate(&env);
    let token_b = Address::generate(&env);
    register_offering(&env, &client, &issuer_b, &token_b);

    client.set_snapshot_config(&issuer_a, &ns, &token_a, &true);

    assert!(client.get_snapshot_config(&issuer_a, &ns, &token_a));
    assert!(!client.get_snapshot_config(&issuer_b, &ns, &token_b));
}

#[test]
fn snapshot_config_for_unknown_offering_reads_disabled() {
    let (env, client, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);

    assert!(!client.get_snapshot_config(&issuer, &ns, &unknown_token));
}

#[test]
fn snapshot_config_reads_are_deterministic() {
    let (_env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_snapshot_config(&issuer, &ns, &token, &true);

    assert_eq!(
        client.get_snapshot_config(&issuer, &ns, &token),
        client.get_snapshot_config(&issuer, &ns, &token)
    );
    assert!(client.get_snapshot_config(&issuer, &ns, &token));
}
