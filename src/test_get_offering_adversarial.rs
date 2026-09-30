//! Adversarial behavior coverage for the public `get_offering` reader (#1074).

#![cfg(test)]

use crate::{Offering, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, Symbol, Vec,
};

fn setup() -> (Env, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    (env, client)
}

fn register(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    bps: u32,
) {
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1,
        namespace,
        token,
        &bps,
        token,
        &0,
        &symbol_short!(""),
        &0,
    );
}

#[test]
fn returns_the_exact_registered_offering() {
    let (env, client) = setup();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let namespace = symbol_short!("primary");
    register(&env, &client, &issuer, &namespace, &token, 2_500);

    let offering = client
        .get_offering(&issuer, &namespace, &token)
        .expect("registered offering must be returned");

    assert_eq!(
        offering,
        Offering {
            issuers: crate::Issuers { primary: issuer, co: Vec::new(&env), quorum: 1 },
            namespace,
            token: token.clone(),
            revenue_share_bps: 2_500,
            payout_asset: token,
            denomination_symbol: symbol_short!(""),
            display_decimals: 0,
        }
    );
}

#[test]
fn unknown_and_boundary_identities_return_none_without_panicking() {
    let (env, client) = setup();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    assert_eq!(client.get_offering(&issuer, &symbol_short!(""), &token), None);
    assert_eq!(
        client.get_offering(&issuer, &Symbol::new(&env, "namespace-at-boundary"), &token),
        None
    );
}

#[test]
fn lookup_is_isolated_by_issuer_namespace_and_token() {
    let (env, client) = setup();
    let issuer = Address::generate(&env);
    let other_issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let other_token = Address::generate(&env);
    let namespace = symbol_short!("prod");
    register(&env, &client, &issuer, &namespace, &token, 1_000);

    assert!(client.get_offering(&issuer, &namespace, &token).is_some());
    assert!(client.get_offering(&other_issuer, &namespace, &token).is_none());
    assert!(client.get_offering(&issuer, &symbol_short!("stage"), &token).is_none());
    assert!(client.get_offering(&issuer, &namespace, &other_token).is_none());
}

#[test]
fn repeated_reads_are_public_and_side_effect_free() {
    let (env, client) = setup();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let namespace = symbol_short!("public");
    register(&env, &client, &issuer, &namespace, &token, 750);

    let expected = client.get_offering(&issuer, &namespace, &token);
    let auths_before = env.auths().len();
    let events_before = env.events().all().len();
    let count_before = client.get_offering_count(&issuer, &namespace);

    for _ in 0..5 {
        assert_eq!(client.get_offering(&issuer, &namespace, &token), expected);
    }

    assert_eq!(env.auths().len(), auths_before, "getter must not require authorization");
    assert_eq!(env.events().all().len(), events_before, "getter must not emit events");
    assert_eq!(client.get_offering_count(&issuer, &namespace), count_before);
}

#[test]
fn rejected_registration_leaves_existing_and_missing_lookups_unchanged() {
    let (env, client) = setup();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let rejected_token = Address::generate(&env);
    let namespace = symbol_short!("stable");
    register(&env, &client, &issuer, &namespace, &token, 1_000);

    let before = client.get_offering(&issuer, &namespace, &token);
    let count_before = client.get_offering_count(&issuer, &namespace);
    let rejected = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1,
        &namespace,
        &rejected_token,
        &10_001,
        &rejected_token,
        &0,
        &symbol_short!(""),
        &0,
    );

    assert_eq!(rejected, Err(Ok(RevoraError::InvalidRevenueShareBps)));
    assert_eq!(client.get_offering(&issuer, &namespace, &token), before);
    assert_eq!(client.get_offering(&issuer, &namespace, &rejected_token), None);
    assert_eq!(client.get_offering_count(&issuer, &namespace), count_before);
}
