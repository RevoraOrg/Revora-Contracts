//! Adversarial coverage for `propose_issuer_transfer` (#1056).
//!
//! `propose_issuer_transfer` writes a pending-transfer record that later lets
//! `accept_issuer_transfer` hand an offering to a new issuer. Existing coverage
//! only uses it as setup for unrelated offering-pagination tests, so this suite
//! pins the state, authorization and boundary behaviour:
//!
//! - the happy path records the new issuer, the ledger timestamp and the
//!   `0` "use the default window" expiry sentinel,
//! - a second proposal for the same offering is rejected and must not overwrite
//!   the first one,
//! - unknown offerings and non-primary issuers are rejected without creating any
//!   pending state,
//! - `propose_transfer_with_expiry` clamps to the protocol window instead of
//!   accepting arbitrary values, and
//! - proposals are scoped per offering, and `replace_issuer_transfer` is the
//!   documented way to change an existing proposal.

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec as SdkVec,
};

const NS: Symbol = symbol_short!("def");

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);

    (env, contract_id, issuer)
}

fn register(env: &Env, contract_id: &Address, issuer: &Address, token: &Address) {
    let client = RevoraRevenueShareClient::new(env, contract_id);
    let payout_asset = Address::generate(env);
    client.register_offering(
        issuer,
        &SdkVec::new(env),
        &1u32,
        &NS,
        token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
}

fn pending(env: &Env, contract_id: &Address, issuer: &Address, token: &Address) -> Option<Address> {
    RevoraRevenueShareClient::new(env, contract_id).get_pending_issuer_transfer(issuer, &NS, token)
}

fn pending_details(
    env: &Env,
    contract_id: &Address,
    issuer: &Address,
    token: &Address,
) -> PendingTransfer {
    RevoraRevenueShareClient::new(env, contract_id)
        .get_pending_transfer_details(issuer, &NS, token)
        .expect("a pending transfer should have been recorded")
}

#[test]
fn proposal_records_new_issuer_timestamp_and_default_expiry_sentinel() {
    let (env, contract_id, issuer) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &token);

    let new_issuer = Address::generate(&env);
    env.ledger().set_timestamp(1_700_000_000);

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    assert_eq!(pending(&env, &contract_id, &issuer, &token), Some(new_issuer.clone()));

    let details = pending_details(&env, &contract_id, &issuer, &token);
    assert_eq!(details.new_issuer, new_issuer);
    assert_eq!(details.timestamp, 1_700_000_000);
    assert_eq!(
        details.expiry_secs, 0,
        "0 is the documented sentinel for the default 7-day window"
    );
}

#[test]
fn duplicate_proposal_is_rejected_and_keeps_the_first_proposal() {
    let (env, contract_id, issuer) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &token);

    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.propose_issuer_transfer(&issuer, &NS, &token, &first);

    let result = client.try_propose_issuer_transfer(&issuer, &NS, &token, &second);

    assert!(result.is_err(), "a pending transfer must block a second proposal");
    assert_eq!(
        pending(&env, &contract_id, &issuer, &token),
        Some(first),
        "the rejected proposal must not overwrite the pending new issuer"
    );
}

#[test]
fn unknown_offering_is_rejected_without_creating_pending_state() {
    let (env, contract_id, issuer) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let unknown_token = Address::generate(&env);
    let new_issuer = Address::generate(&env);

    let result = client.try_propose_issuer_transfer(&issuer, &NS, &unknown_token, &new_issuer);

    assert!(result.is_err(), "an unregistered offering must be rejected");
    assert_eq!(pending(&env, &contract_id, &issuer, &unknown_token), None);
}

#[test]
fn non_primary_issuer_is_rejected_without_creating_pending_state() {
    let (env, contract_id, issuer) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &token);

    let impostor = Address::generate(&env);
    let new_issuer = Address::generate(&env);

    let result = client.try_propose_issuer_transfer(&impostor, &NS, &token, &new_issuer);

    assert!(result.is_err(), "only the offering's primary issuer may propose");
    assert_eq!(pending(&env, &contract_id, &issuer, &token), None);
    assert_eq!(pending(&env, &contract_id, &impostor, &token), None);
}

#[test]
fn custom_expiry_is_clamped_to_the_protocol_window() {
    let (env, contract_id, issuer) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let new_issuer = Address::generate(&env);

    let below = Address::generate(&env);
    let inside = Address::generate(&env);
    let above = Address::generate(&env);
    let zero = Address::generate(&env);

    for token in [below.clone(), inside.clone(), above.clone(), zero.clone()] {
        register(&env, &contract_id, &issuer, &token);
    }

    client.propose_transfer_with_expiry(&issuer, &NS, &below, &new_issuer, 1);
    assert_eq!(
        pending_details(&env, &contract_id, &issuer, &below).expiry_secs,
        MIN_ISSUER_TRANSFER_EXPIRY_SECS,
        "an expiry below the minimum must be clamped up"
    );

    client.propose_transfer_with_expiry(&issuer, &NS, &inside, &new_issuer, 7_200);
    assert_eq!(
        pending_details(&env, &contract_id, &issuer, &inside).expiry_secs,
        7_200,
        "an in-range expiry must be preserved verbatim"
    );

    client.propose_transfer_with_expiry(&issuer, &NS, &above, &new_issuer, u64::MAX);
    assert_eq!(
        pending_details(&env, &contract_id, &issuer, &above).expiry_secs,
        MAX_ISSUER_TRANSFER_EXPIRY_SECS,
        "an expiry above the maximum must be clamped down"
    );

    client.propose_transfer_with_expiry(&issuer, &NS, &zero, &new_issuer, 0);
    assert_eq!(
        pending_details(&env, &contract_id, &issuer, &zero).expiry_secs,
        0,
        "0 must stay the default-window sentinel"
    );
}

#[test]
fn proposals_are_scoped_to_a_single_offering() {
    let (env, contract_id, issuer) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    register(&env, &contract_id, &issuer, &token_a);
    register(&env, &contract_id, &issuer, &token_b);

    let new_issuer = Address::generate(&env);
    client.propose_issuer_transfer(&issuer, &NS, &token_a, &new_issuer);

    assert_eq!(pending(&env, &contract_id, &issuer, &token_a), Some(new_issuer));
    assert_eq!(
        pending(&env, &contract_id, &issuer, &token_b),
        None,
        "a proposal for one token must not leak into another offering"
    );
}

#[test]
fn replace_issuer_transfer_is_the_supported_way_to_change_a_proposal() {
    let (env, contract_id, issuer) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &token);

    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.propose_issuer_transfer(&issuer, &NS, &token, &first);
    client.replace_issuer_transfer(&issuer, &NS, &token, &second);

    assert_eq!(pending(&env, &contract_id, &issuer, &token), Some(second.clone()));
    assert_eq!(pending_details(&env, &contract_id, &issuer, &token).new_issuer, second);

    // Replacing without a pending proposal must fail and create nothing new.
    let fresh_token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &fresh_token);
    let result = client.try_replace_issuer_transfer(&issuer, &NS, &fresh_token, &first);

    assert!(result.is_err(), "replace requires an existing pending proposal");
    assert_eq!(pending(&env, &contract_id, &issuer, &fresh_token), None);
}

#[test]
fn frozen_contract_blocks_proposals_without_recording_state() {
    let (env, contract_id, issuer) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &token);

    let new_issuer = Address::generate(&env);
    assert!(client.freeze().is_ok());

    let result = client.try_propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    assert!(result.is_err(), "a frozen contract must reject the proposal");
    assert_eq!(pending(&env, &contract_id, &issuer, &token), None);
}
