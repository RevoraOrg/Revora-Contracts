//! Adversarial coverage for [`RevoraRevenueShare::set_claim_delay`].
//!
//! The existing fixtures only exercise the happy path.  These tests pin the
//! failure and boundary behaviour that integrators depend on:
//!
//! * the default delay is `0` (immediate claim) and a configured value round-trips;
//! * the full `u64` range is accepted, including `0` and `u64::MAX`;
//! * an unknown offering / token pair is rejected with `OfferingNotFound`;
//! * configuration is isolated per offering — issuer/namespace/token is the key;
//! * a rejected call leaves the previously stored delay untouched.

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
fn set_claim_delay_defaults_to_zero_and_round_trips() {
    let (_env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    assert_eq!(client.get_claim_delay(&issuer, &ns, &token), 0);

    client.set_claim_delay(&issuer, &ns, &token, &3_600);
    assert_eq!(client.get_claim_delay(&issuer, &ns, &token), 3_600);

    // The delay is mutable: the most recent accepted value wins.
    client.set_claim_delay(&issuer, &ns, &token, &7);
    assert_eq!(client.get_claim_delay(&issuer, &ns, &token), 7);
}

#[test]
fn set_claim_delay_accepts_boundary_values() {
    let (_env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.set_claim_delay(&issuer, &ns, &token, &0);
    assert_eq!(client.get_claim_delay(&issuer, &ns, &token), 0);

    client.set_claim_delay(&issuer, &ns, &token, &u64::MAX);
    assert_eq!(client.get_claim_delay(&issuer, &ns, &token), u64::MAX);
}

#[test]
fn set_claim_delay_rejects_unknown_offering() {
    let (env, client, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);

    let result = client.try_set_claim_delay(&issuer, &ns, &unknown_token, &60);
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));
}

#[test]
fn set_claim_delay_is_isolated_per_offering() {
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

    client.set_claim_delay(&issuer_a, &ns, &token_a, &120);

    // Offering B is untouched by A's configuration.
    assert_eq!(client.get_claim_delay(&issuer_a, &ns, &token_a), 120);
    assert_eq!(client.get_claim_delay(&issuer_b, &ns, &token_b), 0);

    // Addressing A's token with B's issuer is not a valid offering key.
    let result = client.try_set_claim_delay(&issuer_b, &ns, &token_a, &9_999);
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));

    // Rejected write must not mutate A's stored delay.
    assert_eq!(client.get_claim_delay(&issuer_a, &ns, &token_a), 120);
}

#[test]
fn set_claim_delay_rejected_call_preserves_previous_value() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);

    client.set_claim_delay(&issuer, &ns, &token, &42);

    let result = client.try_set_claim_delay(&issuer, &ns, &unknown_token, &9_999);
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));

    assert_eq!(client.get_claim_delay(&issuer, &ns, &token), 42);
}
