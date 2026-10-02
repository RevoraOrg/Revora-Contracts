#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

fn setup() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register_contract(None, RevoraRevenueShare);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);
    let namespace = symbol_short!("def");

    let client = RevoraRevenueShareClient::new(&env, &contract);
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &1_000u32,
        &token,
        &0_i128,
        &symbol_short!(""),
        &0u32,
    );
    (env, contract, issuer, token, investor)
}

#[test]
fn removes_unique_entries_and_preserves_remaining_state() {
    let (env, contract, issuer, token, first) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract);
    let namespace = symbol_short!("def");
    let second = Address::generate(&env);
    let remaining = Address::generate(&env);
    let absent = Address::generate(&env);
    let mut seeded = Vec::new(&env);
    seeded.push_back(first.clone());
    seeded.push_back(second.clone());
    seeded.push_back(remaining.clone());
    client.blacklist_add_many(&issuer, &issuer, &namespace, &token, &seeded);
    let mut removals = Vec::new(&env);
    removals.push_back(first.clone());
    removals.push_back(first);
    removals.push_back(absent);
    removals.push_back(second);
    client.blacklist_remove_many(&issuer, &issuer, &namespace, &token, &removals);
    assert!(!client.is_blacklisted(&issuer, &namespace, &token, &removals.get(0).unwrap()));
    assert!(client.is_blacklisted(&issuer, &namespace, &token, &remaining));
}

#[test]
fn rejects_oversized_batch_without_mutation() {
    let (env, contract, issuer, token, investor) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract);
    let namespace = symbol_short!("def");
    client.blacklist_add(&issuer, &issuer, &namespace, &token, &investor);
    let mut oversized = Vec::new(&env);
    for _ in 0..51 {
        oversized.push_back(Address::generate(&env));
    }
    assert_eq!(
        client.try_blacklist_remove_many(&issuer, &issuer, &namespace, &token, &oversized),
        Err(Ok(RevoraError::LimitReached))
    );
    assert!(client.is_blacklisted(&issuer, &namespace, &token, &investor));
}

#[test]
fn rejects_unauthorized_batch_without_mutation() {
    let (env, contract, issuer, token, investor) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract);
    let namespace = symbol_short!("def");
    client.blacklist_add(&issuer, &issuer, &namespace, &token, &investor);
    let attacker = Address::generate(&env);
    let mut removals = Vec::new(&env);
    removals.push_back(investor.clone());
    assert_eq!(
        client.try_blacklist_remove_many(&attacker, &issuer, &namespace, &token, &removals),
        Err(Ok(RevoraError::NotAuthorized))
    );
    assert!(client.is_blacklisted(&issuer, &namespace, &token, &investor));
}
