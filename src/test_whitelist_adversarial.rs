//! Adversarial coverage for [`RevoraRevenueShare::is_whitelisted`] and the
//! write paths that feed it (`whitelist_add` / `whitelist_remove`).
//!
//! The whitelist is a per-offering address allowlist.  These tests pin the
//! read/write contract:
//!
//! * an offering with no allowlist configured reports `false`;
//! * addresses that were never listed report `false`;
//! * listing an investor flips the read to `true` and is idempotent;
//! * removing the investor flips the read back to `false`, and removing an
//!   unlisted address is a safe no-op;
//! * a non-issuer, non-admin caller is rejected with `NotAuthorized` and the
//!   allowlist is left untouched;
//! * the contract admin may manage the allowlist alongside the issuer;
//! * `whitelist_add` for an unknown offering is rejected with `OfferingNotFound`;
//! * the allowlist is isolated per token and per offering.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

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

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &token);
    (env, client, admin, issuer, token)
}

#[test]
fn returns_false_for_unconfigured_allowlist() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let investor = Address::generate(&env);

    assert!(!client.is_whitelisted(&issuer, &ns, &token, &investor));
}

#[test]
fn returns_false_for_unknown_offering() {
    let (env, client, _admin, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);
    let investor = Address::generate(&env);

    assert!(!client.is_whitelisted(&issuer, &ns, &unknown_token, &investor));
}

#[test]
fn add_lists_investor_and_is_idempotent() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let investor = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));

    // Listing the same investor again must not error or flip the read.
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));

    // Unrelated addresses stay unlisted.
    let other = Address::generate(&env);
    assert!(!client.is_whitelisted(&issuer, &ns, &token, &other));
}

#[test]
fn remove_reverts_read_to_false() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let investor = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));

    client.whitelist_remove(&issuer, &issuer, &ns, &token, &investor);
    assert!(!client.is_whitelisted(&issuer, &ns, &token, &investor));
}

#[test]
fn removing_an_unlisted_address_is_a_safe_noop() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let investor = Address::generate(&env);

    client.whitelist_remove(&issuer, &issuer, &ns, &token, &investor);
    assert!(!client.is_whitelisted(&issuer, &ns, &token, &investor));
}

#[test]
fn unauthorized_caller_is_rejected_without_mutating_state() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let attacker = Address::generate(&env);
    let investor = Address::generate(&env);

    let result = client.try_whitelist_add(&attacker, &issuer, &ns, &token, &investor);
    assert!(matches!(result.err(), Some(Ok(RevoraError::NotAuthorized))));
    assert!(!client.is_whitelisted(&issuer, &ns, &token, &investor));

    // Removal by an unauthorized caller is equally rejected, and cannot clear a
    // legitimate listing.
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);
    let remove_result = client.try_whitelist_remove(&attacker, &issuer, &ns, &token, &investor);
    assert!(matches!(remove_result.err(), Some(Ok(RevoraError::NotAuthorized))));
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));
}

#[test]
fn contract_admin_may_manage_the_allowlist() {
    let (env, client, admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let investor = Address::generate(&env);

    client.whitelist_add(&admin, &issuer, &ns, &token, &investor);
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));

    client.whitelist_remove(&admin, &issuer, &ns, &token, &investor);
    assert!(!client.is_whitelisted(&issuer, &ns, &token, &investor));
}

#[test]
fn add_rejects_unknown_offering() {
    let (env, client, _admin, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);
    let investor = Address::generate(&env);

    let result = client.try_whitelist_add(&issuer, &issuer, &ns, &unknown_token, &investor);
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));
    assert!(!client.is_whitelisted(&issuer, &ns, &unknown_token, &investor));
}

#[test]
fn allowlist_is_isolated_per_token_and_per_offering() {
    let (env, client, _admin, issuer_a, token_a) = setup();
    let ns = symbol_short!("def");
    let investor = Address::generate(&env);

    // A second token for the SAME issuer, and a whole other offering.
    let token_a2 = Address::generate(&env);
    register_offering(&env, &client, &issuer_a, &token_a2);
    let issuer_b = Address::generate(&env);
    let token_b = Address::generate(&env);
    register_offering(&env, &client, &issuer_b, &token_b);

    client.whitelist_add(&issuer_a, &issuer_a, &ns, &token_a, &investor);

    assert!(client.is_whitelisted(&issuer_a, &ns, &token_a, &investor));
    // Same issuer, different token: not listed.
    assert!(!client.is_whitelisted(&issuer_a, &ns, &token_a2, &investor));
    // Different offering entirely: not listed.
    assert!(!client.is_whitelisted(&issuer_b, &ns, &token_b, &investor));
}
