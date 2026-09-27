//! Adversarial coverage for [`RevoraRevenueShare::set_secondary_market_royalty_bps`]
//! and its read-side counterpart
//! [`RevoraRevenueShare::get_secondary_market_royalty_bps`].
//!
//! The write side is issuer-gated and per-`(offering, asset)` scoped, while the
//! read side is an unauthenticated `0`-defaulting getter.  These tests pin that
//! contract:
//!
//! * an unconfigured offering/asset reads back as `0`;
//! * the getter never fails for unknown coordinates (it returns `0` instead);
//! * the inclusive maximum (`MAX_PLATFORM_FEE_BPS`, 5 000) is accepted and one
//!   basis point above it is rejected with `InvalidRevenueShareBps`;
//! * a rejected call leaves any previously stored royalty untouched;
//! * an unknown offering, or a caller whose coordinates do not resolve to the
//!   current issuer, is rejected with `OfferingNotFound`;
//! * royalty configuration is isolated per asset and per offering.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

/// Inclusive upper bound enforced by `set_secondary_market_royalty_bps`.
const MAX_BPS: u32 = 5_000;

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

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &token);
    (env, client, issuer, token)
}

#[test]
fn defaults_to_zero_for_unconfigured_offering_and_asset() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let asset = Address::generate(&env);

    assert_eq!(client.get_secondary_market_royalty_bps(&issuer, &ns, &token, &asset), 0);
}

#[test]
fn accepts_valid_value_and_inclusive_maximum_boundary() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let asset = Address::generate(&env);

    client.set_secondary_market_royalty_bps(&issuer, &ns, &token, &asset, &200);
    assert_eq!(client.get_secondary_market_royalty_bps(&issuer, &ns, &token, &asset), 200);

    // Exactly the maximum is allowed.
    client.set_secondary_market_royalty_bps(&issuer, &ns, &token, &asset, &MAX_BPS);
    assert_eq!(client.get_secondary_market_royalty_bps(&issuer, &ns, &token, &asset), MAX_BPS);
}

#[test]
fn rejects_value_above_maximum_and_preserves_previous_state() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let asset = Address::generate(&env);

    client.set_secondary_market_royalty_bps(&issuer, &ns, &token, &asset, &200);

    let result =
        client.try_set_secondary_market_royalty_bps(&issuer, &ns, &token, &asset, &(MAX_BPS + 1));
    assert!(matches!(result.err(), Some(Ok(RevoraError::InvalidRevenueShareBps))));

    // Rejected write must not have mutated the stored value.
    assert_eq!(client.get_secondary_market_royalty_bps(&issuer, &ns, &token, &asset), 200);
}

#[test]
fn zero_disables_a_previously_configured_royalty() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let asset = Address::generate(&env);

    client.set_secondary_market_royalty_bps(&issuer, &ns, &token, &asset, &400);
    client.set_secondary_market_royalty_bps(&issuer, &ns, &token, &asset, &0);
    assert_eq!(client.get_secondary_market_royalty_bps(&issuer, &ns, &token, &asset), 0);
}

#[test]
fn rejects_unknown_offering_while_getter_stays_at_zero() {
    let (env, client, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);
    let asset = Address::generate(&env);

    let result =
        client.try_set_secondary_market_royalty_bps(&issuer, &ns, &unknown_token, &asset, &100);
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));

    // The read-only getter does not validate coordinates; it still returns its default.
    assert_eq!(client.get_secondary_market_royalty_bps(&issuer, &ns, &unknown_token, &asset), 0);
}

#[test]
fn rejects_mismatched_issuer_coordinates_without_writing() {
    let (env, client, _issuer, token) = setup();
    let ns = symbol_short!("def");
    let asset = Address::generate(&env);
    let impostor = Address::generate(&env);

    let result = client.try_set_secondary_market_royalty_bps(&impostor, &ns, &token, &asset, &100);
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));

    assert_eq!(client.get_secondary_market_royalty_bps(&impostor, &ns, &token, &asset), 0);
}

#[test]
fn scopes_royalty_per_asset() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let asset_a = Address::generate(&env);
    let asset_b = Address::generate(&env);

    client.set_secondary_market_royalty_bps(&issuer, &ns, &token, &asset_a, &350);

    assert_eq!(client.get_secondary_market_royalty_bps(&issuer, &ns, &token, &asset_a), 350);
    // A sibling asset for the same offering is untouched.
    assert_eq!(client.get_secondary_market_royalty_bps(&issuer, &ns, &token, &asset_b), 0);
}

#[test]
fn scopes_royalty_per_offering() {
    let (env, client, issuer_a, token_a) = setup();
    let ns = symbol_short!("def");
    let asset = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_b = Address::generate(&env);
    register_offering(&env, &client, &issuer_b, &token_b);

    client.set_secondary_market_royalty_bps(&issuer_a, &ns, &token_a, &asset, &900);

    assert_eq!(client.get_secondary_market_royalty_bps(&issuer_a, &ns, &token_a, &asset), 900);
    assert_eq!(client.get_secondary_market_royalty_bps(&issuer_b, &ns, &token_b, &asset), 0);
}
