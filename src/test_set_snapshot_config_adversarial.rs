//! Adversarial coverage for [`RevoraRevenueShare::set_snapshot_config`].
//!
//! `set_snapshot_config` toggles the snapshot workflow for an offering.  These
//! tests pin the write-side contract:
//!
//! * enabling and disabling is idempotent and readable back;
//! * an unknown offering is rejected with `OfferingNotFound`;
//! * addressing another offering's token with a different issuer is rejected;
//! * a rejected call leaves the previously stored configuration untouched;
//! * configuration is isolated per offering.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
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
fn set_snapshot_config_toggles_the_flag() {
    let (_env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_snapshot_config(&issuer, &ns, &token, &true);
    assert!(client.get_snapshot_config(&issuer, &ns, &token));

    // Idempotent re-enable.
    client.set_snapshot_config(&issuer, &ns, &token, &true);
    assert!(client.get_snapshot_config(&issuer, &ns, &token));

    client.set_snapshot_config(&issuer, &ns, &token, &false);
    assert!(!client.get_snapshot_config(&issuer, &ns, &token));
}

#[test]
fn set_snapshot_config_rejects_unknown_offering() {
    let (env, client, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);

    let result = client.try_set_snapshot_config(&issuer, &ns, &unknown_token, &true);
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));
}

#[test]
fn set_snapshot_config_rejects_mismatched_issuer_coordinates() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    let issuer_a = Address::generate(&env);
    let token_a = Address::generate(&env);
    register_offering(&env, &client, &issuer_a, &token_a);

    let issuer_b = Address::generate(&env);

    let result = client.try_set_snapshot_config(&issuer_b, &ns, &token_a, &true);
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));
}

#[test]
fn set_snapshot_config_rejected_call_preserves_previous_state() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);

    client.set_snapshot_config(&issuer, &ns, &token, &true);

    let result = client.try_set_snapshot_config(&issuer, &ns, &unknown_token, &false);
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));

    assert!(client.get_snapshot_config(&issuer, &ns, &token));
}

#[test]
fn set_snapshot_config_is_scoped_per_offering() {
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
