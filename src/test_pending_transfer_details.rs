//! Adversarial coverage for `get_pending_transfer_details` (#1055).
//!
//! `get_pending_transfer_details` is the read-only accessor that returns the full
//! `PendingTransfer` record (proposed new issuer, proposal timestamp and the
//! effective expiry window) for one offering, and `None` for any offering that
//! has no pending transfer.
//!
//! Coverage matrix
//!
//! | Scenario                                                   | Expected                                   |
//! |------------------------------------------------------------|--------------------------------------------|
//! | No transfer ever proposed                                  | `None`                                      |
//! | After `propose_issuer_transfer` (default window)           | `Some` with proposed issuer + timestamp     |
//! | Default `expiry_secs` sentinel                             | `0` (callers resolve to 7 days)             |
//! | Query for a different token / namespace / issuer           | `None` (no cross-offering leakage)          |
//! | Custom `expiry_secs` below `MIN`                           | clamped up to `MIN`                          |
//! | Custom `expiry_secs` above `MAX`                           | clamped down to `MAX`                        |
//! | Duplicate proposal rejected                                | `IssuerTransferPending`, record unchanged    |
//! | `replace_issuer_transfer`                                  | issuer + timestamp updated, window kept     |
//! | `cancel_issuer_transfer`                                   | accessor back to `None`                      |

#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env,
};

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0u32,
    );

    (env, client, issuer, token)
}

#[test]
fn accessor_is_none_before_any_proposal() {
    let (_env, client, issuer, token) = setup_offering();

    assert!(client.get_pending_transfer_details(&issuer, &symbol_short!("def"), &token).is_none());
    assert!(client.get_pending_issuer_transfer(&issuer, &symbol_short!("def"), &token).is_none());
}

#[test]
fn accessor_exposes_full_record_with_default_expiry_sentinel() {
    let (env, client, issuer, token) = setup_offering();
    let new_issuer = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_234_567);
    client.propose_issuer_transfer(&issuer, &symbol_short!("def"), &token, &new_issuer).unwrap();

    let details = client
        .get_pending_transfer_details(&issuer, &symbol_short!("def"), &token)
        .expect("pending transfer details must be present after a proposal");
    assert_eq!(details.new_issuer, new_issuer);
    assert_eq!(details.timestamp, 1_234_567, "proposal timestamp must be the ledger time");
    assert_eq!(details.expiry_secs, 0, "0 is the sentinel for the 7-day default window");

    // The lightweight accessor must agree with the richer one.
    assert_eq!(
        client.get_pending_issuer_transfer(&issuer, &symbol_short!("def"), &token),
        Some(new_issuer)
    );
}

#[test]
fn accessor_is_scoped_to_the_exact_offering_identity() {
    let (env, client, issuer, token) = setup_offering();

    // Register a second offering (same issuer + namespace, different token).
    let token_b = Address::generate(&env);
    let payout_b = Address::generate(&env);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token_b,
        &1_000,
        &payout_b,
        &0,
        &symbol_short!(""),
        &0u32,
    );

    let new_issuer = Address::generate(&env);
    client.propose_issuer_transfer(&issuer, &symbol_short!("def"), &token, &new_issuer).unwrap();

    assert!(client.get_pending_transfer_details(&issuer, &symbol_short!("def"), &token).is_some());
    assert!(
        client.get_pending_transfer_details(&issuer, &symbol_short!("def"), &token_b).is_none(),
        "a sibling offering (different token) must not inherit the pending transfer"
    );
    assert!(
        client.get_pending_transfer_details(&issuer, &symbol_short!("other"), &token).is_none(),
        "a different namespace must not inherit the pending transfer"
    );

    let other_issuer = Address::generate(&env);
    assert!(
        client.get_pending_transfer_details(&other_issuer, &symbol_short!("def"), &token).is_none(),
        "a different issuer must not inherit the pending transfer"
    );
}

#[test]
fn custom_expiry_below_minimum_is_clamped_up() {
    let (env, client, issuer, token) = setup_offering();
    let new_issuer = Address::generate(&env);

    client
        .propose_transfer_with_expiry(&issuer, &symbol_short!("def"), &token, &new_issuer, &1u64)
        .unwrap();

    let details =
        client.get_pending_transfer_details(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(details.expiry_secs, 60 * 60, "expiry must be clamped up to MIN (1 hour)");
}

#[test]
fn custom_expiry_above_maximum_is_clamped_down() {
    let (env, client, issuer, token) = setup_offering();
    let new_issuer = Address::generate(&env);

    client
        .propose_transfer_with_expiry(
            &issuer,
            &symbol_short!("def"),
            &token,
            &new_issuer,
            &(100 * 24 * 60 * 60u64),
        )
        .unwrap();

    let details =
        client.get_pending_transfer_details(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(
        details.expiry_secs,
        30 * 24 * 60 * 60,
        "expiry must be clamped down to MAX (30 days)"
    );
}

#[test]
fn rejected_duplicate_proposal_leaves_the_record_unchanged() {
    let (env, client, issuer, token) = setup_offering();
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 100);
    client.propose_issuer_transfer(&issuer, &symbol_short!("def"), &token, &first).unwrap();

    let duplicate =
        client.try_propose_issuer_transfer(&issuer, &symbol_short!("def"), &token, &second);
    assert_eq!(duplicate, Err(Ok(RevoraError::IssuerTransferPending)));

    let after =
        client.get_pending_transfer_details(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(after.new_issuer, first, "the rejected proposal must not overwrite the pending one");
    assert_eq!(after.timestamp, 100);
}

#[test]
fn replacement_updates_proposal_and_preserves_expiry_window() {
    let (env, client, issuer, token) = setup_offering();
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 100);
    client
        .propose_transfer_with_expiry(
            &issuer,
            &symbol_short!("def"),
            &token,
            &first,
            &(2 * 60 * 60),
        )
        .unwrap();

    env.ledger().with_mut(|l| l.timestamp = 250);
    client.replace_issuer_transfer(&issuer, &symbol_short!("def"), &token, &second).unwrap();

    let replaced =
        client.get_pending_transfer_details(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(replaced.new_issuer, second);
    assert_eq!(replaced.timestamp, 250);
    assert_eq!(
        replaced.expiry_secs,
        2 * 60 * 60,
        "replacement must inherit the original expiry window"
    );
}

#[test]
fn cancel_clears_the_accessor() {
    let (env, client, issuer, token) = setup_offering();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&issuer, &symbol_short!("def"), &token, &new_issuer).unwrap();
    assert!(client.get_pending_transfer_details(&issuer, &symbol_short!("def"), &token).is_some());

    client.cancel_issuer_transfer(&issuer, &symbol_short!("def"), &token).unwrap();
    assert!(client.get_pending_transfer_details(&issuer, &symbol_short!("def"), &token).is_none());
}
