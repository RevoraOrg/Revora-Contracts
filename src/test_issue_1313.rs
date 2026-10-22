#![cfg(test)]
//! # Issue #1313 — claim (and other outflows) respect HardPaused and the global freeze.
//!
//! `HardPaused` blocks every state-mutating outflow including `claim`;
//! `SoftPaused` still permits `claim`. A global freeze blocks all outflows.

use crate::{PauseState, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
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
/// Returns `(client, admin, safety, issuer, offering_token, payment_token, holder)`.
fn setup_with_offering(
    env: &Env,
) -> (
    RevoraRevenueShareClient<'static>,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
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
    client.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder, &10_000u32, &1u64);

    token::StellarAssetClient::new(env, &payment_token.address()).mint(&issuer, &500_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token.address(),
        &100_000i128,
        &1u64,
    );

    (
        client,
        admin,
        safety,
        issuer,
        offering_token,
        payment_token.address(),
        holder,
    )
}

fn freeze(client: &RevoraRevenueShareClient<'static>) {
    // `freeze` is the global freeze entrypoint (admin-auth required; mocked in tests).
    client.freeze();
}

// ── Section A: HardPaused blocks claim ───

#[test]
fn hard_pause_claim_blocked() {
    let env = Env::default();
    let (client, admin, _safety, issuer, offering_token, _payment_token, holder) =
        setup_with_offering(&env);

    client.hard_pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);

    let result = client.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert_eq!(
        result,
        Err(Ok(RevoraError::ContractPaused)),
        "claim must be blocked under HardPaused"
    );

    // No token movement: holder balance unchanged.
    let holder_bal = token::StellarAssetClient::new(&env, &_payment_token).balance(&holder);
    assert_eq!(holder_bal, 0, "no tokens must move while hard-paused");
}

#[test]
fn soft_pause_claim_succeeds() {
    let env = Env::default();
    let (client, admin, _safety, issuer, offering_token, _payment_token, holder) =
        setup_with_offering(&env);

    client.pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    let result = client.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert!(result.is_ok(), "claim must succeed under SoftPaused, got {result:?}");
    assert_eq!(result.unwrap().unwrap(), 100_000, "holder should receive full payout");
}

// ── Section B: global freeze blocks claim ───

#[test]
fn global_freeze_claim_blocked() {
    let env = Env::default();
    let (client, admin, _safety, issuer, offering_token, _payment_token, holder) =
        setup_with_offering(&env);

    freeze(&client);

    let result = client.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert_eq!(
        result,
        Err(Ok(RevoraError::ContractFrozen)),
        "claim must be blocked when globally frozen"
    );
}

// ── Section C: fulfill_redemption respects hard pause + freeze ───

#[test]
fn fulfill_redemption_blocked_when_hard_paused() {
    let env = Env::default();
    let (client, admin, _safety, issuer, offering_token, _payment_token, holder) =
        setup_with_offering(&env);

    client.hard_pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);

    let result = client.try_fulfill_redemption(&issuer, &symbol_short!("def"), &offering_token, &holder, &1_000i128);
    assert_eq!(
        result,
        Err(Ok(RevoraError::ContractPaused)),
        "fulfill_redemption must be blocked under HardPaused"
    );
}

#[test]
fn fulfill_redemption_blocked_when_frozen() {
    let env = Env::default();
    let (client, admin, _safety, issuer, offering_token, _payment_token, holder) =
        setup_with_offering(&env);

    freeze(&client);

    let result = client.try_fulfill_redemption(&issuer, &symbol_short!("def"), &offering_token, &holder, &1_000i128);
    assert_eq!(
        result,
        Err(Ok(RevoraError::ContractFrozen)),
        "fulfill_redemption must be blocked when globally frozen"
    );
}
