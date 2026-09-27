//! # `is_whitelist_enabled` — enforcement flag semantics
//!
//! `is_whitelist_enabled` is the switch that decides whether the per-offering
//! whitelist gate is enforced during transfers. The contract derives it from the
//! stored whitelist map: enforcement is *on* exactly when the map has at least one
//! entry, and *off* for a missing map or an emptied map.
//!
//! These tests pin that contract from the outside and cover:
//! - the default (`false`) for a registered offering with no whitelist activity
//! - the transition to `true` on the first `whitelist_add`
//! - the transition back to `false` once the last investor is removed
//! - non-listed / duplicate removals never flip the flag on
//! - the flag is scoped per offering (namespace, issuer, token)
//! - the read is pure: it never creates storage and never mutates the flag
//! - unauthorized writes and frozen-contract writes cannot enable enforcement
//! - the interplay with `is_whitelisted` while enforcement is off

#![cfg(test)]

use crate::{DataKey, OfferingId, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, Vec,
};

/// Register a fresh contract with one offering owned by `issuer` under `ns`/`token`.
fn setup() -> (Env, Address, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let _ = client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    (env, contract_id, client, issuer, token)
}

/// Has the whitelist storage entry been created for this offering?
fn whitelist_entry_exists(
    env: &Env,
    contract_id: &Address,
    issuer: &Address,
    namespace: &soroban_sdk::Symbol,
    token: &Address,
) -> bool {
    let key = DataKey::Whitelist(OfferingId {
        issuer: issuer.clone(),
        namespace: namespace.clone(),
        token: token.clone(),
    });
    env.as_contract(contract_id, || env.storage().persistent().has(&key))
}

// ─── 1. Defaults ─────────────────────────────────────────────────────────────

#[test]
fn whitelist_disabled_by_default_for_registered_offering() {
    let (_env, _id, client, issuer, token) = setup();

    assert!(
        !client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token),
        "a registered offering without whitelist entries must not enforce the whitelist"
    );
}

#[test]
fn whitelist_disabled_for_unregistered_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    assert!(
        !client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token),
        "an unknown offering must report the whitelist as disabled instead of panicking"
    );
}

// ─── 2. Enabling on first add ────────────────────────────────────────────────

#[test]
fn whitelist_enabled_after_first_add() {
    let (env, _id, client, issuer, token) = setup();
    let investor = Address::generate(&env);

    assert!(!client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));

    let result = client.try_whitelist_add(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &investor,
    );
    assert!(result.is_ok(), "issuer add must succeed, got {result:?}");

    assert!(
        client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token),
        "a single whitelist entry must enable enforcement"
    );
    assert!(client.is_whitelisted(&issuer, &symbol_short!("ns"), &token, &investor));
}

#[test]
fn whitelist_stays_enabled_for_duplicate_adds() {
    let (env, _id, client, issuer, token) = setup();
    let investor = Address::generate(&env);

    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor)
        .is_ok());
    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor)
        .is_ok());

    assert!(client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));
    assert_eq!(
        client.get_whitelist(&issuer, &symbol_short!("ns"), &token).len(),
        1,
        "duplicate adds must not duplicate the entry"
    );
}

#[test]
fn whitelist_enabled_with_multiple_investors() {
    let (env, _id, client, issuer, token) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &a)
        .is_ok());
    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &b)
        .is_ok());

    assert!(client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));
    assert_eq!(
        client.get_whitelist(&issuer, &symbol_short!("ns"), &token).len(),
        2
    );
}

// ─── 3. Disabling when the map empties ───────────────────────────────────────

#[test]
fn whitelist_disabled_after_removing_the_only_investor() {
    let (env, _id, client, issuer, token) = setup();
    let investor = Address::generate(&env);

    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor)
        .is_ok());
    assert!(client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));

    assert!(client
        .try_whitelist_remove(&issuer, &issuer, &symbol_short!("ns"), &token, &investor)
        .is_ok());

    assert!(
        !client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token),
        "removing the last entry must disable enforcement again"
    );
    assert!(!client.is_whitelisted(&issuer, &symbol_short!("ns"), &token, &investor));
}

#[test]
fn whitelist_remains_enabled_while_one_investor_remains() {
    let (env, _id, client, issuer, token) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &a)
        .is_ok());
    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &b)
        .is_ok());
    assert!(client
        .try_whitelist_remove(&issuer, &issuer, &symbol_short!("ns"), &token, &a)
        .is_ok());

    assert!(
        client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token),
        "one remaining entry is enough to keep enforcement on"
    );
    assert_eq!(
        client.get_whitelist(&issuer, &symbol_short!("ns"), &token).len(),
        1
    );
    assert!(!client.is_whitelisted(&issuer, &symbol_short!("ns"), &token, &a));
    assert!(client.is_whitelisted(&issuer, &symbol_short!("ns"), &token, &b));
}

#[test]
fn removing_a_non_listed_investor_does_not_enable_whitelist() {
    let (env, _id, client, issuer, token) = setup();
    let stranger = Address::generate(&env);

    assert!(client
        .try_whitelist_remove(&issuer, &issuer, &symbol_short!("ns"), &token, &stranger)
        .is_ok());

    assert!(
        !client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token),
        "a no-op removal must not enable enforcement"
    );
}

// ─── 4. Per-offering scoping ─────────────────────────────────────────────────

#[test]
fn whitelist_flag_is_scoped_per_namespace() {
    let (env, _id, client, issuer, token) = setup();
    let investor = Address::generate(&env);

    let _ = client.register_offering(
        &issuer,
        &Vec::new(&env),
        &2u32,
        &symbol_short!("ns2"),
        &token,
        &1_000,
        &token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor)
        .is_ok());

    assert!(client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));
    assert!(
        !client.is_whitelist_enabled(&issuer, &symbol_short!("ns2"), &token),
        "a sibling namespace must not inherit enforcement"
    );
}

#[test]
fn whitelist_flag_is_scoped_per_token() {
    let (env, _id, client, issuer, token) = setup();
    let investor = Address::generate(&env);
    let other_token = Address::generate(&env);

    let _ = client.register_offering(
        &issuer,
        &Vec::new(&env),
        &2u32,
        &symbol_short!("ns"),
        &other_token,
        &1_000,
        &other_token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor)
        .is_ok());

    assert!(client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));
    assert!(
        !client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &other_token),
        "a sibling token must not inherit enforcement"
    );
}

#[test]
fn whitelist_flag_is_scoped_per_issuer() {
    let (env, _id, client, issuer, token) = setup();
    let investor = Address::generate(&env);
    let second_issuer = Address::generate(&env);

    let _ = client.register_offering(
        &second_issuer,
        &Vec::new(&env),
        &2u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor)
        .is_ok());

    assert!(client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));
    assert!(
        !client.is_whitelist_enabled(&second_issuer, &symbol_short!("ns"), &token),
        "another issuer's offering must not inherit enforcement"
    );
}

// ─── 5. The read is pure ─────────────────────────────────────────────────────

#[test]
fn reading_the_flag_does_not_create_state() {
    let (env, contract_id, client, issuer, token) = setup();

    assert!(!whitelist_entry_exists(&env, &contract_id, &issuer, &symbol_short!("ns"), &token));

    assert!(!client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));

    assert!(
        !whitelist_entry_exists(&env, &contract_id, &issuer, &symbol_short!("ns"), &token),
        "is_whitelist_enabled must not write a whitelist entry"
    );
}

#[test]
fn reading_the_flag_is_idempotent() {
    let (env, _id, client, issuer, token) = setup();
    let investor = Address::generate(&env);

    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor)
        .is_ok());

    let first = client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token);
    let second = client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token);
    let third = client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token);

    assert_eq!(first, second);
    assert_eq!(second, third);
    assert!(third);

    // Reading is not affected by ledger movement.
    env.ledger().with_mut(|l| l.timestamp = 9_999);
    assert!(client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));
}

// ─── 6. Rejected writes cannot enable enforcement ────────────────────────────

#[test]
fn unauthorized_add_does_not_enable_whitelist() {
    let (env, _id, client, issuer, token) = setup();
    let stranger = Address::generate(&env);
    let investor = Address::generate(&env);

    let result = client.try_whitelist_add(
        &stranger,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &investor,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::NotAuthorized)),
        "a non-issuer, non-admin caller must be rejected"
    );

    assert!(
        !client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token),
        "a rejected add must not flip the enforcement flag"
    );
}

#[test]
fn add_for_unregistered_offering_does_not_enable_whitelist() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    let result = client.try_whitelist_add(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &investor,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert!(!client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));
}

#[test]
fn frozen_contract_blocks_enabling_the_whitelist() {
    let (env, contract_id, client, issuer, token) = setup();
    let investor = Address::generate(&env);

    // Enforcement is enabled before the freeze and must survive it.
    assert!(client
        .try_whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor)
        .is_ok());
    assert!(client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));

    // A second, still-disabled offering is registered before the freeze.
    let second_issuer = Address::generate(&env);
    let second_token = Address::generate(&env);
    let _ = client.register_offering(
        &second_issuer,
        &Vec::new(&env),
        &2u32,
        &symbol_short!("ns"),
        &second_token,
        &1_000,
        &second_token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
    assert!(!client.is_whitelist_enabled(&second_issuer, &symbol_short!("ns"), &second_token));

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.freeze();

    let result = client.try_whitelist_add(
        &second_issuer,
        &second_issuer,
        &symbol_short!("ns"),
        &second_token,
        &investor,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::ContractFrozen)),
        "a frozen contract must refuse whitelist writes"
    );

    assert!(
        !client.is_whitelist_enabled(&second_issuer, &symbol_short!("ns"), &second_token),
        "the frozen write must not have created an entry"
    );
    assert!(
        !whitelist_entry_exists(
            &env,
            &contract_id,
            &second_issuer,
            &symbol_short!("ns"),
            &second_token
        ),
        "no whitelist storage entry may be created while frozen"
    );

    // The pre-freeze offering keeps enforcement on.
    assert!(client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));
}

// ─── 7. Interplay with `is_whitelisted` ──────────────────────────────────────

#[test]
fn whitelist_gate_is_off_while_flag_is_false() {
    let (env, _id, client, issuer, token) = setup();
    let anyone = Address::generate(&env);

    assert!(!client.is_whitelist_enabled(&issuer, &symbol_short!("ns"), &token));
    assert!(
        !client.is_whitelisted(&issuer, &symbol_short!("ns"), &token, &anyone),
        "with enforcement off, no address reports as whitelisted"
    );
    assert!(client.get_whitelist(&issuer, &symbol_short!("ns"), &token).is_empty());
}
