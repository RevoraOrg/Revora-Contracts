//! Adversarial coverage for `get_rounding_mode` / `set_rounding_mode` (#1122).
//!
//! `get_rounding_mode` is a read path over per-offering configuration, so the
//! behaviour worth pinning is:
//! - the default when nothing was ever written (`Truncation`),
//! - the round-trip after a write, including changing the mode back,
//! - per-offering isolation across token and namespace,
//! - persistence across contract-client instances,
//! - a pure read must not materialise storage, and
//! - rejected writes (unknown offering / non-issuer caller) leave state alone.

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec as SdkVec};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &SdkVec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    (env, contract_id, issuer, token)
}

fn mode_key(issuer: &Address, namespace: &Symbol, token: &Address) -> DataKey {
    DataKey::RoundingMode(OfferingId {
        issuer: issuer.clone(),
        namespace: namespace.clone(),
        token: token.clone(),
    })
}

#[test]
fn unset_rounding_mode_defaults_to_truncation() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    assert_eq!(
        client.get_rounding_mode(&issuer, &symbol_short!("def"), &token),
        RoundingMode::Truncation
    );
}

#[test]
fn reading_rounding_mode_does_not_write_storage() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let key = mode_key(&issuer, &symbol_short!("def"), &token);

    assert!(!env.storage().persistent().has(&key));

    // Reading repeatedly must stay a pure read: no entry is created.
    assert_eq!(
        client.get_rounding_mode(&issuer, &symbol_short!("def"), &token),
        RoundingMode::Truncation
    );
    assert_eq!(
        client.get_rounding_mode(&issuer, &symbol_short!("def"), &token),
        RoundingMode::Truncation
    );
    assert!(!env.storage().persistent().has(&key));
}

#[test]
fn set_then_get_round_trips_and_can_be_changed_back() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_rounding_mode(&issuer, &ns, &token, &RoundingMode::RoundHalfUp);
    assert_eq!(
        client.get_rounding_mode(&issuer, &ns, &token),
        RoundingMode::RoundHalfUp
    );

    client.set_rounding_mode(&issuer, &ns, &token, &RoundingMode::Truncation);
    assert_eq!(
        client.get_rounding_mode(&issuer, &ns, &token),
        RoundingMode::Truncation
    );
}

#[test]
fn rounding_mode_is_scoped_per_offering() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let other_token = Address::generate(&env);
    let other_ns = symbol_short!("alt");

    client.set_rounding_mode(&issuer, &ns, &token, &RoundingMode::RoundHalfUp);

    assert_eq!(
        client.get_rounding_mode(&issuer, &ns, &token),
        RoundingMode::RoundHalfUp
    );
    assert_eq!(
        client.get_rounding_mode(&issuer, &ns, &other_token),
        RoundingMode::Truncation
    );
    assert_eq!(
        client.get_rounding_mode(&issuer, &other_ns, &token),
        RoundingMode::Truncation
    );
}

#[test]
fn rounding_mode_survives_a_new_client_instance() {
    let (env, contract_id, issuer, token) = setup();
    let ns = symbol_short!("def");

    {
        let client = RevoraRevenueShareClient::new(&env, &contract_id);
        client.set_rounding_mode(&issuer, &ns, &token, &RoundingMode::RoundHalfUp);
    }

    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    assert_eq!(
        client.get_rounding_mode(&issuer, &ns, &token),
        RoundingMode::RoundHalfUp
    );
}

#[test]
fn setting_rounding_mode_for_unknown_offering_is_rejected_without_state_change() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let unknown_issuer = Address::generate(&env);
    let ns = symbol_short!("def");

    let result =
        client.try_set_rounding_mode(&unknown_issuer, &ns, &token, &RoundingMode::RoundHalfUp);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    assert!(!env
        .storage()
        .persistent()
        .has(&mode_key(&unknown_issuer, &ns, &token)));
    // The read path stays permissive and reports the default.
    assert_eq!(
        client.get_rounding_mode(&unknown_issuer, &ns, &token),
        RoundingMode::Truncation
    );
    assert_eq!(
        client.get_rounding_mode(&issuer, &ns, &token),
        RoundingMode::Truncation
    );
}

#[test]
fn setting_rounding_mode_from_non_issuer_is_rejected() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let impostor = Address::generate(&env);
    let ns = symbol_short!("def");

    let result = client.try_set_rounding_mode(&impostor, &ns, &token, &RoundingMode::RoundHalfUp);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(
        client.get_rounding_mode(&issuer, &ns, &token),
        RoundingMode::Truncation
    );
}
