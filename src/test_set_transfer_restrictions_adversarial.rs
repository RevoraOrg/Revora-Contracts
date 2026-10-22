//! Adversarial coverage for `set_transfer_restrictions` / `get_transfer_restrictions` (#1111).
//!
//! The holder cap written by `set_transfer_restrictions` was previously
//! write-only: nothing could read it back, so rejection, isolation, and
//! boundary behaviour could not be asserted. `get_transfer_restrictions` is the
//! read-only accessor that makes those invariants observable.
//!
//! Cases covered:
//! - lifecycle: unset -> set -> read back, and a second set overwrites;
//! - isolation: caps are scoped per category and per offering (`issuer`,
//!   `namespace`, `token`);
//! - boundaries: `0` and `u32::MAX` are stored verbatim;
//! - freeze: a frozen contract rejects the write and leaves the slot unset;
//! - authorization: an unauthorized write is rejected and not persisted.
#![cfg(test)]

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

const CAP_SMALL: u32 = 3;

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);

    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );

    (env, client, issuer, token, payout)
}

#[test]
fn getter_is_none_before_set_and_round_trips() {
    let (_env, client, issuer, token, _payout) = setup();
    let ns = symbol_short!("def");
    let cat = symbol_short!("RegD");

    assert_eq!(
        client.get_transfer_restrictions(&issuer, &ns, &token, &cat),
        None
    );

    client.set_transfer_restrictions(&issuer, &ns, &token, &cat, &CAP_SMALL);

    assert_eq!(
        client.get_transfer_restrictions(&issuer, &ns, &token, &cat),
        Some(TransferRestrictions {
            category: cat.clone(),
            max_holders: CAP_SMALL,
        }),
    );
}

#[test]
fn set_overwrites_the_previous_cap() {
    let (_env, client, issuer, token, _payout) = setup();
    let ns = symbol_short!("def");
    let cat = symbol_short!("RegD");

    client.set_transfer_restrictions(&issuer, &ns, &token, &cat, &1);
    client.set_transfer_restrictions(&issuer, &ns, &token, &cat, &9);

    assert_eq!(
        client
            .get_transfer_restrictions(&issuer, &ns, &token, &cat)
            .map(|r| r.max_holders),
        Some(9),
    );
}

#[test]
fn caps_are_isolated_per_category() {
    let (_env, client, issuer, token, _payout) = setup();
    let ns = symbol_short!("def");
    let a = symbol_short!("RegA");
    let b = symbol_short!("RegB");

    client.set_transfer_restrictions(&issuer, &ns, &token, &a, &1);
    client.set_transfer_restrictions(&issuer, &ns, &token, &b, &2);

    assert_eq!(
        client
            .get_transfer_restrictions(&issuer, &ns, &token, &a)
            .unwrap()
            .max_holders,
        1
    );
    assert_eq!(
        client
            .get_transfer_restrictions(&issuer, &ns, &token, &b)
            .unwrap()
            .max_holders,
        2
    );
    // A category that was never set stays unset.
    let c = symbol_short!("RegC");
    assert_eq!(
        client.get_transfer_restrictions(&issuer, &ns, &token, &c),
        None
    );
}

#[test]
fn caps_are_isolated_per_offering() {
    let (env, client, issuer, token, _payout) = setup();
    let ns = symbol_short!("def");
    let cat = symbol_short!("RegD");
    let other_token = Address::generate(&env);
    let other_ns = symbol_short!("other");

    client.set_transfer_restrictions(&issuer, &ns, &token, &cat, &5);

    // A different token (same namespace/category) does not inherit the cap.
    assert_eq!(
        client.get_transfer_restrictions(&issuer, &ns, &other_token, &cat),
        None
    );
    // A different namespace (same token/category) does not inherit it either.
    assert_eq!(
        client.get_transfer_restrictions(&issuer, &other_ns, &token, &cat),
        None
    );

    // Setting the cap on the other offering leaves the first untouched.
    client.set_transfer_restrictions(&issuer, &ns, &other_token, &cat, &7);
    assert_eq!(
        client
            .get_transfer_restrictions(&issuer, &ns, &token, &cat)
            .unwrap()
            .max_holders,
        5
    );
    assert_eq!(
        client
            .get_transfer_restrictions(&issuer, &ns, &other_token, &cat)
            .unwrap()
            .max_holders,
        7
    );
}

#[test]
fn boundary_caps_are_stored_verbatim() {
    let (_env, client, issuer, token, _payout) = setup();
    let ns = symbol_short!("def");
    let zero = symbol_short!("Zero");
    let max = symbol_short!("Max");

    client.set_transfer_restrictions(&issuer, &ns, &token, &zero, &0);
    client.set_transfer_restrictions(&issuer, &ns, &token, &max, &u32::MAX);

    assert_eq!(
        client
            .get_transfer_restrictions(&issuer, &ns, &token, &zero)
            .unwrap()
            .max_holders,
        0
    );
    assert_eq!(
        client
            .get_transfer_restrictions(&issuer, &ns, &token, &max)
            .unwrap()
            .max_holders,
        u32::MAX
    );
}

#[test]
fn frozen_contract_rejects_write_and_leaves_slot_unset() {
    let (_env, client, issuer, token, _payout) = setup();
    let ns = symbol_short!("def");
    let cat = symbol_short!("RegD");

    client.freeze();
    let result = client.try_set_transfer_restrictions(&issuer, &ns, &token, &cat, &1);

    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
    // The rejected write must not have persisted anything.
    assert_eq!(
        client.get_transfer_restrictions(&issuer, &ns, &token, &cat),
        None
    );
}

#[test]
fn unauthorized_write_is_rejected_and_not_persisted() {
    // Fresh environment with auth mocking deliberately OFF so `require_auth`
    // on the issuer is not satisfied.
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    // `initialize` is not auth-gated.
    client.initialize(&issuer, &None::<Address>, &None::<bool>);

    let ns = symbol_short!("def");
    let token = Address::generate(&env);
    let cat = symbol_short!("RegD");

    let result = client.try_set_transfer_restrictions(&issuer, &ns, &token, &cat, &1);
    assert!(result.is_err(), "write without issuer auth must be rejected");
    assert_eq!(
        client.get_transfer_restrictions(&issuer, &ns, &token, &cat),
        None
    );
}
