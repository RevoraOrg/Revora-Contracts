//! Adversarial coverage for [`RevoraRevenueShare::set_fx_oracle`] and its
//! read-side counterpart [`RevoraRevenueShare::get_fx_oracle`].
//!
//! The setter is issuer-gated and blocked while the contract is frozen or
//! paused; the getter is an unauthenticated `Option`-returning read.  These
//! tests pin that contract:
//!
//! * an unconfigured offering reads back as `None`;
//! * a stored configuration round-trips field-for-field, including the two
//!   `max_oracle_age_secs` boundaries (`0` and `u64::MAX`);
//! * re-configuring replaces the previous value in place;
//! * unknown or mismatched coordinates are rejected with `OfferingNotFound`;
//! * a frozen or paused contract rejects the write and preserves the previously
//!   stored configuration;
//! * configuration is isolated per offering.

#![cfg(test)]

use crate::{FxOracleConfig, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

fn register_offering(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
) {
    let payout_asset = Address::generate(env);
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        &symbol_short!("def"),
        token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
}

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &token);
    (env, client, admin, issuer, token)
}

#[test]
fn defaults_to_none_for_unconfigured_offering() {
    let (_env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");

    assert_eq!(client.get_fx_oracle(&issuer, &ns, &token), None);
}

#[test]
fn stores_configuration_and_reads_it_back() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let oracle = Address::generate(&env);

    client.set_fx_oracle(
        &issuer,
        &ns,
        &token,
        &oracle,
        &symbol_short!("USD"),
        &symbol_short!("XLM"),
        &300,
    );

    assert_eq!(
        client.get_fx_oracle(&issuer, &ns, &token),
        Some(FxOracleConfig {
            oracle,
            revenue_symbol: symbol_short!("USD"),
            payout_symbol: symbol_short!("XLM"),
            max_oracle_age_secs: 300,
        })
    );
}

#[test]
fn accepts_upper_and_lower_max_age_boundaries() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let oracle = Address::generate(&env);

    // A zero staleness window is allowed (always-fresh policy is a caller decision).
    client.set_fx_oracle(
        &issuer,
        &ns,
        &token,
        &oracle,
        &symbol_short!("USD"),
        &symbol_short!("XLM"),
        &0,
    );
    assert_eq!(
        client.get_fx_oracle(&issuer, &ns, &token),
        Some(FxOracleConfig {
            oracle: oracle.clone(),
            revenue_symbol: symbol_short!("USD"),
            payout_symbol: symbol_short!("XLM"),
            max_oracle_age_secs: 0,
        })
    );

    // The maximum representable window is accepted without arithmetic overflow.
    client.set_fx_oracle(
        &issuer,
        &ns,
        &token,
        &oracle,
        &symbol_short!("USD"),
        &symbol_short!("XLM"),
        &u64::MAX,
    );
    assert_eq!(
        client.get_fx_oracle(&issuer, &ns, &token),
        Some(FxOracleConfig {
            oracle,
            revenue_symbol: symbol_short!("USD"),
            payout_symbol: symbol_short!("XLM"),
            max_oracle_age_secs: u64::MAX,
        })
    );
}

#[test]
fn reconfiguration_replaces_previous_configuration() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.set_fx_oracle(
        &issuer,
        &ns,
        &token,
        &first,
        &symbol_short!("USD"),
        &symbol_short!("XLM"),
        &60,
    );
    client.set_fx_oracle(
        &issuer,
        &ns,
        &token,
        &second,
        &symbol_short!("EUR"),
        &symbol_short!("XLM"),
        &120,
    );

    assert_eq!(
        client.get_fx_oracle(&issuer, &ns, &token),
        Some(FxOracleConfig {
            oracle: second,
            revenue_symbol: symbol_short!("EUR"),
            payout_symbol: symbol_short!("XLM"),
            max_oracle_age_secs: 120,
        })
    );
}

#[test]
fn rejects_unknown_offering_without_creating_configuration() {
    let (env, client, _admin, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);
    let oracle = Address::generate(&env);

    let result = client.try_set_fx_oracle(
        &issuer,
        &ns,
        &unknown_token,
        &oracle,
        &symbol_short!("USD"),
        &symbol_short!("XLM"),
        &60,
    );
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));
    assert_eq!(client.get_fx_oracle(&issuer, &ns, &unknown_token), None);
}

#[test]
fn rejects_mismatched_issuer_coordinates() {
    let (env, client, _admin, _issuer, token) = setup();
    let ns = symbol_short!("def");
    let oracle = Address::generate(&env);
    let impostor = Address::generate(&env);

    let result = client.try_set_fx_oracle(
        &impostor,
        &ns,
        &token,
        &oracle,
        &symbol_short!("USD"),
        &symbol_short!("XLM"),
        &60,
    );
    assert!(matches!(result.err(), Some(Ok(RevoraError::OfferingNotFound))));
    assert_eq!(client.get_fx_oracle(&impostor, &ns, &token), None);
}

#[test]
fn frozen_contract_rejects_write_and_preserves_existing_configuration() {
    let (env, client, _admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let oracle = Address::generate(&env);
    let replacement = Address::generate(&env);

    client.set_fx_oracle(
        &issuer,
        &ns,
        &token,
        &oracle,
        &symbol_short!("USD"),
        &symbol_short!("XLM"),
        &60,
    );

    client.freeze();

    let result = client.try_set_fx_oracle(
        &issuer,
        &ns,
        &token,
        &replacement,
        &symbol_short!("EUR"),
        &symbol_short!("XLM"),
        &120,
    );
    assert!(matches!(result.err(), Some(Ok(RevoraError::ContractFrozen))));

    // Still the pre-freeze configuration.
    assert_eq!(
        client.get_fx_oracle(&issuer, &ns, &token),
        Some(FxOracleConfig {
            oracle,
            revenue_symbol: symbol_short!("USD"),
            payout_symbol: symbol_short!("XLM"),
            max_oracle_age_secs: 60,
        })
    );
}

#[test]
fn paused_contract_rejects_write() {
    let (env, client, admin, issuer, token) = setup();
    let ns = symbol_short!("def");
    let oracle = Address::generate(&env);

    client.pause_admin(&admin);

    let result = client.try_set_fx_oracle(
        &issuer,
        &ns,
        &token,
        &oracle,
        &symbol_short!("USD"),
        &symbol_short!("XLM"),
        &60,
    );
    assert!(matches!(result.err(), Some(Ok(RevoraError::ContractPaused))));
    assert_eq!(client.get_fx_oracle(&issuer, &ns, &token), None);

    // Unpausing restores the write path.
    client.unpause_admin(&admin);
    client.set_fx_oracle(
        &issuer,
        &ns,
        &token,
        &oracle,
        &symbol_short!("USD"),
        &symbol_short!("XLM"),
        &60,
    );
    assert!(client.get_fx_oracle(&issuer, &ns, &token).is_some());
}

#[test]
fn scopes_configuration_per_offering() {
    let (env, client, _admin, issuer_a, token_a) = setup();
    let ns = symbol_short!("def");
    let oracle = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_b = Address::generate(&env);
    register_offering(&env, &client, &issuer_b, &token_b);

    client.set_fx_oracle(
        &issuer_a,
        &ns,
        &token_a,
        &oracle,
        &symbol_short!("USD"),
        &symbol_short!("XLM"),
        &60,
    );

    assert!(client.get_fx_oracle(&issuer_a, &ns, &token_a).is_some());
    assert_eq!(client.get_fx_oracle(&issuer_b, &ns, &token_b), None);
}
