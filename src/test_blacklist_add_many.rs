#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn setup_offering(
    env: &Env,
    client: &RevoraRevenueShareClient<'_>,
) -> (Address, Address, Address, Symbol) {
    let admin = Address::generate(env);
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let namespace = symbol_short!("def");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        &namespace,
        &token,
        &1_000u32,
        &token,
        &0_i128,
        &symbol_short!(""),
        &0u32,
    );

    (admin, issuer, token, namespace)
}

#[test]
fn blacklist_add_many_accepts_authorized_single_and_multiple_investors() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (admin, issuer, token, namespace) = setup_offering(&env, &client);
    let single = Address::generate(&env);
    let investor_a = Address::generate(&env);
    let investor_b = Address::generate(&env);

    assert_eq!(client.get_blacklist_size(&issuer, &namespace, &token), 0);
    let mut single_batch = Vec::new(&env);
    single_batch.push_back(single.clone());
    client.blacklist_add_many(&issuer, &issuer, &namespace, &token, &single_batch);

    assert!(client.is_blacklisted(&issuer, &namespace, &token, &single));
    let mut multiple_batch = Vec::new(&env);
    multiple_batch.push_back(investor_a.clone());
    multiple_batch.push_back(investor_b.clone());
    client.blacklist_add_many(&admin, &issuer, &namespace, &token, &multiple_batch);

    let mut expected = Vec::new(&env);
    expected.push_back(single.clone());
    expected.push_back(investor_a.clone());
    expected.push_back(investor_b.clone());
    assert_eq!(client.get_blacklist(&issuer, &namespace, &token), expected);
    assert_eq!(client.get_blacklist_size(&issuer, &namespace, &token), 3);
    assert!(client.is_blacklisted(&issuer, &namespace, &token, &investor_a));
    assert!(client.is_blacklisted(&issuer, &namespace, &token, &investor_b));
    assert!(!client.is_blacklisted(&issuer, &symbol_short!("other"), &token, &investor_a));
}

#[test]
fn blacklist_add_many_empty_batch_is_a_no_op() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (_, issuer, token, namespace) = setup_offering(&env, &client);
    let existing = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &namespace, &token, &existing);
    let before = client.get_blacklist(&issuer, &namespace, &token);
    let empty: Vec<Address> = Vec::new(&env);

    client.blacklist_add_many(&issuer, &issuer, &namespace, &token, &empty);

    assert_eq!(client.get_blacklist(&issuer, &namespace, &token), before);
    assert_eq!(client.get_blacklist_size(&issuer, &namespace, &token), 1);
}

#[test]
fn blacklist_add_many_duplicate_and_existing_entries_are_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (_, issuer, token, namespace) = setup_offering(&env, &client);
    let existing = Address::generate(&env);
    let new_investor = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &namespace, &token, &existing);

    let mut batch = Vec::new(&env);
    batch.push_back(existing.clone());
    batch.push_back(new_investor.clone());
    batch.push_back(existing.clone());
    batch.push_back(new_investor.clone());
    client.blacklist_add_many(&issuer, &issuer, &namespace, &token, &batch);

    let mut expected = Vec::new(&env);
    expected.push_back(existing);
    expected.push_back(new_investor.clone());
    assert_eq!(client.get_blacklist(&issuer, &namespace, &token), expected);
    assert_eq!(client.get_blacklist_size(&issuer, &namespace, &token), 2);
    assert!(client.is_blacklisted(&issuer, &namespace, &token, &new_investor));
}

#[test]
fn blacklist_add_many_accepts_the_maximum_batch_size() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (_, issuer, token, namespace) = setup_offering(&env, &client);
    let mut batch = Vec::new(&env);
    for _ in 0..50 {
        batch.push_back(Address::generate(&env));
    }

    client.blacklist_add_many(&issuer, &issuer, &namespace, &token, &batch);

    assert_eq!(client.get_blacklist(&issuer, &namespace, &token), batch);
    assert_eq!(client.get_blacklist_size(&issuer, &namespace, &token), 50);
}

#[test]
fn blacklist_add_many_unauthorized_caller_preserves_state() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (_, issuer, token, namespace) = setup_offering(&env, &client);
    let existing = Address::generate(&env);
    let candidate_a = Address::generate(&env);
    let candidate_b = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &namespace, &token, &existing);
    let before = client.get_blacklist(&issuer, &namespace, &token);
    let mut batch = Vec::new(&env);
    batch.push_back(candidate_a.clone());
    batch.push_back(candidate_b.clone());
    let attacker = Address::generate(&env);

    let result = client.try_blacklist_add_many(&attacker, &issuer, &namespace, &token, &batch);

    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));
    assert_eq!(client.get_blacklist(&issuer, &namespace, &token), before);
    assert!(client.is_blacklisted(&issuer, &namespace, &token, &existing));
    assert!(!client.is_blacklisted(&issuer, &namespace, &token, &candidate_a));
    assert!(!client.is_blacklisted(&issuer, &namespace, &token, &candidate_b));
}

#[test]
fn blacklist_add_many_mismatched_offering_context_preserves_state() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (_, issuer, token, namespace) = setup_offering(&env, &client);
    let existing = Address::generate(&env);
    let candidate = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &namespace, &token, &existing);
    let before = client.get_blacklist(&issuer, &namespace, &token);
    let mut batch = Vec::new(&env);
    batch.push_back(candidate.clone());

    let wrong_issuer = Address::generate(&env);
    let wrong_issuer_result =
        client.try_blacklist_add_many(&issuer, &wrong_issuer, &namespace, &token, &batch);
    assert_eq!(wrong_issuer_result, Err(Ok(RevoraError::OfferingNotFound)));

    let wrong_namespace = symbol_short!("wrong");
    let wrong_namespace_result =
        client.try_blacklist_add_many(&issuer, &issuer, &wrong_namespace, &token, &batch);
    assert_eq!(wrong_namespace_result, Err(Ok(RevoraError::OfferingNotFound)));

    let wrong_token = Address::generate(&env);
    let wrong_token_result =
        client.try_blacklist_add_many(&issuer, &issuer, &namespace, &wrong_token, &batch);
    assert_eq!(wrong_token_result, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(client.get_blacklist(&issuer, &namespace, &token), before);
    assert!(!client.is_blacklisted(&issuer, &namespace, &token, &candidate));
    assert_eq!(client.get_blacklist_size(&issuer, &wrong_namespace, &token), 0);
    assert_eq!(client.get_blacklist_size(&issuer, &namespace, &wrong_token), 0);
}

#[test]
fn blacklist_add_many_over_capacity_rejects_without_partial_mutation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (_, issuer, token, namespace) = setup_offering(&env, &client);
    let existing = Address::generate(&env);
    let candidate_a = Address::generate(&env);
    let candidate_b = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &namespace, &token, &existing);
    client.set_blacklist_size_limit(&issuer, &issuer, &namespace, &token, &2u32);
    let before = client.get_blacklist(&issuer, &namespace, &token);
    let mut batch = Vec::new(&env);
    batch.push_back(candidate_a.clone());
    batch.push_back(candidate_b.clone());

    let result = client.try_blacklist_add_many(&issuer, &issuer, &namespace, &token, &batch);

    assert_eq!(result, Err(Ok(RevoraError::BlacklistSizeLimitExceeded)));
    assert_eq!(client.get_blacklist(&issuer, &namespace, &token), before);
    assert_eq!(client.get_blacklist_size(&issuer, &namespace, &token), 1);
    assert!(client.is_blacklisted(&issuer, &namespace, &token, &existing));
    assert!(!client.is_blacklisted(&issuer, &namespace, &token, &candidate_a));
    assert!(!client.is_blacklisted(&issuer, &namespace, &token, &candidate_b));
}

#[test]
fn blacklist_add_many_rejects_batches_over_the_input_limit_without_mutation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (_, issuer, token, namespace) = setup_offering(&env, &client);
    let mut batch = Vec::new(&env);
    for _ in 0..51 {
        batch.push_back(Address::generate(&env));
    }

    let result = client.try_blacklist_add_many(&issuer, &issuer, &namespace, &token, &batch);

    assert_eq!(result, Err(Ok(RevoraError::LimitReached)));
    assert_eq!(client.get_blacklist_size(&issuer, &namespace, &token), 0);
    assert!(client.get_blacklist(&issuer, &namespace, &token).is_empty());
}
