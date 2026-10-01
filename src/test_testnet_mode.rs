//! Adversarial coverage for `set_testnet_mode` (`src/lib.rs`, Revora-Contracts).
//!
//! `set_testnet_mode(env, enabled: bool)` is the admin-only switch that relaxes
//! on-chain validation (currently the `revenue_share_bps <= 10_000` cap enforced
//! by `register_offering`) for test networks. It must therefore:
//!
//! 1. Default to **disabled** — a fresh contract must reject an out-of-range share.
//! 2. Actually flip the stored flag, in **both** directions, on every call.
//! 3. Emit `test_mode` with the new flag so off-chain indexers can observe the change.
//! 4. Fail closed with the typed `NotInitialized` / `ContractFrozen` errors, and
//!    leave the stored flag (and the event log) untouched when it does.
//!
//! The flag lives in private storage, so it is observed here through its only
//! public consequence: whether `register_offering` accepts `RELAXED_BPS`.
//! `set_testnet_mode` takes no caller argument and gates solely on
//! `admin.require_auth()`, so caller identity cannot be varied from the outside;
//! the repo's auth suite (`src/test_auth.rs`) documents that the un-mocked
//! `require_auth` path panics rather than returning a typed error, which is why
//! the guards that *do* return typed errors are what we assert on here.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Vec,
};

/// Basis points that only pass while testnet mode is enabled (> 10 000 = > 100%).
const RELAXED_BPS: u32 = 15_000;

struct Ctx {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    admin: Address,
    ns: Symbol,
    payout: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    Ctx { env, client, admin, ns: symbol_short!("def"), payout: Address::generate(&env) }
}

/// Register an offering whose share exceeds the production cap.
///
/// A fresh `token` is required per call: `register_offering` is idempotent on
/// `(issuer, namespace, token)` and would otherwise return early without
/// re-running the bps validation.
fn try_relaxed_registration(c: &Ctx, token: &Address) -> Result<(), RevoraError> {
    match c.client.try_register_offering(
        &c.admin,
        &Vec::new(&c.env),
        &1u32,
        &c.ns,
        token,
        &RELAXED_BPS,
        &c.payout,
        &0i128,
        &symbol_short!(""),
        &0u32,
    ) {
        Ok(()) => Ok(()),
        Err(inner) => Err(inner.unwrap()),
    }
}

/// Assert that the contract currently accepts an out-of-range revenue share.
fn assert_relaxed_accepted(c: &Ctx) {
    let token = Address::generate(&c.env);
    assert!(
        try_relaxed_registration(c, &token).is_ok(),
        "testnet mode must accept {} bps",
        RELAXED_BPS
    );
}

/// Assert that the contract currently enforces the production share cap.
fn assert_relaxed_rejected(c: &Ctx) {
    let token = Address::generate(&c.env);
    assert_eq!(
        try_relaxed_registration(c, &token),
        Err(RevoraError::InvalidRevenueShareBps),
        "production mode must reject {} bps",
        RELAXED_BPS
    );
}

fn set_mode(c: &Ctx, enabled: bool) {
    c.client.set_testnet_mode(&enabled).unwrap();
}

/// Decode every `test_mode` flag emitted at or after `start_idx`.
fn testnet_mode_flags(env: &Env, start_idx: u32) -> Vec<bool> {
    let all = env.events().all();
    let mut flags = Vec::new(env);
    let mut i = start_idx;
    while i < all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        if !topics.is_empty() {
            let t0: Symbol = topics.get(0).unwrap().into_val(env);
            if t0 == crate::EVENT_TESTNET_MODE {
                let flag: bool = data.into_val(env);
                flags.push_back(flag);
            }
        }
        i += 1;
    }
    flags
}

fn testnet_mode_event_count(env: &Env, start_idx: u32) -> u32 {
    testnet_mode_flags(env, start_idx).len()
}

// ── Default state ─────────────────────────────────────────────────────────────

#[test]
fn testnet_mode_is_disabled_by_default() {
    let c = setup();
    // The relaxation only exists while the flag is on; out of the box the strict
    // production cap must apply.
    assert_relaxed_rejected(&c);
}

// ── State transitions ────────────────────────────────────────────────────────

#[test]
fn enabling_testnet_mode_relaxes_share_validation() {
    let c = setup();
    set_mode(&c, true);
    assert_relaxed_accepted(&c);
}

#[test]
fn disabling_testnet_mode_restores_strict_validation() {
    let c = setup();
    set_mode(&c, true);
    assert_relaxed_accepted(&c);

    set_mode(&c, false);
    assert_relaxed_rejected(&c);
}

#[test]
fn toggling_repeatedly_always_tracks_the_latest_write() {
    let c = setup();
    for enabled in [true, false, true, true, false] {
        set_mode(&c, enabled);
        if enabled {
            assert_relaxed_accepted(&c);
        } else {
            assert_relaxed_rejected(&c);
        }
    }
}

#[test]
fn setting_false_on_a_fresh_contract_is_an_explicit_no_op_write() {
    let c = setup();
    assert_relaxed_rejected(&c);

    set_mode(&c, false);

    // Writing the default value must not flip the relaxation on, and must still be
    // observable for indexers.
    assert_relaxed_rejected(&c);
    assert_eq!(testnet_mode_event_count(&c.env, 0), 1);
}

// ── Events ───────────────────────────────────────────────────────────────────

#[test]
fn enabling_emits_the_flag_as_true() {
    let c = setup();
    let before = c.env.events().all().len();

    set_mode(&c, true);

    let flags = testnet_mode_flags(&c.env, before);
    assert_eq!(flags.len(), 1, "exactly one test_mode event per call");
    assert!(flags.get(0).unwrap());
}

#[test]
fn disabling_emits_the_flag_as_false() {
    let c = setup();
    set_mode(&c, true);
    let before = c.env.events().all().len();

    set_mode(&c, false);

    let flags = testnet_mode_flags(&c.env, before);
    assert_eq!(flags.len(), 1);
    assert!(!flags.get(0).unwrap());
}

#[test]
fn every_successful_call_emits_exactly_one_event() {
    let c = setup();
    let before = c.env.events().all().len();

    set_mode(&c, true);
    set_mode(&c, true);
    set_mode(&c, false);

    let flags = testnet_mode_flags(&c.env, before);
    assert_eq!(flags.len(), 3);
    assert!(flags.get(0).unwrap());
    assert!(flags.get(1).unwrap());
    assert!(!flags.get(2).unwrap());
}

// ── Fail-closed paths ───────────────────────────────────────────────────────

#[test]
fn set_testnet_mode_without_initialization_returns_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    // No initialize() call: the admin slot is empty, so the admin lookup fails
    // before require_auth is ever reached.
    let res = client.try_set_testnet_mode(&true);
    assert_eq!(res, Err(Ok(RevoraError::NotInitialized)));
    assert_eq!(testnet_mode_event_count(&env, 0), 0);
}

#[test]
fn set_testnet_mode_is_rejected_while_the_contract_is_frozen() {
    let c = setup();
    c.client.freeze().unwrap();
    let before = c.env.events().all().len();

    let enable = c.client.try_set_testnet_mode(&true);
    let disable = c.client.try_set_testnet_mode(&false);

    assert_eq!(enable, Err(Ok(RevoraError::ContractFrozen)));
    assert_eq!(disable, Err(Ok(RevoraError::ContractFrozen)));
    // The freeze guard runs before the storage write, so nothing changed and no
    // test_mode event was emitted.
    assert_eq!(testnet_mode_event_count(&c.env, before), 0);
    assert!(c.client.is_frozen());
}

#[test]
fn a_frozen_rejection_cannot_flip_an_already_enabled_flag() {
    let c = setup();
    set_mode(&c, true);
    // Observed while the contract is still operable: the flag really is enabled.
    assert_relaxed_accepted(&c);

    // Freezing is a one-way latch in this contract, so the rejected call below is
    // the last observable interaction. The evidence that the flag was not flipped
    // is that `require_not_frozen` short-circuits before the storage write, and the
    // write and its `test_mode` event live on the same code path — nothing emitted,
    // nothing written.
    c.client.freeze().unwrap();
    let before = c.env.events().all().len();

    let res = c.client.try_set_testnet_mode(&false);

    assert_eq!(res, Err(Ok(RevoraError::ContractFrozen)));
    assert_eq!(testnet_mode_event_count(&c.env, before), 0);
}
