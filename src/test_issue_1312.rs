#![cfg(test)]
//! # Issue #1312 — Claim pays historical periods at the share held in each period.
//!
//! Claim must walk the per-holder `HolderShareSchedule` (shared helper with
//! `get_claimable`) instead of multiplying every unclaimed period by the
//! holder's current `share_bps`.

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, token, Address, Env};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'static> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn setup(env: &Env) -> (RevoraRevenueShareClient<'static>, Address, Address) {
    env.mock_all_auths();
    let client = make_client(env);
    let admin = Address::generate(env);
    let safety = Address::generate(env);
    client.initialize(&admin, &Some(safety.clone()), &None::<bool>);
    (client, admin, safety)
}

/// Offering + holder with a real Stellar asset so `claim` can transfer tokens.
/// Returns `(client, issuer, offering_token, payment_token, holder)`.
fn setup_with_offering(env: &Env) -> (RevoraRevenueShareClient<'static>, Address, Address, Address, Address) {
    env.mock_all_auths();
    let client = make_client(env);
    let admin = Address::generate(env);
    let safety = Address::generate(env);
    client.initialize(&admin, &Some(safety.clone()), &None::<bool>);

    let issuer = Address::generate(env);
    let offering_token = Address::generate(env);
    let payment_admin = Address::generate(env);
    let payment_token = env.register_stellar_asset_contract_v2(payment_admin.clone());
    let holder = Address::generate(env);

    client.register_offering(
        &issuer,
        &soroban_sdk::Vec::new(env),
        &1u32,
        &symbol_short!("def"),
        &offering_token,
        &10_000u32,
        &payment_token.address(),
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    (client, issuer, offering_token, payment_token.address(), holder)
}

fn deposit(env: &Env, client: &RevoraRevenueShareClient<'static>, issuer: &Address, namespace: &soroban_sdk::Symbol, offering_token: &Address, payment_token: &Address, amount: i128, period_id: u64) {
    token::StellarAssetClient::new(env, payment_token).mint(issuer, amount);
    client.deposit_revenue(issuer, namespace, offering_token, payment_token, &amount, &period_id);
}

fn set_share(
    env: &Env,
    client: &RevoraRevenueShareClient<'static>,
    issuer: &Address,
    namespace: &soroban_sdk::Symbol,
    offering_token: &Address,
    holder: &Address,
    share_bps: u32,
    nonce: u64,
) {
    client.set_holder_share(issuer, namespace, offering_token, holder, &share_bps, &nonce);
}

// ── Section A: share raised after deposit → old periods paid at old share ───

/// Holder (sole owner) at 1_000 bps; deposit period 1 (100_000); issuer raises
/// holder to 10_000 bps AFTER the deposit. Period 1 must be paid at the share
/// in effect during period 1. As the sole owner the holder receives the full
/// 100_000 for that period; the later share raise must not inflate it.
#[test]
fn claim_pays_old_periods_at_old_share_when_share_raised() {
    let env = Env::default();
    let (client, issuer, offering_token, payment_token, holder) = setup_with_offering(&env);
    let namespace = symbol_short!("def");

    set_share(&env, &client, &issuer, &namespace, &offering_token, &holder, 1_000, 1);
    deposit(&env, &client, &issuer, &namespace, &offering_token, &payment_token, 100_000, 1);
    // Raise share AFTER the deposit.
    set_share(&env, &client, &issuer, &namespace, &offering_token, &holder, 10_000, 2);

    let result = client.try_claim(&holder, &issuer, &namespace, &offering_token, &50);
    assert!(result.is_ok(), "claim must succeed, got {result:?}");
    assert_eq!(result.unwrap().unwrap(), 100_000, "period 1 must be paid at the share in effect during period 1");
}

// ── Section B: share lowered to 0 after deposit → still claim periods earned before ──

/// Holder at 10_000 bps; deposit period 1 (100_000); issuer lowers holder to 0
/// bps AFTER the deposit. Claim must still pay period 1 at the share in effect
/// during that period (the holder earned it before the share was removed).
#[test]
fn claim_still_pays_periods_earned_before_share_lowered_to_zero() {
    let env = Env::default();
    let (client, issuer, offering_token, payment_token, holder) = setup_with_offering(&env);
    let namespace = symbol_short!("def");

    set_share(&env, &client, &issuer, &namespace, &offering_token, &holder, 10_000, 1);
    deposit(&env, &client, &issuer, &namespace, &offering_token, &payment_token, 100_000, 1);
    // Lower share to 0 AFTER the deposit.
    set_share(&env, &client, &issuer, &namespace, &offering_token, &holder, 0, 2);

    let result = client.try_claim(&holder, &issuer, &namespace, &offering_token, &50);
    assert!(result.is_ok(), "claim must succeed even with current share 0, got {result:?}");
    assert_eq!(result.unwrap().unwrap(), 100_000, "period earned before share removal must still be paid");
}

// ── Section C: claim == get_claimable parity ───

/// Claim result equals the prior get_claimable preview for the same holder.
#[test]
fn claim_matches_get_claimable() {
    let env = Env::default();
    let (client, issuer, offering_token, payment_token, holder) = setup_with_offering(&env);
    let namespace = symbol_short!("def");

    set_share(&env, &client, &issuer, &namespace, &offering_token, &holder, 3_000, 1);
    deposit(&env, &client, &issuer, &namespace, &offering_token, &payment_token, 100_000, 1);
    // Change share mid-way so the schedule has multiple checkpoints.
    set_share(&env, &client, &issuer, &namespace, &offering_token, &holder, 7_000, 2);
    deposit(&env, &client, &issuer, &namespace, &offering_token, &payment_token, 200_000, 2);

    let preview = client.get_claimable(&issuer, &namespace, &offering_token, &holder);
    let result = client.try_claim(&holder, &issuer, &namespace, &offering_token, &50);
    assert!(result.is_ok(), "claim must succeed, got {result:?}");
    assert_eq!(
        result.unwrap().unwrap(),
        preview,
        "claim result must equal get_claimable preview"
    );
}

/// After a successful claim, get_claimable returns 0 (nothing left to claim).
#[test]
fn claim_consumes_claimable_so_get_claimable_is_zero() {
    let env = Env::default();
    let (client, issuer, offering_token, payment_token, holder) = setup_with_offering(&env);
    let namespace = symbol_short!("def");

    set_share(&env, &client, &issuer, &namespace, &offering_token, &holder, 5_000, 1);
    deposit(&env, &client, &issuer, &namespace, &offering_token, &payment_token, 100_000, 1);

    let _ = client.try_claim(&holder, &issuer, &namespace, &offering_token, &50);
    let after = client.get_claimable(&issuer, &namespace, &offering_token, &holder);
    assert_eq!(after, 0, "get_claimable must be 0 after a full claim");
}
