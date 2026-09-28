//! Focused adversarial coverage for `set_max_total_supply_shares` (#1033).
//!
//! Entry point under test:
//! `set_max_total_supply_shares(env, issuer, namespace, token, max_total_supply_shares)`
//!
//! Behaviour pinned by these tests:
//! - an unset cap reads back as `0` (which the contract treats as "unlimited");
//! - a positive cap is stored and read back;
//! - setting `0` removes the stored cap, returning the offering to unlimited;
//! - a negative cap is rejected with `InvalidAmount` and does not mutate state;
//! - caps are scoped per offering, so writing one does not leak to another;
//! - an unauthorized caller is rejected and state is unchanged.

#![cfg(test)]

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

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
        &10_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );
    (env, contract_id, issuer, token, payout)
}

#[test]
fn defaults_to_unlimited_when_unset() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    assert_eq!(client.get_max_total_supply_shares(&issuer, &symbol_short!("def"), &token), 0);
}

#[test]
fn stores_reads_and_clears_the_cap() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_max_total_supply_shares(&issuer, &ns, &token, &5_000);
    assert_eq!(client.get_max_total_supply_shares(&issuer, &ns, &token), 5_000);

    // `0` clears the cap, which restores unlimited supply.
    client.set_max_total_supply_shares(&issuer, &ns, &token, &0);
    assert_eq!(client.get_max_total_supply_shares(&issuer, &ns, &token), 0);
}

#[test]
fn negative_cap_is_rejected_and_state_is_unchanged() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.set_max_total_supply_shares(&issuer, &ns, &token, &5_000);

    for invalid in [-1_i128, i128::MIN] {
        let result = client.try_set_max_total_supply_shares(&issuer, &ns, &token, &invalid);
        assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));
        assert_eq!(
            client.get_max_total_supply_shares(&issuer, &ns, &token),
            5_000,
            "a rejected write must not change the stored cap",
        );
    }
}

#[test]
fn accepts_i128_max_as_a_hard_cap() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    client.set_max_total_supply_shares(&issuer, &ns, &token, &i128::MAX);
    assert_eq!(client.get_max_total_supply_shares(&issuer, &ns, &token), i128::MAX);
}

#[test]
fn caps_are_isolated_per_offering() {
    let (env, contract_id, issuer, token_a, payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let token_b = Address::generate(&env);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &2u32,
        &ns,
        &token_b,
        &10_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );

    client.set_max_total_supply_shares(&issuer, &ns, &token_a, &5_000);

    assert_eq!(client.get_max_total_supply_shares(&issuer, &ns, &token_a), 5_000);
    assert_eq!(
        client.get_max_total_supply_shares(&issuer, &ns, &token_b),
        0,
        "setting a cap on one offering must not affect another",
    );
}

#[test]
fn unauthorized_caller_is_rejected_and_state_is_unchanged() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    // Drop all mocked authorizations: `issuer.require_auth()` must now fail.
    env.mock_auths(&[]);
    let result = client.try_set_max_total_supply_shares(&issuer, &ns, &token, &5_000);
    assert!(result.is_err(), "unauthorized cap write must be rejected");
    assert_eq!(client.get_max_total_supply_shares(&issuer, &ns, &token), 0);
}
