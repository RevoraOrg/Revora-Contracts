//! Focused adversarial coverage for `atomic_swap` (#1045).
//!
//! Entry point under test:
//! `atomic_swap(env, issuer, namespace, token, seller, buyer, amount_bps,
//!              category, payment_asset, payment_amount)`
//!
//! `atomic_swap` is a three-party secondary-market settlement: it routes an
//! optional royalty from buyer to issuer, pays the remainder to the seller, and
//! moves `amount_bps` of shares from seller to buyer. These tests pin the happy
//! path plus the adversarial paths that must *not* mutate balances or holder
//! shares:
//! - non-positive `payment_amount` is rejected with `InvalidAmount`;
//! - a zero share amount is rejected;
//! - a buyer that cannot cover the payment fails with `TransferFailed`;
//! - dropping all authorizations fails the swap;
//! - a successful swap emits `swap_v1`.

#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val,
};

/// (env, contract_id, issuer, seller, buyer, share_token, payment_asset)
fn setup() -> (Env, Address, Address, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = admin.clone();
    let seller = Address::generate(&env);
    let buyer = Address::generate(&env);
    let share_token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &share_token,
        &10_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &share_token, &seller, &5_000, &1);

    let payment_asset = crate::test_utils::create_token(&env, &admin);
    crate::test_utils::mint_tokens(&env, &payment_asset, &buyer, 10_000);

    (env, contract_id, issuer, seller, buyer, share_token, payment_asset)
}

fn emits_swap_v1(env: &Env) -> bool {
    env.events().all().iter().any(|event| {
        let topics: soroban_sdk::Vec<Val> = event.1.clone().into_val(env);
        let sym: Symbol = topics.get(0).unwrap().into_val(env);
        sym == symbol_short!("swap_v1")
    })
}

#[test]
fn swap_moves_payment_and_shares_without_royalty() {
    let (env, contract_id, issuer, seller, buyer, share_token, payment_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    client.atomic_swap(
        &issuer,
        &ns,
        &share_token,
        &seller,
        &buyer,
        &100,
        &symbol_short!("A"),
        &payment_asset,
        &1_000,
    );

    // No royalty configured: the buyer pays the seller in full.
    assert_eq!(crate::test_utils::get_balance(&env, &payment_asset, &issuer), 0);
    assert_eq!(crate::test_utils::get_balance(&env, &payment_asset, &seller), 1_000);
    assert_eq!(crate::test_utils::get_balance(&env, &payment_asset, &buyer), 9_000);

    // Shares move from seller to buyer.
    assert_eq!(client.get_holder_share(&issuer, &ns, &share_token, &seller), 4_900);
    assert_eq!(client.get_holder_share(&issuer, &ns, &share_token, &buyer), 100);

    assert!(emits_swap_v1(&env), "successful swap must emit swap_v1");
}

#[test]
fn non_positive_payment_amount_is_rejected_without_state_change() {
    let (env, contract_id, issuer, seller, buyer, share_token, payment_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    for invalid in [0_i128, -1, i128::MIN] {
        let result = client.try_atomic_swap(
            &issuer,
            &ns,
            &share_token,
            &seller,
            &buyer,
            &100,
            &symbol_short!("A"),
            &payment_asset,
            &invalid,
        );
        assert_eq!(
            result,
            Err(Ok(RevoraError::InvalidAmount)),
            "payment_amount = {} must be rejected",
            invalid,
        );

        // No balances and no shares moved.
        assert_eq!(crate::test_utils::get_balance(&env, &payment_asset, &issuer), 0);
        assert_eq!(crate::test_utils::get_balance(&env, &payment_asset, &seller), 0);
        assert_eq!(crate::test_utils::get_balance(&env, &payment_asset, &buyer), 10_000);
        assert_eq!(client.get_holder_share(&issuer, &ns, &share_token, &seller), 5_000);
        assert_eq!(client.get_holder_share(&issuer, &ns, &share_token, &buyer), 0);
    }
}

#[test]
fn zero_share_amount_is_rejected_without_state_change() {
    let (env, contract_id, issuer, seller, buyer, share_token, payment_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    let result = client.try_atomic_swap(
        &issuer,
        &ns,
        &share_token,
        &seller,
        &buyer,
        &0,
        &symbol_short!("A"),
        &payment_asset,
        &1_000,
    );
    assert!(result.is_err(), "a zero share transfer must be rejected");

    assert_eq!(crate::test_utils::get_balance(&env, &payment_asset, &seller), 0);
    assert_eq!(crate::test_utils::get_balance(&env, &payment_asset, &buyer), 10_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &share_token, &seller), 5_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &share_token, &buyer), 0);
}

#[test]
fn underfunded_buyer_fails_the_swap() {
    let (env, contract_id, issuer, seller, buyer, share_token, payment_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    // Buyer only holds 10_000, so a 20_000 payment cannot settle.
    let result = client.try_atomic_swap(
        &issuer,
        &ns,
        &share_token,
        &seller,
        &buyer,
        &100,
        &symbol_short!("A"),
        &payment_asset,
        &20_000,
    );
    assert_eq!(result, Err(Ok(RevoraError::TransferFailed)));

    assert_eq!(crate::test_utils::get_balance(&env, &payment_asset, &seller), 0);
    assert_eq!(crate::test_utils::get_balance(&env, &payment_asset, &buyer), 10_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &share_token, &seller), 5_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &share_token, &buyer), 0);
}

#[test]
fn all_parties_must_authorize() {
    let (env, contract_id, issuer, seller, buyer, share_token, payment_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    // Drop mocked auths: issuer/seller/buyer no longer authorize the swap.
    env.mock_auths(&[]);
    let result = client.try_atomic_swap(
        &issuer,
        &ns,
        &share_token,
        &seller,
        &buyer,
        &100,
        &symbol_short!("A"),
        &payment_asset,
        &1_000,
    );
    assert!(result.is_err(), "swap without authorizations must be rejected");
    assert_eq!(client.get_holder_share(&issuer, &ns, &share_token, &seller), 5_000);
    assert_eq!(client.get_holder_share(&issuer, &ns, &share_token, &buyer), 0);
}
