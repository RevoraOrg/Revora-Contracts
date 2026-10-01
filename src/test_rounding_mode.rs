//! Regression and adversarial coverage for `set_rounding_mode` / `get_rounding_mode`
//! (issue #1121).
//!
//! `set_rounding_mode` is a stateful, issuer-authenticated configuration entrypoint.
//! The matrix below pins its success path, its error contract, and the invariant
//! that every rejected call leaves the previously stored mode untouched.
//!
//! | Case                                   | Expected outcome                          |
//! |----------------------------------------|-------------------------------------------|
//! | never set                              | getter returns `RoundingMode::Truncation` |
//! | set `RoundHalfUp`                      | persisted, returned by getter             |
//! | set `Truncation` after `RoundHalfUp`   | overwrites, returned by getter            |
//! | successful set                         | exactly one `rnd_mode` event emitted      |
//! | unknown offering                       | `Err(OfferingNotFound)`, no mutation      |
//! | non-primary issuer                     | `Err(OfferingNotFound)`, no mutation      |
//! | rejected call after a stored mode      | previously stored mode unchanged          |
//! | globally frozen contract               | `Err(ContractFrozen)`, no mutation        |

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val, Vec as SdkVec,
};

fn setup() -> (Env, Address, Address, Address, Address) {
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
    (env, contract_id, issuer, token, payout)
}

/// Count the `rnd_mode` topic events currently recorded by the host.
fn rounding_mode_event_count(env: &Env) -> u32 {
    let events = env.events().all();
    let mut count = 0u32;
    for i in 0..events.len() {
        let (_, topics, _) = events.get(i).unwrap();
        let topic_vec: SdkVec<Val> = topics.into_val(env);
        if let Some(first) = topic_vec.get(0) {
            let topic: Symbol = first.into_val(env);
            if topic == symbol_short!("rnd_mode") {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn rounding_mode_defaults_to_truncation() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    assert_eq!(
        client.get_rounding_mode(&issuer, &symbol_short!("def"), &token),
        RoundingMode::Truncation,
    );
}

#[test]
fn set_rounding_mode_persists_and_overwrites() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    assert!(client
        .set_rounding_mode(&issuer, &ns, &token, &RoundingMode::RoundHalfUp)
        .is_ok());
    assert_eq!(client.get_rounding_mode(&issuer, &ns, &token), RoundingMode::RoundHalfUp);

    assert!(client
        .set_rounding_mode(&issuer, &ns, &token, &RoundingMode::Truncation)
        .is_ok());
    assert_eq!(client.get_rounding_mode(&issuer, &ns, &token), RoundingMode::Truncation);
}

#[test]
fn set_rounding_mode_emits_exactly_one_event() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let before = rounding_mode_event_count(&env);

    client.set_rounding_mode(&issuer, &symbol_short!("def"), &token, &RoundingMode::RoundHalfUp);

    assert_eq!(
        rounding_mode_event_count(&env),
        before + 1,
        "a successful set must emit exactly one rnd_mode event",
    );
}

#[test]
fn set_rounding_mode_unknown_offering_is_rejected_without_mutation() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let stranger = Address::generate(&env);

    let result = client.try_set_rounding_mode(
        &stranger,
        &symbol_short!("def"),
        &token,
        &RoundingMode::RoundHalfUp,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    // The real offering keeps its default mode.
    assert_eq!(
        client.get_rounding_mode(&issuer, &symbol_short!("def"), &token),
        RoundingMode::Truncation,
    );
}

#[test]
fn rejected_set_keeps_the_previously_stored_mode() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_rounding_mode(&issuer, &ns, &token, &RoundingMode::RoundHalfUp);
    assert_eq!(client.get_rounding_mode(&issuer, &ns, &token), RoundingMode::RoundHalfUp);

    // A non-primary issuer cannot change the primary issuer's mode.
    let stranger = Address::generate(&env);
    let result =
        client.try_set_rounding_mode(&stranger, &ns, &token, &RoundingMode::Truncation);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(
        client.get_rounding_mode(&issuer, &ns, &token),
        RoundingMode::RoundHalfUp,
        "rejected call must not mutate the stored mode",
    );
}

#[test]
fn frozen_contract_rejects_set_rounding_mode() {
    let (env, contract_id, issuer, token, _payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.freeze().unwrap();

    let result =
        client.try_set_rounding_mode(&issuer, &ns, &token, &RoundingMode::RoundHalfUp);
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
    assert_eq!(
        client.get_rounding_mode(&issuer, &ns, &token),
        RoundingMode::Truncation,
        "mode must stay at default after a frozen rejection",
    );
}
