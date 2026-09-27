//! Adversarial coverage for [`RevoraRevenueShare::commit_snapshot`].
//!
//! `commit_snapshot` is a write-once, monotonically increasing record of the
//! canonical holder dataset.  These tests pin the guard rails that protect the
//! monotonicity invariant:
//!
//! * a disabled snapshot configuration is rejected with `SnapshotNotEnabled`;
//! * an unknown offering is rejected with `OfferingNotFound`;
//! * the happy path stores the entry and advances the last-ref pointer;
//! * replaying an existing ref, or submitting a lower / zero ref, is rejected
//!   with `OutdatedSnapshot` and leaves state untouched;
//! * the caller-supplied `content_hash` is stored verbatim.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, SnapshotEntry};
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

fn enable_snapshots(client: &RevoraRevenueShareClient, issuer: &Address, token: &Address) {
    client.set_snapshot_config(issuer, &symbol_short!("def"), token, &true);
}

#[test]
fn commit_snapshot_rejected_when_feature_disabled() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let hash = BytesN::random(&env);

    let result = client.try_commit_snapshot(&issuer, &ns, &token, &1, &hash);
    assert!(matches!(result.err(), Some(Ok(RevoraError::SnapshotNotEnabled))));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 0);
}

#[test]
fn commit_snapshot_rejects_unknown_offering() {
    let (env, client, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);
    let hash = BytesN::random(&env);

    enable_snapshots(&client, &issuer, &unknown_token);

    let result = client.try_commit_snapshot(&issuer, &ns, &unknown_token, &1, &hash);
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));
}

#[test]
fn commit_snapshot_stores_entry_and_advances_last_ref() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let hash = BytesN::random(&env);

    enable_snapshots(&client, &issuer, &token);
    client.commit_snapshot(&issuer, &ns, &token, &1, &hash);

    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 1);

    let stored: Option<SnapshotEntry> = client.get_snapshot_entry(&issuer, &ns, &token, &1);
    let stored = stored.expect("committed entry must be readable");
    assert_eq!(stored.snapshot_ref, 1);
    assert_eq!(stored.content_hash, hash);
    assert_eq!(stored.holder_count, 0);
    assert_eq!(stored.total_bps, 0);
}

#[test]
fn commit_snapshot_enforces_strict_monotonicity() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    enable_snapshots(&client, &issuer, &token);
    client.commit_snapshot(&issuer, &ns, &token, &5, &BytesN::random(&env));

    // Replaying the current ref.
    let replay = client.try_commit_snapshot(&issuer, &ns, &token, &5, &BytesN::random(&env));
    assert!(matches!(replay.err(), Some(Ok(RevoraError::OutdatedSnapshot))));

    // Regressing to a lower ref.
    let regress = client.try_commit_snapshot(&issuer, &ns, &token, &4, &BytesN::random(&env));
    assert!(matches!(regress.err(), Some(Ok(RevoraError::OutdatedSnapshot))));

    // Zero is never strictly greater than the initial pointer.
    let zero = client.try_commit_snapshot(&issuer, &ns, &token, &0, &BytesN::random(&env));
    assert!(matches!(zero.err(), Some(Ok(RevoraError::OutdatedSnapshot))));

    // Rejected writes leave the watermark untouched.
    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 5);

    // A strictly greater ref is accepted.
    client.commit_snapshot(&issuer, &ns, &token, &6, &BytesN::random(&env));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 6);
}

#[test]
fn commit_snapshot_rejected_call_leaves_state_unchanged() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let first_hash = BytesN::random(&env);

    enable_snapshots(&client, &issuer, &token);
    client.commit_snapshot(&issuer, &ns, &token, &2, &first_hash);

    let replay = client.try_commit_snapshot(&issuer, &ns, &token, &2, &BytesN::random(&env));
    assert!(matches!(replay.err(), Some(Ok(RevoraError::OutdatedSnapshot))));

    let stored = client
        .get_snapshot_entry(&issuer, &ns, &token, &2)
        .expect("original entry must survive the rejected replay");
    assert_eq!(stored.content_hash, first_hash);
    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 2);
}
