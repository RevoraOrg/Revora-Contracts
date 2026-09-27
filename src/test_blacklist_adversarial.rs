//! # Adversarial coverage for `is_blacklisted`
//!
//! `is_blacklisted` is a pure, unauthenticated read used by payout/report paths
//! to decide whether an investor is eligible. Its result is keyed on the full
//! `(issuer, namespace, token, investor)` tuple, so the important adversarial
//! properties are isolation (no cross-offering or cross-namespace bleed) and a
//! fail-closed `false` for unknown offerings rather than a panic.
//!
//! ## Coverage matrix
//!
//! | Scenario | Expected |
//! |---|---|
//! | Unknown offering / investor | `false` (no panic) |
//! | Added investor | `true` |
//! | Other, non-added investor | `false` |
//! | After `blacklist_remove` | `false` |
//! | `blacklist_remove` on a non-blacklisted investor | `false`, no error |
//! | Same investor, sibling offering | `false` |
//! | Same token, different namespace | `false` |
//! | Multiple investors, partial removal | only the removed one flips to `false` |

#![cfg(test)]
#![allow(unused_imports)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Register a minimal offering owned by `issuer` under (`namespace`, `token`).
fn register(
    client: &RevoraRevenueShareClient,
    env: &Env,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) {
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        namespace,
        token,
        &1_000,
        token,
        &0,
        &symbol_short!(""),
        &0,
    );
}

#[test]
fn unknown_offering_reads_false_without_panicking() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    assert!(!client.is_blacklisted(&issuer, &symbol_short!("ns"), &token, &investor));
}

#[test]
fn added_investor_is_blacklisted_and_others_are_not() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);

    let flagged = Address::generate(&env);
    let clean = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &flagged);

    assert!(client.is_blacklisted(&issuer, &ns, &token, &flagged));
    assert!(!client.is_blacklisted(&issuer, &ns, &token, &clean));
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 1);
}

#[test]
fn remove_clears_the_blacklist_flag() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);
    assert!(client.is_blacklisted(&issuer, &ns, &token, &investor));

    client.blacklist_remove(&issuer, &issuer, &ns, &token, &investor);
    assert!(!client.is_blacklisted(&issuer, &ns, &token, &investor));
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 0);
}

#[test]
fn remove_of_non_blacklisted_investor_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);
    let investor = Address::generate(&env);

    // Removing an investor who was never blacklisted must be a no-op, not a panic.
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &investor);

    assert!(!client.is_blacklisted(&issuer, &ns, &token, &investor));
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 0);
}

#[test]
fn blacklist_is_isolated_between_sibling_offerings() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token_a);
    register(&client, &env, &issuer, &ns, &token_b);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token_a, &investor);

    assert!(client.is_blacklisted(&issuer, &ns, &token_a, &investor));
    assert!(
        !client.is_blacklisted(&issuer, &ns, &token_b, &investor),
        "blacklisting on one offering must not leak to a sibling offering"
    );
}

#[test]
fn blacklist_is_isolated_between_namespaces() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    register(&client, &env, &issuer, &symbol_short!("aa"), &token_a);
    register(&client, &env, &issuer, &symbol_short!("bb"), &token_b);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &symbol_short!("aa"), &token_a, &investor);

    assert!(client.is_blacklisted(&issuer, &symbol_short!("aa"), &token_a, &investor));
    assert!(!client.is_blacklisted(&issuer, &symbol_short!("bb"), &token_b, &investor));
    // Same token but a namespace it was never registered under must not be blacklisted.
    assert!(!client.is_blacklisted(&issuer, &symbol_short!("cc"), &token_a, &investor));
}

#[test]
fn multiple_investors_are_tracked_independently() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);

    let first = Address::generate(&env);
    let second = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &first);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &second);
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 2);

    client.blacklist_remove(&issuer, &issuer, &ns, &token, &first);

    assert!(!client.is_blacklisted(&issuer, &ns, &token, &first));
    assert!(client.is_blacklisted(&issuer, &ns, &token, &second));
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 1);
}

#[test]
fn removing_an_unrelated_investor_does_not_clear_a_blacklisted_one() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);

    let flagged = Address::generate(&env);
    let unrelated = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &flagged);

    client.blacklist_remove(&issuer, &issuer, &ns, &token, &unrelated);

    assert!(client.is_blacklisted(&issuer, &ns, &token, &flagged));
    assert!(!client.is_blacklisted(&issuer, &ns, &token, &unrelated));
}
