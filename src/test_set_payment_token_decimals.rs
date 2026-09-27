#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, Vec,
};

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payment_token = Address::generate(&env);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &offering_token,
        &5_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0,
    );

    (env, client, issuer, offering_token, payment_token)
}

#[test]
fn set_payment_token_decimals_happy_path() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    // Default should be 7
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &token), 7);

    // Set to 18 (max allowed)
    client.set_payment_token_decimals(&issuer, &ns, &token, &18);
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &token), 18);

    // Set to 0 (valid edge case)
    client.set_payment_token_decimals(&issuer, &ns, &token, &0);
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &token), 0);
}

#[test]
fn set_payment_token_decimals_exceeds_max_fails() {
    let (env, client, issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");

    let result = client.try_set_payment_token_decimals(&issuer, &ns, &token, &19);
    assert_eq!(result, Err(Ok(RevoraError::LimitReached)));

    // Verify state is unchanged
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &token), 7);
}

#[test]
fn set_payment_token_decimals_wrong_issuer() {
    let (env, client, _issuer, token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let attacker = Address::generate(&env);

    let result = client.try_set_payment_token_decimals(&attacker, &ns, &token, &6);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn set_payment_token_decimals_unknown_offering() {
    let (env, client, issuer, _token, _payment) = setup_offering();
    let ns = symbol_short!("ns");
    let wrong_token = Address::generate(&env);

    let result = client.try_set_payment_token_decimals(&issuer, &ns, &wrong_token, &6);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
#[ignore = "legacy host-panic auth test; Soroban aborts process in unit tests"]
fn set_payment_token_decimals_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    
    let bad_actor = Address::generate(&env);
    let issuer = bad_actor.clone();
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");

    let result = client.try_set_payment_token_decimals(&issuer, &ns, &token, &6);
    // In Soroban tests, `require_auth` with unmocked auth triggers an unrecoverable process abort,
    // so this line won't actually be reached, which is why the test is ignored.
    assert!(result.is_err());
}
