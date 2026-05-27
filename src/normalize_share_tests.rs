#![cfg(test)]
#![allow(warnings)]

use crate::{RevoraError, RevoraRevenueShare, RoundingMode};
use soroban_sdk::{symbol_short, testutils::Address as _, token, Address, Env, Vec};

// Focused unit tests for normalization and compute_share logic.

fn make_client(env: &Env) -> crate::RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    crate::RevoraRevenueShareClient::new(env, &id)
}

#[test]
fn decimals_bounds_and_default() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    client.register_offering(&issuer, &ns, &token, &0u32, &token, &0i128);

    assert!(client.try_set_payment_token_decimals(&issuer, &ns, &token, &0u32).is_ok());
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &token), 0u32);
    assert!(client.try_set_payment_token_decimals(&issuer, &ns, &token, &18u32).is_ok());
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &token), 18u32);
    let err = client.try_set_payment_token_decimals(&issuer, &ns, &token, &19u32).err().unwrap();
    assert_eq!(err, RevoraError::LimitReached);

    // New offering default
    let env2 = Env::default();
    let issuer2 = Address::generate(&env2);
    let token2 = Address::generate(&env2);
    let ns2 = symbol_short!("x");
    let id2 = env2.register_contract(None, RevoraRevenueShare);
    let c2 = crate::RevoraRevenueShareClient::new(&env2, &id2);
    assert_eq!(c2.get_payment_token_decimals(&issuer2, &ns2, &token2), crate::CANONICAL_PRECISION);
}

#[test]
fn normalize_and_compute_integration() {
    let env = Env::default();
    let client = make_client(&env);

    // 6-decimal
    let n6 = client.normalize_amount(&1_000_000_i128, &6u32).unwrap();
    let s6 = client.compute_share(&n6, &1_000u32, &RoundingMode::Truncation);
    assert_eq!(n6, 10_000_000);
    assert_eq!(s6, 1_000_000);

    // 7-decimal
    let n7 = client.normalize_amount(&10_000_000_i128, &7u32).unwrap();
    let s7 = client.compute_share(&n7, &1_000u32, &RoundingMode::Truncation);
    assert_eq!(n7, 10_000_000);
    assert_eq!(s7, 1_000_000);

    // 8-decimal
    let n8 = client.normalize_amount(&12_345_678_i128, &8u32).unwrap();
    let s8 = client.compute_share(&n8, &1_000u32, &RoundingMode::Truncation);
    assert_eq!(n8, 1_234_567);
    assert_eq!(s8, 123_456);
}

#[test]
fn normalize_overflow_and_zero() {
    let env = Env::default();
    let client = make_client(&env);

    for d in 0..=crate::MAX_TOKEN_DECIMALS {
        assert_eq!(client.normalize_amount(&0_i128, &d).unwrap(), 0_i128);
    }

    assert!(client.normalize_amount(&1_i128, &(crate::MAX_TOKEN_DECIMALS + 1)).is_none());

    let multiplier: i128 = 10_000_000;
    let safe_max = i128::MAX / multiplier;
    let will_overflow = safe_max.saturating_add(1);
    let res = client.normalize_amount(&will_overflow, &0u32);
    assert_eq!(res.unwrap(), 0_i128);
}

#[test]
fn compute_share_edgecases() {
    let env = Env::default();
    let client = make_client(&env);

    let s = client.compute_share(&1_000_000_i128, &10_001u32, &RoundingMode::Truncation);
    assert_eq!(s, 0_i128);

    let s_over = client.compute_share(&i128::MAX, &10_000u32, &RoundingMode::Truncation);
    assert_eq!(s_over, 0_i128);
}
