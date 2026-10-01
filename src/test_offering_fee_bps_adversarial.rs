//! Adversarial coverage for [`RevoraRevenueShare::set_offering_fee_bps`].
//!
//! The setter stores a per-offering, per-asset fee override behind an
//! issuer/quorum auth gate. These tests pin the write-side contract beyond the
//! happy path:
//!
//! * the accepted range is `0..=MAX_PLATFORM_FEE_BPS` and the stored value
//!   round-trips through `get_offering_fee_bps`;
//! * a fee above the cap is rejected with `InvalidRevenueShareBps` and leaves
//!   the previously stored override untouched;
//! * unknown offering coordinates and a mismatched issuer are rejected with
//!   `OfferingNotFound` and create no state;
//! * a rejected write for one asset must not disturb a sibling asset or a
//!   different offering.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

/// Mirrors the contract's private `MAX_PLATFORM_FEE_BPS`.
const MAX_FEE_BPS: u32 = 5_000;

struct Ctx {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    issuer: Address,
    ns: Symbol,
    token: Address,
    asset: Address,
}

fn register_offering(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    ns: &Symbol,
    token: &Address,
) {
    let payout = Address::generate(env);
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        ns,
        token,
        &2_500,
        &payout,
        &0,
        &symbol_short!(""),
        &0u32,
    );
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let ns = symbol_short!("def");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &ns, &token);
    let asset = Address::generate(&env);

    Ctx { env, client, issuer, ns, token, asset }
}

#[test]
fn stores_valid_fee_and_accepts_the_boundary_range() {
    let ctx = setup();
    assert_eq!(ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset), 0);

    ctx.client.set_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset, &1_250).unwrap();
    assert_eq!(
        ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset),
        1_250
    );

    // Lower boundary.
    ctx.client.set_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset, &0).unwrap();
    assert_eq!(ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset), 0);

    // Upper boundary.
    ctx.client
        .set_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset, &MAX_FEE_BPS)
        .unwrap();
    assert_eq!(
        ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset),
        MAX_FEE_BPS
    );
}

#[test]
fn rejects_fee_above_cap_without_mutating_state() {
    let ctx = setup();
    ctx.client.set_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset, &2_000).unwrap();

    let over = ctx.client.try_set_offering_fee_bps(
        &ctx.issuer,
        &ctx.ns,
        &ctx.token,
        &ctx.asset,
        &(MAX_FEE_BPS + 1),
    );
    assert_eq!(over, Err(Ok(RevoraError::InvalidRevenueShareBps)));
    assert_eq!(
        ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset),
        2_000
    );

    let max_u32 = ctx.client.try_set_offering_fee_bps(
        &ctx.issuer,
        &ctx.ns,
        &ctx.token,
        &ctx.asset,
        &u32::MAX,
    );
    assert_eq!(max_u32, Err(Ok(RevoraError::InvalidRevenueShareBps)));
    assert_eq!(
        ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset),
        2_000
    );
}

#[test]
fn rejects_unknown_offering_without_creating_asset_state() {
    let ctx = setup();
    let unknown_token = Address::generate(&ctx.env);

    let result =
        ctx.client.try_set_offering_fee_bps(&ctx.issuer, &ctx.ns, &unknown_token, &ctx.asset, &100);

    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(
        ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &unknown_token, &ctx.asset),
        0
    );
}

#[test]
fn rejects_mismatched_issuer_coordinates() {
    let ctx = setup();
    let other_issuer = Address::generate(&ctx.env);

    let result =
        ctx.client.try_set_offering_fee_bps(&other_issuer, &ctx.ns, &ctx.token, &ctx.asset, &100);

    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    // The real offering's override must remain untouched.
    assert_eq!(ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset), 0);
}

#[test]
fn rejected_write_does_not_disturb_sibling_asset_or_other_offering() {
    let ctx = setup();
    let sibling_asset = Address::generate(&ctx.env);

    ctx.client.set_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset, &150).unwrap();
    ctx.client
        .set_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &sibling_asset, &250)
        .unwrap();

    let other_issuer = Address::generate(&ctx.env);
    let other_token = Address::generate(&ctx.env);
    register_offering(&ctx.env, &ctx.client, &other_issuer, &ctx.ns, &other_token);
    ctx.client
        .set_offering_fee_bps(&other_issuer, &ctx.ns, &other_token, &ctx.asset, &350)
        .unwrap();

    // Rejected over-cap write on the primary offering.
    let rejected = ctx.client.try_set_offering_fee_bps(
        &ctx.issuer,
        &ctx.ns,
        &ctx.token,
        &ctx.asset,
        &(MAX_FEE_BPS + 100),
    );
    assert_eq!(rejected, Err(Ok(RevoraError::InvalidRevenueShareBps)));

    assert_eq!(ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset), 150);
    assert_eq!(
        ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &sibling_asset),
        250
    );
    assert_eq!(
        ctx.client.get_offering_fee_bps(&other_issuer, &ctx.ns, &other_token, &ctx.asset),
        350
    );
}

#[test]
fn overrides_are_scoped_per_offering_and_per_asset() {
    let ctx = setup();
    let asset_b = Address::generate(&ctx.env);
    let issuer_b = Address::generate(&ctx.env);
    let token_b = Address::generate(&ctx.env);
    register_offering(&ctx.env, &ctx.client, &issuer_b, &ctx.ns, &token_b);

    ctx.client.set_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset, &100).unwrap();
    ctx.client.set_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &asset_b, &200).unwrap();
    ctx.client.set_offering_fee_bps(&issuer_b, &ctx.ns, &token_b, &ctx.asset, &300).unwrap();

    assert_eq!(ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &ctx.asset), 100);
    assert_eq!(ctx.client.get_offering_fee_bps(&ctx.issuer, &ctx.ns, &ctx.token, &asset_b), 200);
    assert_eq!(ctx.client.get_offering_fee_bps(&issuer_b, &ctx.ns, &token_b, &ctx.asset), 300);
    // The other offering never inherited the first one's override.
    assert_eq!(ctx.client.get_offering_fee_bps(&issuer_b, &ctx.ns, &token_b, &asset_b), 0);
}
