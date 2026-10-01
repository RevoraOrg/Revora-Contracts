//! Adversarial coverage for `blacklist_remove` (#1095).
//!
//! `blacklist_remove` re-enables an investor to claim revenue for an offering,
//! so the happy path (add then remove) is not the interesting part — the
//! interesting part is everything it must refuse to do. This suite pins:
//!
//! - the removal actually clears the entry and the deterministic order vector,
//! - the operation is idempotent for investors that were never blacklisted,
//! - a non-issuer and a non-existent offering are both rejected, and the
//!   blacklist is left byte-for-byte unchanged when that happens,
//! - the admin escape hatch is honoured (mirroring the add-side guard),
//! - removals are scoped to a single offering, so a second namespace keeps its
//!   own blacklist, and
//! - a globally frozen contract blocks the mutation without touching state.

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec as SdkVec};

const NS: Symbol = symbol_short!("def");
const NS_ALT: Symbol = symbol_short!("alt");

fn setup() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &SdkVec::new(&env),
        &1u32,
        &NS,
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    (env, contract_id, issuer, admin, token)
}

fn blacklist(env: &Env, contract_id: &Address, issuer: &Address, token: &Address) -> SdkVec<Address> {
    RevoraRevenueShareClient::new(env, contract_id).get_blacklist(issuer, &NS, token)
}

#[test]
fn remove_clears_the_entry_and_the_order_vector() {
    let (env, contract_id, issuer, _admin, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &NS, &token, &investor);
    assert!(client.is_blacklisted(&issuer, &NS, &token, &investor));
    assert_eq!(client.get_blacklist_size(&issuer, &NS, &token), 1);

    client.blacklist_remove(&issuer, &issuer, &NS, &token, &investor);

    assert!(!client.is_blacklisted(&issuer, &NS, &token, &investor));
    assert_eq!(client.get_blacklist_size(&issuer, &NS, &token), 0);
    assert_eq!(blacklist(&env, &contract_id, &issuer, &token).len(), 0);
}

#[test]
fn remove_is_idempotent_for_a_never_blacklisted_investor() {
    let (env, contract_id, issuer, _admin, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let investor = Address::generate(&env);
    let other = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &NS, &token, &other);

    // Removing an address that was never blacklisted must succeed and be a no-op.
    client.blacklist_remove(&issuer, &issuer, &NS, &token, &investor);
    client.blacklist_remove(&issuer, &issuer, &NS, &token, &investor);

    assert!(!client.is_blacklisted(&issuer, &NS, &token, &investor));
    assert!(client.is_blacklisted(&issuer, &NS, &token, &other));
    assert_eq!(client.get_blacklist_size(&issuer, &NS, &token), 1);
    assert_eq!(blacklist(&env, &contract_id, &issuer, &token).len(), 1);
}

#[test]
fn non_issuer_caller_is_rejected_and_state_is_unchanged() {
    let (env, contract_id, issuer, _admin, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let investor = Address::generate(&env);
    let stranger = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &NS, &token, &investor);

    let result = client.try_blacklist_remove(&stranger, &issuer, &NS, &token, &investor);

    assert!(result.is_err(), "a non-issuer must not be able to re-enable an investor");
    assert!(client.is_blacklisted(&issuer, &NS, &token, &investor));
    assert_eq!(client.get_blacklist_size(&issuer, &NS, &token), 1);
}

#[test]
fn admin_caller_can_remove_on_behalf_of_the_issuer() {
    let (env, contract_id, issuer, admin, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &NS, &token, &investor);
    client.blacklist_remove(&admin, &issuer, &NS, &token, &investor);

    assert!(!client.is_blacklisted(&issuer, &NS, &token, &investor));
    assert_eq!(client.get_blacklist_size(&issuer, &NS, &token), 0);
}

#[test]
fn unknown_offering_is_rejected_and_creates_nothing() {
    let (env, contract_id, issuer, _admin, _token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let unknown_token = Address::generate(&env);
    let investor = Address::generate(&env);

    let result = client.try_blacklist_remove(&issuer, &issuer, &NS, &unknown_token, &investor);

    assert!(result.is_err(), "an unregistered offering must be rejected");
    assert_eq!(client.get_blacklist_size(&issuer, &NS, &unknown_token), 0);
    assert!(!client.is_blacklisted(&issuer, &NS, &unknown_token, &investor));
}

#[test]
fn removing_one_investor_preserves_the_remaining_order() {
    let (env, contract_id, issuer, _admin, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    let third = Address::generate(&env);

    for investor in [first.clone(), second.clone(), third.clone()] {
        client.blacklist_add(&issuer, &issuer, &NS, &token, &investor);
    }

    client.blacklist_remove(&issuer, &issuer, &NS, &token, &second);

    let remaining = blacklist(&env, &contract_id, &issuer, &token);
    assert_eq!(remaining.len(), 2);
    assert_eq!(remaining.get(0).unwrap(), first);
    assert_eq!(remaining.get(1).unwrap(), third);
    assert_eq!(client.get_blacklist_size(&issuer, &NS, &token), 2);

    // No stale order entry survived the removal.
    assert!(remaining.get(0).unwrap() != second);
    assert!(remaining.get(1).unwrap() != second);
}

#[test]
fn removal_is_scoped_to_the_requested_offering() {
    let (env, contract_id, issuer, _admin, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let alt_token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let investor = Address::generate(&env);

    client.register_offering(
        &issuer,
        &SdkVec::new(&env),
        &1u32,
        &NS_ALT,
        &alt_token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    client.blacklist_add(&issuer, &issuer, &NS, &token, &investor);
    client.blacklist_add(&issuer, &issuer, &NS_ALT, &alt_token, &investor);

    client.blacklist_remove(&issuer, &issuer, &NS, &token, &investor);

    assert!(!client.is_blacklisted(&issuer, &NS, &token, &investor));
    assert!(
        client.is_blacklisted(&issuer, &NS_ALT, &alt_token, &investor),
        "removing from one namespace must not affect another"
    );
    assert_eq!(client.get_blacklist_size(&issuer, &NS, &token), 0);
    assert_eq!(client.get_blacklist_size(&issuer, &NS_ALT, &alt_token), 1);
}

#[test]
fn frozen_contract_blocks_removal_and_leaves_state_untouched() {
    let (env, contract_id, issuer, _admin, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &NS, &token, &investor);

    assert!(client.freeze().is_ok());

    let result = client.try_blacklist_remove(&issuer, &issuer, &NS, &token, &investor);

    assert!(result.is_err(), "a frozen contract must reject the mutation");
    assert!(client.is_blacklisted(&issuer, &NS, &token, &investor));
    assert_eq!(client.get_blacklist_size(&issuer, &NS, &token), 1);
}

#[test]
fn remove_then_re_add_round_trip_is_deterministic() {
    let (env, contract_id, issuer, _admin, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &NS, &token, &investor);
    client.blacklist_remove(&issuer, &issuer, &NS, &token, &investor);
    client.blacklist_add(&issuer, &issuer, &NS, &token, &investor);

    assert!(client.is_blacklisted(&issuer, &NS, &token, &investor));
    assert_eq!(client.get_blacklist_size(&issuer, &NS, &token), 1);

    let order = blacklist(&env, &contract_id, &issuer, &token);
    assert_eq!(order.len(), 1);
    assert_eq!(order.get(0).unwrap(), investor);

    client.blacklist_remove(&issuer, &issuer, &NS, &token, &investor);
    assert_eq!(blacklist(&env, &contract_id, &issuer, &token).len(), 0);
    assert_eq!(client.get_blacklist_size(&issuer, &NS, &token), 0);
}
