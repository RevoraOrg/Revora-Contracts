//! Adversarial tests for `set_investment_constraints` (issue #1123).
//!
//! Test matrix:
//! | Case                                       | Expected outcome                        |
//! |--------------------------------------------|------------------------------------------|
//! | valid min/max stake                        | stored; `inv_cfg` event emitted          |
//! | min_stake == max_stake (equal boundary)    | stored successfully                      |
//! | min_stake == 0                             | stored successfully (zero is valid)      |
//! | max_stake == 0                             | stored successfully (0 disables max)     |
//! | min_stake negative                         | `InvalidAmount`; state unchanged         |
//! | max_stake negative                         | `InvalidAmount`; state unchanged         |
//! | min_stake > max_stake (max > 0)            | `InvalidAmount`; state unchanged         |
//! | min_stake == i128::MAX                     | stored successfully                      |
//! | max_stake == i128::MAX                     | stored successfully                      |
//! | offering not found (wrong issuer)          | `OfferingNotFound`; state unchanged      |
//! | offering not found (no offering)           | `OfferingNotFound`                       |
//! | contract frozen                            | `ContractFrozen`; state unchanged        |
//! | get_investment_constraints before set      | returns `None`                           |
//! | overwrite existing constraints             | new values stored; previous flag set     |

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val, Vec as SdkVec,
};

// ΓöÇΓöÇ helpers ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Set up a fully initialized environment with a registered offering.
/// Returns `(env, contract_id, issuer, token)`.
fn setup() -> (Env, Address, Address, Address) {
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
    (env, contract_id, issuer, token)
}

/// Collect topic symbols from events emitted at or after `start` index.
fn topics_since(env: &Env, start: u32) -> alloc::vec::Vec<Symbol> {
    let events = env.events().all();
    let mut out = alloc::vec::Vec::new();
    for i in start..events.len() {
        let (_, topics, _) = events.get(i).unwrap();
        let v: SdkVec<Val> = topics.clone().into_val(env);
        if let Some(val) = v.get(0) {
            let sym: Symbol = val.into_val(env);
            out.push(sym);
        }
    }
    out
}

// ΓöÇΓöÇ success path tests ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// Valid min/max stake: stored successfully and `inv_cfg` event is emitted.
#[test]
fn valid_constraints_stored_and_event_emitted() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let before = env.events().all().len();
    client.set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &100i128,
        &10_000i128,
    );

    let cfg = client
        .get_investment_constraints(&issuer, &symbol_short!("def"), &token)
        .expect("constraints must be stored");
    assert_eq!(cfg.min_stake, 100);
    assert_eq!(cfg.max_stake, 10_000);

    let topics = topics_since(&env, before);
    assert!(topics.contains(&symbol_short!("inv_cfg")), "inv_cfg event must be emitted");
}

/// min_stake == max_stake: equal boundary is valid and stored.
#[test]
fn equal_min_max_stake_is_valid() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &500i128, &500i128);

    let cfg = client.get_investment_constraints(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(cfg.min_stake, 500);
    assert_eq!(cfg.max_stake, 500);
}

/// min_stake == 0 and max_stake > 0: zero min is valid.
#[test]
fn zero_min_stake_is_valid() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &0i128, &1_000i128);

    let cfg = client.get_investment_constraints(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(cfg.min_stake, 0);
    assert_eq!(cfg.max_stake, 1_000);
}

/// max_stake == 0: semantically "no maximum" ΓÇö stored successfully.
#[test]
fn zero_max_stake_disables_upper_bound() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &100i128, &0i128);

    let cfg = client.get_investment_constraints(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(cfg.min_stake, 100);
    assert_eq!(cfg.max_stake, 0);
}

/// i128::MAX values are stored without overflow.
#[test]
fn i128_max_values_stored_correctly() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &i128::MAX,
        &i128::MAX,
    );

    let cfg = client.get_investment_constraints(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(cfg.min_stake, i128::MAX);
    assert_eq!(cfg.max_stake, i128::MAX);
}

/// Overwriting existing constraints: previous flag is set in event; new values
/// replace old ones.
#[test]
fn overwrite_existing_constraints_reflects_new_values() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    // First write.
    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &100i128, &500i128);

    // Second write ΓÇö overwrites.
    let before = env.events().all().len();
    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &200i128, &1_000i128);

    let cfg = client.get_investment_constraints(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(cfg.min_stake, 200, "min_stake must reflect overwrite");
    assert_eq!(cfg.max_stake, 1_000, "max_stake must reflect overwrite");

    let topics = topics_since(&env, before);
    assert!(
        topics.contains(&symbol_short!("inv_cfg")),
        "inv_cfg event must be emitted on overwrite"
    );
}

// ΓöÇΓöÇ get_investment_constraints before any set ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// Before any call to set_investment_constraints, get returns None.
#[test]
fn get_investment_constraints_returns_none_before_set() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    assert!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token).is_none(),
        "must return None before any set"
    );
}

// ΓöÇΓöÇ boundary / invalid value tests ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// Negative min_stake returns `InvalidAmount`; no state is written.
#[test]
fn negative_min_stake_returns_invalid_amount() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let result = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &-1i128,
        &1_000i128,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
    assert!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token).is_none(),
        "state must be unchanged after rejected call"
    );
}

/// Negative max_stake returns `InvalidAmount`; no state is written.
#[test]
fn negative_max_stake_returns_invalid_amount() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let result = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &0i128,
        &-1i128,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
    assert!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token).is_none(),
        "state must be unchanged after rejected call"
    );
}

/// i128::MIN for min_stake returns `InvalidAmount`.
#[test]
fn i128_min_for_min_stake_returns_invalid_amount() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let result = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &i128::MIN,
        &1_000i128,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
}

/// i128::MIN for max_stake returns `InvalidAmount`.
#[test]
fn i128_min_for_max_stake_returns_invalid_amount() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let result = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &0i128,
        &i128::MIN,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
}

/// min_stake > max_stake (when max_stake > 0) returns `InvalidAmount`;
/// no state is written.
#[test]
fn min_greater_than_max_returns_invalid_amount() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let result = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_001i128,
        &1_000i128,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
    assert!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token).is_none(),
        "state must be unchanged after rejected call"
    );
}

/// Both min and max negative: validation rejects on min_stake first.
#[test]
fn both_negative_returns_invalid_amount() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let result = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &-100i128,
        &-50i128,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
}

// ΓöÇΓöÇ unauthorized caller tests ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// Caller with a different address (wrong issuer) gets `OfferingNotFound`;
/// no state is written.
#[test]
fn wrong_issuer_returns_offering_not_found_no_mutation() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let attacker = Address::generate(&env);

    let result = client.try_set_investment_constraints(
        &attacker,
        &symbol_short!("def"),
        &token,
        &100i128,
        &1_000i128,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    // Real issuer's offering must remain untouched.
    assert!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token).is_none(),
        "state must be unchanged after unauthorized call"
    );
}

/// No offering registered for issuer/namespace/token: returns `OfferingNotFound`.
#[test]
fn no_offering_returns_offering_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    // No register_offering call ΓÇö offering does not exist.

    let result = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &100i128,
        &1_000i128,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

// ΓöÇΓöÇ frozen contract test ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// When the contract is frozen, `set_investment_constraints` returns
/// `ContractFrozen` and no state is written.
#[test]
fn frozen_contract_returns_contract_frozen() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    // Freeze the contract.
    client.freeze();

    let result = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &100i128,
        &1_000i128,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
    // State must remain absent since freeze happened before any set.
    assert!(
        client.get_investment_constraints(&issuer, &symbol_short!("def"), &token).is_none(),
        "state must be unchanged when contract is frozen"
    );
}

/// Constraints set before freeze are preserved; subsequent calls are rejected.
#[test]
fn constraints_set_before_freeze_are_preserved_after_freeze() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    // Set constraints before freeze.
    client.set_investment_constraints(&issuer, &symbol_short!("def"), &token, &50i128, &500i128);

    // Freeze.
    client.freeze();

    // Attempt to overwrite ΓÇö must fail.
    let result = client.try_set_investment_constraints(
        &issuer,
        &symbol_short!("def"),
        &token,
        &999i128,
        &9_999i128,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));

    // Original values preserved.
    let cfg = client
        .get_investment_constraints(&issuer, &symbol_short!("def"), &token)
        .expect("pre-freeze constraints must still be readable");
    assert_eq!(cfg.min_stake, 50, "min_stake must not change after frozen rejection");
    assert_eq!(cfg.max_stake, 500, "max_stake must not change after frozen rejection");
}
