//! Focused unit tests for decimal normalisation and `compute_share`.
//!
//! Drift notes (this file was orphaned and never compiled until issue #1399):
//! - `normalize_amount` is no longer a contract entrypoint; it is a private
//!   associated fn on `RevoraRevenueShare` returning `i128` (0 on overflow)
//!   instead of `Option<i128>`. Tests call it directly. The strict variant,
//!   `normalize_amount_checked`, returns `RevoraError::ArithmeticOverflow`.
//! - `CANONICAL_PRECISION` was renamed to `STELLAR_CANONICAL_DECIMALS`.
//! - `compute_share` with `bps == 10_000` now returns the full amount for any
//!   input, including `i128::MAX` (decomposed arithmetic, see `compute_share`),
//!   where it previously saturated to 0.

use crate::{
    RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode, MAX_TOKEN_DECIMALS,
    STELLAR_CANONICAL_DECIMALS,
};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn normalize(amount: i128, decimals: u32) -> i128 {
    RevoraRevenueShare::normalize_amount(amount, decimals)
}

#[test]
fn decimals_bounds_and_default() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");
    let signers = Vec::from_array(&env, [issuer.clone()]);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token,
        &0u32,
        &token,
        &0i128,
        &symbol_short!(""),
        &0,
    );

    // Unset decimals fall back to the Stellar canonical precision.
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &token), STELLAR_CANONICAL_DECIMALS);

    assert!(client.try_set_payment_token_decimals(&issuer, &ns, &token, &0u32, &signers).is_ok());
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &token), 0u32);
    assert!(client
        .try_set_payment_token_decimals(&issuer, &ns, &token, &MAX_TOKEN_DECIMALS, &signers)
        .is_ok());
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &token), MAX_TOKEN_DECIMALS);

    let err = client
        .try_set_payment_token_decimals(&issuer, &ns, &token, &(MAX_TOKEN_DECIMALS + 1), &signers)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(err, RevoraError::LimitReached);
    // The rejected call leaves the stored precision untouched.
    assert_eq!(client.get_payment_token_decimals(&issuer, &ns, &token), MAX_TOKEN_DECIMALS);

    // An offering that was never registered also reports the canonical default.
    let env2 = Env::default();
    let c2 = make_client(&env2);
    let issuer2 = Address::generate(&env2);
    let token2 = Address::generate(&env2);
    assert_eq!(
        c2.get_payment_token_decimals(&issuer2, &symbol_short!("x"), &token2),
        STELLAR_CANONICAL_DECIMALS
    );
}

#[test]
fn normalize_and_compute_integration() {
    let env = Env::default();
    let client = make_client(&env);

    // 6-decimal: scaled up by 10.
    let n6 = normalize(1_000_000, 6);
    assert_eq!(n6, 10_000_000);
    assert_eq!(client.compute_share(&n6, &1_000u32, &RoundingMode::Truncation), 1_000_000);

    // 7-decimal: unchanged.
    let n7 = normalize(10_000_000, 7);
    assert_eq!(n7, 10_000_000);
    assert_eq!(client.compute_share(&n7, &1_000u32, &RoundingMode::Truncation), 1_000_000);

    // 8-decimal: scaled down by 10 with truncation.
    let n8 = normalize(12_345_678, 8);
    assert_eq!(n8, 1_234_567);
    assert_eq!(client.compute_share(&n8, &1_000u32, &RoundingMode::Truncation), 123_456);
    assert_eq!(client.compute_share(&n8, &1_000u32, &RoundingMode::RoundHalfUp), 123_457);
}

#[test]
fn normalize_overflow_and_zero() {
    for d in 0..=MAX_TOKEN_DECIMALS {
        assert_eq!(normalize(0, d), 0);
        assert_eq!(RevoraRevenueShare::normalize_amount_checked(0, d), Ok(0));
    }

    // Above MAX_TOKEN_DECIMALS the helper still scales down; tiny amounts truncate to 0.
    // Out-of-range precision is rejected earlier, by set_payment_token_decimals (LimitReached).
    assert_eq!(normalize(1, MAX_TOKEN_DECIMALS + 1), 0);

    // Scaling 0-decimal amounts up by 10^7 overflows past i128::MAX / 10^7.
    let multiplier: i128 = 10_000_000;
    let safe_max = i128::MAX / multiplier;
    assert_eq!(normalize(safe_max, 0), safe_max * multiplier);
    let will_overflow = safe_max + 1;
    assert_eq!(normalize(will_overflow, 0), 0);
    assert_eq!(
        RevoraRevenueShare::normalize_amount_checked(will_overflow, 0),
        Err(RevoraError::ArithmeticOverflow)
    );
}

#[test]
fn compute_share_edgecases() {
    let env = Env::default();
    let client = make_client(&env);

    // bps above 100% is rejected with a zero share.
    assert_eq!(client.compute_share(&1_000_000_i128, &10_001u32, &RoundingMode::Truncation), 0);
    // bps == 0 or amount == 0 always yields 0.
    assert_eq!(client.compute_share(&1_000_000_i128, &0u32, &RoundingMode::RoundHalfUp), 0);
    assert_eq!(client.compute_share(&0_i128, &10_000u32, &RoundingMode::RoundHalfUp), 0);

    // 100% of i128::MAX is returned exactly, without overflow (see drift note above).
    assert_eq!(client.compute_share(&i128::MAX, &10_000u32, &RoundingMode::Truncation), i128::MAX);
    // Shares are clamped to [0, amount] for positive input.
    let half = client.compute_share(&i128::MAX, &5_000u32, &RoundingMode::RoundHalfUp);
    assert!(half > 0 && half <= i128::MAX / 2 + 1);
}
