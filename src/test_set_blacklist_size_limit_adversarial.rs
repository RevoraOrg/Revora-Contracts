//! Adversarial coverage for `set_blacklist_size_limit` (#1103).
//!
//! The setter is the only way for an issuer to shrink the per-offering
//! blacklist cap, so its failure modes matter as much as its happy path. This
//! suite pins:
//!
//! - the configured cap is enforced exactly at the boundary by both
//!   `blacklist_add` and `blacklist_add_many`,
//! - `max_size == 0` is rejected without clobbering the previously stored cap,
//! - caps are scoped per offering (token *and* namespace), not per issuer,
//! - only the offering's current issuer may change the cap — the global admin
//!   escape hatch that `blacklist_add` grants does **not** apply here,
//! - lowering the cap below the current size never evicts existing entries,
//! - raising the cap restores capacity, and idempotent re-adds are still free,
//! - unknown offerings and a frozen contract are rejected before any write.

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec as SdkVec};

const NS: Symbol = symbol_short!("def");
const NS_ALT: Symbol = symbol_short!("alt");

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);

    (env, contract_id, issuer, admin)
}

fn register(env: &Env, contract_id: &Address, issuer: &Address, ns: &Symbol, token: &Address) {
    let client = RevoraRevenueShareClient::new(env, contract_id);
    let payout_asset = Address::generate(env);
    client.register_offering(
        issuer,
        &SdkVec::new(env),
        &1u32,
        ns,
        token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
}

fn size(env: &Env, contract_id: &Address, issuer: &Address, ns: &Symbol, token: &Address) -> u32 {
    RevoraRevenueShareClient::new(env, contract_id).get_blacklist_size(issuer, ns, token)
}

#[test]
fn configured_limit_is_enforced_exactly_at_the_boundary() {
    let (env, contract_id, issuer, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &NS, &token);

    client.set_blacklist_size_limit(&issuer, &issuer, &NS, &token, &3);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    let d = Address::generate(&env);

    // Exactly `max_size` entries are accepted.
    client.blacklist_add(&issuer, &issuer, &NS, &token, &a);
    client.blacklist_add(&issuer, &issuer, &NS, &token, &b);
    client.blacklist_add(&issuer, &issuer, &NS, &token, &c);

    // The next distinct entry is refused.
    let result = client.try_blacklist_add(&issuer, &issuer, &NS, &token, &d);
    assert_eq!(result, Err(Ok(RevoraError::BlacklistSizeLimitExceeded)));

    assert_eq!(size(&env, &contract_id, &issuer, &NS, &token), 3);
    assert!(!client.is_blacklisted(&issuer, &NS, &token, &d));
}

#[test]
fn max_size_zero_is_rejected_and_preserves_the_previous_limit() {
    let (env, contract_id, issuer, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &NS, &token);

    client.set_blacklist_size_limit(&issuer, &issuer, &NS, &token, &2);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &NS, &token, &a);
    client.blacklist_add(&issuer, &issuer, &NS, &token, &b);

    let result = client.try_set_blacklist_size_limit(&issuer, &issuer, &NS, &token, &0);
    assert_eq!(result, Err(Ok(RevoraError::LimitReached)));

    // A rejected write must not have replaced the stored cap with 0 (which would
    // make the blacklist unusable) nor with anything else.
    assert_eq!(size(&env, &contract_id, &issuer, &NS, &token), 2);
    let c = Address::generate(&env);
    let still_capped = client.try_blacklist_add(&issuer, &issuer, &NS, &token, &c);
    assert_eq!(still_capped, Err(Ok(RevoraError::BlacklistSizeLimitExceeded)));
}

#[test]
fn lowering_the_limit_never_evicts_existing_entries() {
    let (env, contract_id, issuer, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &NS, &token);

    let keep = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    for investor in keep.iter() {
        client.blacklist_add(&issuer, &issuer, &NS, &token, investor);
    }

    client.set_blacklist_size_limit(&issuer, &issuer, &NS, &token, &1);

    assert_eq!(size(&env, &contract_id, &issuer, &NS, &token), 3);
    for investor in keep.iter() {
        assert!(
            client.is_blacklisted(&issuer, &NS, &token, investor),
            "shrinking the cap must not silently un-blacklist sanctions hits"
        );
    }

    let newcomer = Address::generate(&env);
    let result = client.try_blacklist_add(&issuer, &issuer, &NS, &token, &newcomer);
    assert_eq!(result, Err(Ok(RevoraError::BlacklistSizeLimitExceeded)));
}

#[test]
fn raising_the_limit_restores_capacity() {
    let (env, contract_id, issuer, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &NS, &token);

    client.set_blacklist_size_limit(&issuer, &issuer, &NS, &token, &1);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &NS, &token, &a);
    assert!(client.try_blacklist_add(&issuer, &issuer, &NS, &token, &b).is_err());

    client.set_blacklist_size_limit(&issuer, &issuer, &NS, &token, &3);

    client.blacklist_add(&issuer, &issuer, &NS, &token, &b);
    client.blacklist_add(&issuer, &issuer, &NS, &token, &c);
    assert_eq!(size(&env, &contract_id, &issuer, &NS, &token), 3);
}

#[test]
fn re_adding_an_existing_entry_is_allowed_at_capacity() {
    let (env, contract_id, issuer, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &NS, &token);

    client.set_blacklist_size_limit(&issuer, &issuer, &NS, &token, &1);

    let investor = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &NS, &token, &investor);

    // Idempotent re-add of a known entry does not consume new capacity.
    client.blacklist_add(&issuer, &issuer, &NS, &token, &investor);
    assert_eq!(size(&env, &contract_id, &issuer, &NS, &token), 1);
}

#[test]
fn limit_is_scoped_per_offering_token() {
    let (env, contract_id, issuer, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    register(&env, &contract_id, &issuer, &NS, &token_a);
    register(&env, &contract_id, &issuer, &NS, &token_b);

    client.set_blacklist_size_limit(&issuer, &issuer, &NS, &token_a, &1);

    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &NS, &token_a, &a1);
    assert_eq!(
        client.try_blacklist_add(&issuer, &issuer, &NS, &token_a, &a2),
        Err(Ok(RevoraError::BlacklistSizeLimitExceeded))
    );

    // The sibling offering keeps the default cap.
    let b1 = Address::generate(&env);
    let b2 = Address::generate(&env);
    let b3 = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &NS, &token_b, &b1);
    client.blacklist_add(&issuer, &issuer, &NS, &token_b, &b2);
    client.blacklist_add(&issuer, &issuer, &NS, &token_b, &b3);
}

#[test]
fn limit_is_scoped_per_namespace_for_the_same_token() {
    let (env, contract_id, issuer, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &NS, &token);
    register(&env, &contract_id, &issuer, &NS_ALT, &token);

    client.set_blacklist_size_limit(&issuer, &issuer, &NS, &token, &1);

    let def_a = Address::generate(&env);
    let def_b = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &NS, &token, &def_a);
    assert_eq!(
        client.try_blacklist_add(&issuer, &issuer, &NS, &token, &def_b),
        Err(Ok(RevoraError::BlacklistSizeLimitExceeded))
    );

    let alt_a = Address::generate(&env);
    let alt_b = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &NS_ALT, &token, &alt_a);
    client.blacklist_add(&issuer, &issuer, &NS_ALT, &token, &alt_b);
}

#[test]
fn non_issuer_cannot_change_the_limit() {
    let (env, contract_id, issuer, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &NS, &token);

    client.set_blacklist_size_limit(&issuer, &issuer, &NS, &token, &1);

    let stone = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &NS, &token, &stone);

    let impostor = Address::generate(&env);
    let result = client.try_set_blacklist_size_limit(&impostor, &issuer, &NS, &token, &50);
    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));

    // The cap must still be the one the issuer configured.
    let second = Address::generate(&env);
    let capped = client.try_blacklist_add(&issuer, &issuer, &NS, &token, &second);
    assert_eq!(capped, Err(Ok(RevoraError::BlacklistSizeLimitExceeded)));
}

#[test]
fn admin_escape_hatch_does_not_extend_to_the_limit() {
    let (env, contract_id, issuer, admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &NS, &token);

    // `blacklist_add` lets the admin act on any offering ...
    let investor = Address::generate(&env);
    client.blacklist_add(&admin, &issuer, &NS, &token, &investor);
    assert!(client.is_blacklisted(&issuer, &NS, &token, &investor));

    // ... but the size limit stays a per-offering issuer decision.
    let result = client.try_set_blacklist_size_limit(&admin, &issuer, &NS, &token, &1);
    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));
}

#[test]
fn unknown_offering_is_rejected() {
    let (env, contract_id, issuer, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let unknown_token = Address::generate(&env);

    let result = client.try_set_blacklist_size_limit(&issuer, &issuer, &NS, &unknown_token, &5);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn frozen_contract_rejects_limit_changes() {
    let (env, contract_id, issuer, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &NS, &token);

    assert!(client.freeze().is_ok());

    let result = client.try_set_blacklist_size_limit(&issuer, &issuer, &NS, &token, &5);
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
}

#[test]
fn batch_add_uses_the_same_cap_and_writes_nothing_on_overflow() {
    let (env, contract_id, issuer, _admin) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    register(&env, &contract_id, &issuer, &NS, &token);

    client.set_blacklist_size_limit(&issuer, &issuer, &NS, &token, &3);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    let d = Address::generate(&env);

    let mut too_many = SdkVec::new(&env);
    too_many.push_back(a.clone());
    too_many.push_back(b.clone());
    too_many.push_back(c.clone());
    too_many.push_back(d.clone());

    let result = client.try_blacklist_add_many(&issuer, &issuer, &NS, &token, &too_many);
    assert_eq!(result, Err(Ok(RevoraError::BlacklistSizeLimitExceeded)));
    assert_eq!(
        size(&env, &contract_id, &issuer, &NS, &token),
        0,
        "a rejected batch must not partially apply"
    );

    // Two entries fit; a batch of [a, b, c] then adds only one new address.
    let mut fits = SdkVec::new(&env);
    fits.push_back(a.clone());
    fits.push_back(b.clone());
    client.blacklist_add_many(&issuer, &issuer, &NS, &token, &fits);
    assert_eq!(size(&env, &contract_id, &issuer, &NS, &token), 2);

    let mut overlapping = SdkVec::new(&env);
    overlapping.push_back(a.clone());
    overlapping.push_back(b.clone());
    overlapping.push_back(c.clone());
    client.blacklist_add_many(&issuer, &issuer, &NS, &token, &overlapping);
    assert_eq!(
        size(&env, &contract_id, &issuer, &NS, &token),
        3,
        "already-blacklisted addresses must not count against the cap"
    );

    let mut over = SdkVec::new(&env);
    over.push_back(d.clone());
    let over_result = client.try_blacklist_add_many(&issuer, &issuer, &NS, &token, &over);
    assert_eq!(over_result, Err(Ok(RevoraError::BlacklistSizeLimitExceeded)));
    assert_eq!(size(&env, &contract_id, &issuer, &NS, &token), 3);
}
