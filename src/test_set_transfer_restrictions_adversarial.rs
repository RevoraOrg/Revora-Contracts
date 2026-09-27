#![cfg(test)]
//! Adversarial coverage for `set_transfer_restrictions` and its new
//! `get_transfer_restrictions` accessor.
//!
//! The cap was previously write-only, so a caller could neither confirm what
//! was stored nor reason about rejection behaviour. These tests make the stored
//! state observable and pin:
//!   - the getter's `None`-before-`Some` transition and overwrite semantics;
//!   - per-`(offering, category)` isolation (no cross-talk between namespaces,
//!     tokens or categories);
//!   - boundary `max_holders` values (`0` and `u32::MAX`);
//!   - the freeze guard, asserting state is unchanged after a rejected write;
//!   - the `require_auth` guard on the caller.

extern crate std;

use super::*;
use crate::{RevoraRevenueShare, RevoraRevenueShareClient, RevoraError, TransferRestrictions};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

fn client() -> (Env, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let c = RevoraRevenueShareClient::new(&env, &id);
    (env, c)
}

// ── Getter lifecycle ─────────────────────────────────────────────────────────

/// Nothing is configured until it is explicitly set.
#[test]
fn getter_is_none_before_any_set() {
    let (env, c) = client();
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "public");
    let token = Address::generate(&env);
    let category = Symbol::new(&env, "RegD");

    assert_eq!(c.get_transfer_restrictions(&issuer, &namespace, &token, &category), None);
}

/// A set is observable and round-trips exactly.
#[test]
fn set_then_get_round_trips() {
    let (env, c) = client();
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "public");
    let token = Address::generate(&env);
    let category = Symbol::new(&env, "RegD");

    c.set_transfer_restrictions(&issuer, &namespace, &token, &category, &7);

    let stored = c.get_transfer_restrictions(&issuer, &namespace, &token, &category).unwrap();
    assert_eq!(stored, TransferRestrictions { category: category.clone(), max_holders: 7 });
}

/// Setting the same key twice overwrites rather than failing.
#[test]
fn setting_again_overwrites_the_previous_cap() {
    let (env, c) = client();
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "public");
    let token = Address::generate(&env);
    let category = Symbol::new(&env, "RegD");

    c.set_transfer_restrictions(&issuer, &namespace, &token, &category, &1);
    c.set_transfer_restrictions(&issuer, &namespace, &token, &category, &42);

    assert_eq!(
        c.get_transfer_restrictions(&issuer, &namespace, &token, &category).unwrap().max_holders,
        42
    );
}

// ── Isolation ────────────────────────────────────────────────────────────────

/// Caps are scoped per category; setting one must not affect a sibling.
#[test]
fn caps_are_isolated_per_category() {
    let (env, c) = client();
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "public");
    let token = Address::generate(&env);
    let reg_d = Symbol::new(&env, "RegD");
    let reg_s = Symbol::new(&env, "RegS");

    c.set_transfer_restrictions(&issuer, &namespace, &token, &reg_d, &3);

    assert_eq!(
        c.get_transfer_restrictions(&issuer, &namespace, &token, &reg_d).unwrap().max_holders,
        3
    );
    // The sibling category stays unconfigured.
    assert_eq!(c.get_transfer_restrictions(&issuer, &namespace, &token, &reg_s), None);
}

/// Caps are scoped per offering; a different token or namespace never inherits.
#[test]
fn caps_are_isolated_per_offering() {
    let (env, c) = client();
    let issuer = Address::generate(&env);
    let namespace_a = Symbol::new(&env, "alpha");
    let namespace_b = Symbol::new(&env, "beta");
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let category = Symbol::new(&env, "RegD");

    c.set_transfer_restrictions(&issuer, &namespace_a, &token_a, &category, &5);

    assert_eq!(
        c.get_transfer_restrictions(&issuer, &namespace_a, &token_a, &category).unwrap().max_holders,
        5
    );
    assert_eq!(c.get_transfer_restrictions(&issuer, &namespace_b, &token_a, &category), None);
    assert_eq!(c.get_transfer_restrictions(&issuer, &namespace_a, &token_b, &category), None);
}

// ── Boundaries ───────────────────────────────────────────────────────────────

/// Both ends of the `u32` range are stored verbatim.
#[test]
fn boundary_zero_and_max_holders_are_stored_verbatim() {
    let (env, c) = client();
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "public");
    let token = Address::generate(&env);
    let zero_cat = Symbol::new(&env, "ZeroCat");
    let max_cat = Symbol::new(&env, "MaxCat");

    c.set_transfer_restrictions(&issuer, &namespace, &token, &zero_cat, &0);
    c.set_transfer_restrictions(&issuer, &namespace, &token, &max_cat, &u32::MAX);

    assert_eq!(
        c.get_transfer_restrictions(&issuer, &namespace, &token, &zero_cat).unwrap().max_holders,
        0
    );
    assert_eq!(
        c.get_transfer_restrictions(&issuer, &namespace, &token, &max_cat).unwrap().max_holders,
        u32::MAX
    );
}

// ── Rejection paths ──────────────────────────────────────────────────────────

/// A frozen contract rejects the write, and the rejected write is a no-op.
#[test]
fn frozen_contract_rejects_write_and_leaves_state_unchanged() {
    let (env, c) = client();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "public");
    let token = Address::generate(&env);
    let category = Symbol::new(&env, "RegD");

    c.initialize(&admin, &Some(admin.clone()), &Some(false));
    c.freeze();

    let result = c.try_set_transfer_restrictions(&issuer, &namespace, &token, &category, &9);
    assert_eq!(result.unwrap_err().unwrap(), RevoraError::ContractFrozen);

    // The rejected write must not have persisted anything.
    assert_eq!(c.get_transfer_restrictions(&issuer, &namespace, &token, &category), None);
}

/// Without auth mocking the caller's `require_auth` gate rejects the write.
#[test]
fn write_requires_caller_authorization() {
    // Deliberately NO `mock_all_auths()` so the auth gate is exercised.
    let env = Env::default();
    let id = env.register_contract(None, RevoraRevenueShare);
    let c = RevoraRevenueShareClient::new(&env, &id);
    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "public");
    let token = Address::generate(&env);
    let category = Symbol::new(&env, "RegD");

    let result = c.try_set_transfer_restrictions(&issuer, &namespace, &token, &category, &1);
    assert!(result.is_err(), "unauthorized write must be rejected");

    // And nothing was written.
    assert_eq!(c.get_transfer_restrictions(&issuer, &namespace, &token, &category), None);
}
