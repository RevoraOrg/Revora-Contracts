//! Adversarial coverage for [`RevoraRevenueShare::get_last_snapshot_ref`].
//!
//! `get_last_snapshot_ref` is the read side of the snapshot watermark.  These
//! tests pin the behaviour that consumers use to detect stale data:
//!
//! * a fresh offering reports `0`;
//! * committing a snapshot moves the watermark to exactly that ref;
//! * the watermark only moves forward, tracking the highest committed ref;
//! * the value is scoped to a single offering;
//! * an unknown offering reads as `0` instead of erroring;
//! * repeated reads are pure and deterministic.

#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, BytesN as _},
    Address, BytesN, Env, Vec,
};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &token);
    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);
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
fn last_snapshot_ref_defaults_to_zero() {
    let (_env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 0);
}

#[test]
fn last_snapshot_ref_tracks_the_highest_committed_ref() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.commit_snapshot(&issuer, &ns, &token, &3, &BytesN::random(&env));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 3);

    client.commit_snapshot(&issuer, &ns, &token, &10, &BytesN::random(&env));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 10);
}

#[test]
fn last_snapshot_ref_is_scoped_per_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    let issuer_a = Address::generate(&env);
    let token_a = Address::generate(&env);
    register_offering(&env, &client, &issuer_a, &token_a);
    client.set_snapshot_config(&issuer_a, &ns, &token_a, &true);

    let issuer_b = Address::generate(&env);
    let token_b = Address::generate(&env);
    register_offering(&env, &client, &issuer_b, &token_b);
    client.set_snapshot_config(&issuer_b, &ns, &token_b, &true);

    client.commit_snapshot(&issuer_a, &ns, &token_a, &7, &BytesN::random(&env));

    assert_eq!(client.get_last_snapshot_ref(&issuer_a, &ns, &token_a), 7);
    assert_eq!(client.get_last_snapshot_ref(&issuer_b, &ns, &token_b), 0);
}

#[test]
fn last_snapshot_ref_for_unknown_offering_reads_zero() {
    let (env, client, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);

    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &unknown_token), 0);
}

#[test]
fn last_snapshot_ref_reads_are_deterministic() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    client.commit_snapshot(&issuer, &ns, &token, &2, &BytesN::random(&env));

    let first = client.get_last_snapshot_ref(&issuer, &ns, &token);
    let second = client.get_last_snapshot_ref(&issuer, &ns, &token);
    let third = client.get_last_snapshot_ref(&issuer, &ns, &token);
    assert_eq!(first, second);
    assert_eq!(second, third);
    assert_eq!(first, 2);
}
