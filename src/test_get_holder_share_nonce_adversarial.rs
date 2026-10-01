//! Adversarial coverage for [`RevoraRevenueShare::get_holder_share_nonce`].
//!
//! `get_holder_share_nonce` is the read side of the `set_holder_share`
//! replay-protection guard.  These tests pin the behaviour that off-chain
//! orchestrators rely on:
//!
//! * the nonce starts at `0` for a holder that has never been written;
//! * an accepted `set_holder_share` advances the nonce to exactly that value;
//! * replaying / regressing the nonce is rejected with `StaleNonce` and the
//!   stored nonce is left unchanged;
//! * a write rejected before the nonce guard (invalid bps) does not advance it;
//! * nonces are tracked independently per holder;
//! * an unknown offering reads as `0` rather than erroring.

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
fn nonce_defaults_to_zero_before_any_write() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    assert_eq!(client.get_holder_share_nonce(&issuer, &ns, &token, &holder), 0);
}

#[test]
fn accepted_set_holder_share_advances_nonce() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &2_500, &5);

    assert_eq!(client.get_holder_share_nonce(&issuer, &ns, &token, &holder), 5);
}

#[test]
fn replayed_and_regressing_nonces_are_rejected_without_advancing() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder, &2_500, &5);

    // Exact replay of the last accepted nonce.
    let replay = client.try_set_holder_share(&issuer, &ns, &token, &holder, &2_500, &5);
    assert!(matches!(replay.err(), Some(Ok(RevoraError::StaleNonce))));
    assert_eq!(client.get_holder_share_nonce(&issuer, &ns, &token, &holder), 5);

    // Regression to an older nonce.
    let regress = client.try_set_holder_share(&issuer, &ns, &token, &holder, &2_500, &4);
    assert!(matches!(regress.err(), Some(Ok(RevoraError::StaleNonce))));
    assert_eq!(client.get_holder_share_nonce(&issuer, &ns, &token, &holder), 5);

    // A strictly greater nonce is accepted and becomes the new watermark.
    client.set_holder_share(&issuer, &ns, &token, &holder, &2_500, &6);
    assert_eq!(client.get_holder_share_nonce(&issuer, &ns, &token, &holder), 6);
}

#[test]
fn write_rejected_before_nonce_guard_does_not_advance_nonce() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    // Invalid bps is rejected by the input validation that runs before the
    // nonce guard, so the nonce must remain at its default.
    let result = client.try_set_holder_share(&issuer, &ns, &token, &holder, &10_001, &10);
    assert!(matches!(result.err(), Some(Ok(RevoraError::InvalidShareBps))));
    assert_eq!(client.get_holder_share_nonce(&issuer, &ns, &token, &holder), 0);
}

#[test]
fn nonces_are_tracked_independently_per_holder() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);

    client.set_holder_share(&issuer, &ns, &token, &holder_a, &2_000, &3);
    client.set_holder_share(&issuer, &ns, &token, &holder_b, &1_000, &1);

    assert_eq!(client.get_holder_share_nonce(&issuer, &ns, &token, &holder_a), 3);
    assert_eq!(client.get_holder_share_nonce(&issuer, &ns, &token, &holder_b), 1);
}

#[test]
fn nonce_for_unknown_offering_reads_zero() {
    let (env, client, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);
    let holder = Address::generate(&env);

    assert_eq!(client.get_holder_share_nonce(&issuer, &ns, &unknown_token, &holder), 0);
}
