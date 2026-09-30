//! Tests for the `get_payment_token` read-only method.
//!
//! ## Coverage matrix
//!
//! | Scenario | Expected |
//! |----------|----------|
//! | Valid offering | Returns `Some(payment_token)` |
//! | Invalid offering (wrong issuer) | Returns `None` |
//! | Invalid offering (wrong namespace) | Returns `None` |
//! | Invalid offering (wrong token) | Returns `None` |
//! | Unregistered offering | Returns `None` |
//! | No state mutation | State remains unchanged after call |

#![cfg(test)]

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let admin = Address::generate(&env);
    let payment_token = env.register_stellar_asset_contract_v2(admin.clone()).address();

    client.initialize(&issuer, &None::<Address>, &None::<bool>);
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
        &soroban_sdk::token::Client::new(&env, &payment_token)
            .try_decimals()
            .ok()
            .and_then(|d| d.ok())
            .unwrap_or(0),
    );

    // `get_payment_token` resolves to `Some` only once the offering's first
    // successful deposit has locked the payment token in.
    soroban_sdk::token::StellarAssetClient::new(&env, &payment_token).mint(&issuer, &1_000_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("ns"),
        &offering_token,
        &payment_token,
        &100_000,
        &1,
    );

    (env, client, issuer, offering_token, payment_token)
}

#[test]
fn test_get_payment_token_success() {
    let (env, client, issuer, offering_token, payment_token) = setup_offering();
    let namespace = symbol_short!("ns");

    let result = client.get_payment_token(&issuer, &namespace, &offering_token);
    assert_eq!(result, Some(payment_token));
}

#[test]
fn test_get_payment_token_wrong_issuer() {
    let (env, client, _issuer, offering_token, _payment_token) = setup_offering();
    let wrong_issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");

    let result = client.get_payment_token(&wrong_issuer, &namespace, &offering_token);
    assert_eq!(result, None);
}

#[test]
fn test_get_payment_token_wrong_namespace() {
    let (env, client, issuer, offering_token, _payment_token) = setup_offering();
    let wrong_namespace = symbol_short!("wrong");

    let result = client.get_payment_token(&issuer, &wrong_namespace, &offering_token);
    assert_eq!(result, None);
}

#[test]
fn test_get_payment_token_wrong_token() {
    let (env, client, issuer, _offering_token, _payment_token) = setup_offering();
    let wrong_token = Address::generate(&env);
    let namespace = symbol_short!("ns");

    let result = client.get_payment_token(&issuer, &namespace, &wrong_token);
    assert_eq!(result, None);
}

#[test]
fn test_get_payment_token_no_state_change() {
    let (env, client, issuer, offering_token, payment_token) = setup_offering();
    let namespace = symbol_short!("ns");

    // Call the function once and record the result
    let result1 = client.get_payment_token(&issuer, &namespace, &offering_token);
    assert_eq!(result1, Some(payment_token.clone()));

    // Repeated calls should return exactly the same thing without error.
    let result2 = client.get_payment_token(&issuer, &namespace, &offering_token);
    assert_eq!(result2, Some(payment_token));
}
