//! Adversarial coverage for `RevoraRevenueShare::get_supply_cap` (Issue #1032).
//!
//! `get_supply_cap` is a permissionless read that feeds the deposit cap check in
//! `report_revenue`: a value that is wrong, leaking across offerings, or mutable
//! by a rejected call changes when an offering stops accepting revenue. The
//! happy path (a non-zero cap is stored and read back) is the least interesting
//! part of that contract. This fixture pins the adversarial edges:
//!
//! * absence — an unknown offering reads as `0` ("no cap"), never `None`/panic;
//! * explicit zero — a `0` cap is *not* persisted, so it can never be confused
//!   with a real cap;
//! * isolation — caps are keyed by `(issuer, namespace, token)` and never leak
//!   between offerings that differ in any one of those components;
//! * boundaries — `1` and `i128::MAX` round-trip exactly;
//! * rejected writes — a negative cap is refused by the amount-validation matrix
//!   and leaves the previously stored cap untouched;
//! * idempotent re-registration — the duplicate guard returns early, so a second
//!   `register_offering` cannot silently overwrite an established cap;
//! * purity — reads never mutate state and are stable across repeated calls.

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol, Vec};

const BPS_50_PCT: u32 = 5_000;

fn new_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn deploy(env: &Env) -> RevoraRevenueShareClient<'static> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Registers an offering with an explicit supply cap and a randomly generated,
/// non-contract payout asset (so the decimals-consistency guard is skipped).
fn register(
    env: &Env,
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    supply_cap: i128,
) {
    let payout_asset = Address::generate(env);
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1,
        namespace,
        token,
        &BPS_50_PCT,
        &payout_asset,
        &supply_cap,
        &Symbol::new(env, "USD"),
        &0,
    );
}

fn ids(env: &Env) -> (Address, Symbol, Address) {
    (
        Address::generate(env),
        Symbol::new(env, "ns"),
        Address::generate(env),
    )
}

#[test]
fn unknown_offering_reads_as_zero() {
    let env = new_env();
    let client = deploy(&env);
    let (issuer, ns, token) = ids(&env);

    assert_eq!(client.get_supply_cap(&issuer, &ns, &token), 0);
}

#[test]
fn registered_cap_round_trips() {
    let env = new_env();
    let client = deploy(&env);
    let (issuer, ns, token) = ids(&env);

    register(&env, &client, &issuer, &ns, &token, 1_000);

    assert_eq!(client.get_supply_cap(&issuer, &ns, &token), 1_000);
}

#[test]
fn explicit_zero_cap_is_treated_as_no_cap() {
    let env = new_env();
    let client = deploy(&env);
    let (issuer, ns, token) = ids(&env);

    register(&env, &client, &issuer, &ns, &token, 0);

    assert_eq!(client.get_supply_cap(&issuer, &ns, &token), 0);
}

#[test]
fn cap_of_one_round_trips_exactly() {
    let env = new_env();
    let client = deploy(&env);
    let (issuer, ns, token) = ids(&env);

    register(&env, &client, &issuer, &ns, &token, 1);

    assert_eq!(client.get_supply_cap(&issuer, &ns, &token), 1);
}

#[test]
fn i128_max_cap_round_trips_without_truncation() {
    let env = new_env();
    let client = deploy(&env);
    let (issuer, ns, token) = ids(&env);

    register(&env, &client, &issuer, &ns, &token, i128::MAX);

    assert_eq!(client.get_supply_cap(&issuer, &ns, &token), i128::MAX);
}

#[test]
fn caps_are_isolated_per_token() {
    let env = new_env();
    let client = deploy(&env);
    let issuer = Address::generate(&env);
    let ns = Symbol::new(&env, "ns");
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    register(&env, &client, &issuer, &ns, &token_a, 111);
    register(&env, &client, &issuer, &ns, &token_b, 222);

    assert_eq!(client.get_supply_cap(&issuer, &ns, &token_a), 111);
    assert_eq!(client.get_supply_cap(&issuer, &ns, &token_b), 222);
}

#[test]
fn caps_are_isolated_per_namespace() {
    let env = new_env();
    let client = deploy(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns_a = Symbol::new(&env, "aaa");
    let ns_b = Symbol::new(&env, "bbb");

    register(&env, &client, &issuer, &ns_a, &token, 7);
    register(&env, &client, &issuer, &ns_b, &token, 9);

    assert_eq!(client.get_supply_cap(&issuer, &ns_a, &token), 7);
    assert_eq!(client.get_supply_cap(&issuer, &ns_b, &token), 9);
}

#[test]
fn caps_are_isolated_per_issuer() {
    let env = new_env();
    let client = deploy(&env);
    let ns = Symbol::new(&env, "ns");
    let token = Address::generate(&env);
    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);

    register(&env, &client, &issuer_a, &ns, &token, 300);
    register(&env, &client, &issuer_b, &ns, &token, 400);

    assert_eq!(client.get_supply_cap(&issuer_a, &ns, &token), 300);
    assert_eq!(client.get_supply_cap(&issuer_b, &ns, &token), 400);
}

#[test]
fn reads_are_pure_and_stable() {
    let env = new_env();
    let client = deploy(&env);
    let (issuer, ns, token) = ids(&env);
    register(&env, &client, &issuer, &ns, &token, 55);

    let first = client.get_supply_cap(&issuer, &ns, &token);
    let second = client.get_supply_cap(&issuer, &ns, &token);
    let third = client.get_supply_cap(&issuer, &ns, &token);

    assert_eq!(first, 55);
    assert_eq!(second, 55);
    assert_eq!(third, 55);
}

#[test]
fn negative_cap_is_rejected_and_leaves_state_unchanged() {
    let env = new_env();
    let client = deploy(&env);
    let (issuer, ns, token) = ids(&env);
    register(&env, &client, &issuer, &ns, &token, 100);

    let payout_asset = Address::generate(&env);
    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1,
        &ns,
        &token,
        &BPS_50_PCT,
        &payout_asset,
        &-1,
        &Symbol::new(&env, "USD"),
        &0,
    );

    assert!(result.is_err(), "negative supply cap must be rejected");
    assert_eq!(client.get_supply_cap(&issuer, &ns, &token), 100);
}

#[test]
fn re_registration_does_not_overwrite_an_existing_cap() {
    let env = new_env();
    let client = deploy(&env);
    let (issuer, ns, token) = ids(&env);

    register(&env, &client, &issuer, &ns, &token, 100);
    // Same stable identity → idempotent early return; the new cap is ignored.
    register(&env, &client, &issuer, &ns, &token, 999);

    assert_eq!(client.get_supply_cap(&issuer, &ns, &token), 100);
}

#[test]
fn rejected_registration_for_a_new_offering_leaves_no_cap() {
    let env = new_env();
    let client = deploy(&env);
    let (issuer, ns, token) = ids(&env);

    let payout_asset = Address::generate(&env);
    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1,
        &ns,
        &token,
        &BPS_50_PCT,
        &payout_asset,
        &-5,
        &Symbol::new(&env, "USD"),
        &0,
    );

    assert!(result.is_err());
    assert_eq!(client.get_supply_cap(&issuer, &ns, &token), 0);
}
