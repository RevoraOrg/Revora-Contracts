//! Adversarial coverage for `get_jurisdiction_grace_period` (#1161).
//!
//! The per-offering jurisdiction migration grace period defines how long a holder
//! may keep claiming after relocating into a potentially disallowed jurisdiction.
//! `get_jurisdiction_grace_period` is the read path that holders, indexers and the
//! off-chain compliance tooling use to reason about that deadline, so its
//! default-value contract, its boundaries and the "rejected configuration leaves
//! state untouched" property are pinned here.
//!
//! Coverage matrix
//!
//! | Scenario                                                | Expected                                        |
//! |---------------------------------------------------------|-------------------------------------------------|
//! | Offering never configured                               | `DEFAULT_JURISDICTION_GRACE_SECS` (7 days)       |
//! | Unknown offering identity                               | default (pure read, no panic)                    |
//! | `MIN_JURISDICTION_GRACE_SECS` (1 hour)                  | accepted, read back verbatim                     |
//! | `MAX_JURISDICTION_GRACE_SECS` (90 days)                 | accepted, read back verbatim                     |
//! | One second below the minimum                            | `InvalidAmount`, value unchanged                 |
//! | One second above the maximum                            | `InvalidAmount`, value unchanged                 |
//! | Zero                                                    | `InvalidAmount`, value unchanged                 |
//! | Unregistered offering                                   | `OfferingNotFound`, nothing written              |
//! | Sibling offering / different namespace                  | keeps its own value (no cross-offering leakage)  |

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

/// `MIN_JURISDICTION_GRACE_SECS` in `src/lib.rs`.
const MIN_GRACE: u64 = 60 * 60;
/// `MAX_JURISDICTION_GRACE_SECS` in `src/lib.rs`.
const MAX_GRACE: u64 = 90 * 24 * 60 * 60;
/// `DEFAULT_JURISDICTION_GRACE_SECS` in `src/lib.rs`.
const DEFAULT_GRACE: u64 = 7 * 24 * 60 * 60;

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0u32,
    );

    (env, client, issuer, token)
}

#[test]
fn unconfigured_offering_returns_the_seven_day_default() {
    let (_env, client, issuer, token) = setup_offering();

    assert_eq!(
        client.get_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token),
        DEFAULT_GRACE
    );
}

#[test]
fn unknown_offering_identity_returns_the_default_without_panicking() {
    let (env, client, _issuer, token) = setup_offering();
    let unknown_issuer = Address::generate(&env);

    assert_eq!(
        client.get_jurisdiction_grace_period(&unknown_issuer, &symbol_short!("def"), &token),
        DEFAULT_GRACE
    );
    assert_eq!(
        client.get_jurisdiction_grace_period(&unknown_issuer, &symbol_short!("other"), &token),
        DEFAULT_GRACE
    );
}

#[test]
fn minimum_boundary_is_accepted_and_read_back() {
    let (_env, client, issuer, token) = setup_offering();

    client
        .set_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token, &MIN_GRACE)
        .unwrap();

    assert_eq!(
        client.get_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token),
        MIN_GRACE
    );
}

#[test]
fn maximum_boundary_is_accepted_and_read_back() {
    let (_env, client, issuer, token) = setup_offering();

    client
        .set_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token, &MAX_GRACE)
        .unwrap();

    assert_eq!(
        client.get_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token),
        MAX_GRACE
    );
}

#[test]
fn below_minimum_is_rejected_and_the_configured_value_is_unchanged() {
    let (_env, client, issuer, token) = setup_offering();

    client
        .set_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token, &MIN_GRACE)
        .unwrap();

    assert_eq!(
        client.try_set_jurisdiction_grace_period(
            &issuer,
            &symbol_short!("def"),
            &token,
            &(MIN_GRACE - 1)
        ),
        Err(Ok(RevoraError::InvalidAmount)),
        "the minimum bound is inclusive, so one second below must be refused"
    );

    assert_eq!(
        client.get_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token),
        MIN_GRACE,
        "a rejected configuration must not move the stored value"
    );
}

#[test]
fn above_maximum_is_rejected_and_the_configured_value_is_unchanged() {
    let (_env, client, issuer, token) = setup_offering();

    client
        .set_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token, &MIN_GRACE)
        .unwrap();

    assert_eq!(
        client.try_set_jurisdiction_grace_period(
            &issuer,
            &symbol_short!("def"),
            &token,
            &(MAX_GRACE + 1)
        ),
        Err(Ok(RevoraError::InvalidAmount)),
        "the maximum bound is inclusive, so one second above must be refused"
    );

    assert_eq!(
        client.get_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token),
        MIN_GRACE
    );
}

#[test]
fn zero_is_rejected_and_the_default_is_preserved() {
    let (_env, client, issuer, token) = setup_offering();

    assert_eq!(
        client.try_set_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token, &0u64),
        Err(Ok(RevoraError::InvalidAmount))
    );

    assert_eq!(
        client.get_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token),
        DEFAULT_GRACE,
        "a zero-length grace window must never be stored"
    );
}

#[test]
fn unregistered_offering_is_rejected_without_writing_any_value() {
    let (env, client, _issuer, token) = setup_offering();
    let unknown_issuer = Address::generate(&env);

    assert_eq!(
        client.try_set_jurisdiction_grace_period(
            &unknown_issuer,
            &symbol_short!("def"),
            &token,
            &MIN_GRACE
        ),
        Err(Ok(RevoraError::OfferingNotFound))
    );

    assert_eq!(
        client.get_jurisdiction_grace_period(&unknown_issuer, &symbol_short!("def"), &token),
        DEFAULT_GRACE
    );
}

#[test]
fn configured_value_is_scoped_to_the_exact_offering() {
    let (env, client, issuer, token) = setup_offering();

    // Sibling offering: same issuer + namespace, different token.
    let token_b = Address::generate(&env);
    let payout_b = Address::generate(&env);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token_b,
        &5_000,
        &payout_b,
        &0,
        &symbol_short!(""),
        &0u32,
    );

    client
        .set_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token, &MIN_GRACE)
        .unwrap();

    assert_eq!(
        client.get_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token),
        MIN_GRACE
    );
    assert_eq!(
        client.get_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token_b),
        DEFAULT_GRACE,
        "configuring one offering must not leak into a sibling offering"
    );
    assert_eq!(
        client.get_jurisdiction_grace_period(&issuer, &symbol_short!("other"), &token),
        DEFAULT_GRACE,
        "configuring one namespace must not leak into another namespace"
    );
}

#[test]
fn reconfiguring_replaces_the_previous_value() {
    let (_env, client, issuer, token) = setup_offering();

    client
        .set_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token, &MIN_GRACE)
        .unwrap();
    client
        .set_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token, &MAX_GRACE)
        .unwrap();

    assert_eq!(
        client.get_jurisdiction_grace_period(&issuer, &symbol_short!("def"), &token),
        MAX_GRACE,
        "the last accepted configuration wins"
    );
}
