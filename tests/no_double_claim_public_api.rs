//! Regression test for issue #1308.
//!
//! The test-only helpers `test_insert_period` and `test_set_last_claimed_idx`
//! let a caller rewrite an offering's period revenue and a holder's claim
//! cursor with no authorization and no token transfer. They are now gated
//! behind `#[cfg(any(test, feature = "testutils"))]`, so a normal build (which
//! is what this integration test links against — no `testutils` feature) cannot
//! see them at all.
//!
//! This test drives the public client API end to end and documents that a
//! holder cannot be paid twice: the only way to advance `LastClaimedIdx` is a
//! successful `claim`, and a second `claim` finds nothing pending. If the
//! gating regresses, the helpers would become callable from a production client
//! and this invariant would no longer hold.

use revora_contracts::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payment_admin = Address::generate(&env);
    let payment_token = env.register_stellar_asset_contract_v2(payment_admin.clone()).address();

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &offering_token,
        &5_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        // A Stellar Asset Contract reports 7 decimals, and register_offering
        // rejects a mismatch (RevoraError::DecimalsMismatch).
        &7u32,
    );

    // Fund the issuer so they can deposit real revenue into the offering.
    StellarAssetClient::new(&env, &payment_token).mint(&issuer, &10_000_000);

    (env, client, issuer, offering_token, payment_token)
}

#[test]
fn a_holder_cannot_be_paid_twice_through_the_public_api() {
    let (env, client, issuer, token, payment_token) = setup();
    let holder = Address::generate(&env);
    let ns = symbol_short!("def");

    client.set_holder_share(&issuer, &ns, &token, &holder, &10_000, &1);

    // Deposit one period of real revenue (tokens actually move into the
    // contract — unlike the gated test helper).
    client.deposit_revenue(&issuer, &ns, &token, &payment_token, &100_000, &1);

    let balance_before = TokenClient::new(&env, &payment_token).balance(&holder);
    let paid = client.claim(&holder, &issuer, &ns, &token, &0);
    assert_eq!(paid, 100_000, "the holder should receive the full period payout");
    assert_eq!(TokenClient::new(&env, &payment_token).balance(&holder), balance_before + 100_000,);

    // The holder's claim cursor has advanced past the only deposited period, so
    // a second claim is rejected and pays nothing. There is no client-reachable
    // entrypoint that can rewind `LastClaimedIdx`.
    let second = client.try_claim(&holder, &issuer, &ns, &token, &0);
    assert_eq!(second, Err(Ok(RevoraError::NoPendingClaims)));
    assert_eq!(
        TokenClient::new(&env, &payment_token).balance(&holder),
        balance_before + 100_000,
        "a second claim must not pay the holder twice",
    );
}
