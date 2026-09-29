//! # Adversarial coverage for `whitelist_remove`
//!
//! Focused tests for `RevoraRevenueShare::whitelist_remove` covering the
//! success path, idempotency, authorization boundaries, missing-offering
//! handling, namespace isolation, admin override, and the frozen/paused
//! guards. Every rejected operation is asserted to leave storage unchanged.
//!
//! The public contract is not modified by these tests.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, Symbol, Vec,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Register a fresh contract with `admin` initialized and one offering owned by
/// `issuer` under `namespace`/`token`. Returns the client.
fn setup_offering<'a>(
    env: &'a Env,
    admin: &Address,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) -> RevoraRevenueShareClient<'a> {
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &contract_id);
    client.initialize(admin, &None::<Address>, &None::<bool>);
    let payout_asset = Address::generate(env);
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        namespace,
        token,
        &1_000u32,
        &payout_asset,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
    client
}

/// Assert a `try_*` result is exactly the expected `RevoraError`.
fn assert_err<T: core::fmt::Debug>(
    result: Result<T, Result<RevoraError, soroban_sdk::InvokeError>>,
    expected: RevoraError,
) {
    match result {
        Err(Ok(err)) => assert_eq!(err, expected, "unexpected error variant"),
        other => panic!("expected {:?}, got {:?}", expected, other),
    }
}

// ── Success path ──────────────────────────────────────────────────────────────

#[test]
fn whitelist_remove_removes_listed_investor() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let client = setup_offering(&env, &admin, &issuer, &ns, &token);

    let investor = Address::generate(&env);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));

    client.whitelist_remove(&issuer, &issuer, &ns, &token, &investor);
    assert!(!client.is_whitelisted(&issuer, &ns, &token, &investor));
    assert_eq!(client.get_whitelist(&issuer, &ns, &token).len(), 0);
}

#[test]
fn whitelist_remove_emits_wl_rem_event() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let client = setup_offering(&env, &admin, &issuer, &ns, &token);

    let investor = Address::generate(&env);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);
    client.whitelist_remove(&issuer, &issuer, &ns, &token, &investor);

    let events = env.events().all();
    let found = events.iter().any(|(_, topics, _)| {
        topics
            .get(0)
            .map(|v| {
                Symbol::try_from_val(&env, &v)
                    .map(|s| s == symbol_short!("wl_rem"))
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    });
    assert!(found, "expected a wl_rem event to be emitted");
}

// ── Idempotency / boundary ────────────────────────────────────────────────────

#[test]
fn whitelist_remove_nonexistent_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let client = setup_offering(&env, &admin, &issuer, &ns, &token);

    let investor = Address::generate(&env);
    // Never added — removal must succeed and leave the whitelist empty.
    client.whitelist_remove(&issuer, &issuer, &ns, &token, &investor);
    assert!(!client.is_whitelisted(&issuer, &ns, &token, &investor));
    assert_eq!(client.get_whitelist(&issuer, &ns, &token).len(), 0);
}

#[test]
fn whitelist_remove_twice_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let client = setup_offering(&env, &admin, &issuer, &ns, &token);

    let investor = Address::generate(&env);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);
    client.whitelist_remove(&issuer, &issuer, &ns, &token, &investor);
    // Second removal must not error.
    client.whitelist_remove(&issuer, &issuer, &ns, &token, &investor);
    assert!(!client.is_whitelisted(&issuer, &ns, &token, &investor));
}

#[test]
fn whitelist_remove_only_removes_target_investor() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let client = setup_offering(&env, &admin, &issuer, &ns, &token);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &a);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &b);

    client.whitelist_remove(&issuer, &issuer, &ns, &token, &a);
    assert!(!client.is_whitelisted(&issuer, &ns, &token, &a));
    assert!(client.is_whitelisted(&issuer, &ns, &token, &b));
}

// ── Authorization ─────────────────────────────────────────────────────────────

#[test]
fn whitelist_remove_unauthorized_caller_rejected_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let client = setup_offering(&env, &admin, &issuer, &ns, &token);

    let investor = Address::generate(&env);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);

    let stranger = Address::generate(&env);
    let result = client.try_whitelist_remove(&stranger, &issuer, &ns, &token, &investor);
    assert_err(result, RevoraError::NotAuthorized);

    // Rejected operation must not mutate state.
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));
}

#[test]
fn whitelist_remove_admin_can_remove_on_behalf_of_issuer() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let client = setup_offering(&env, &admin, &issuer, &ns, &token);

    let investor = Address::generate(&env);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);

    // Admin is an authorized override even though it is not the issuer.
    client.whitelist_remove(&admin, &issuer, &ns, &token, &investor);
    assert!(!client.is_whitelisted(&issuer, &ns, &token, &investor));
}

// ── Missing offering ──────────────────────────────────────────────────────────

#[test]
fn whitelist_remove_unknown_offering_returns_offering_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let client = setup_offering(&env, &admin, &issuer, &ns, &token);

    let unknown_token = Address::generate(&env);
    let investor = Address::generate(&env);
    let result = client.try_whitelist_remove(&issuer, &issuer, &ns, &unknown_token, &investor);
    assert_err(result, RevoraError::OfferingNotFound);
}

// ── Namespace isolation ───────────────────────────────────────────────────────

#[test]
fn whitelist_remove_is_namespace_scoped() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns_a = symbol_short!("nsa");
    let ns_b = symbol_short!("nsb");

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let payout_asset = Address::generate(&env);
    for ns in [&ns_a, &ns_b] {
        client.register_offering(
            &issuer,
            &Vec::new(&env),
            &1u32,
            ns,
            &token,
            &1_000u32,
            &payout_asset,
            &0i128,
            &symbol_short!(""),
            &0u32,
        );
    }

    let investor = Address::generate(&env);
    client.whitelist_add(&issuer, &issuer, &ns_a, &token, &investor);
    client.whitelist_add(&issuer, &issuer, &ns_b, &token, &investor);

    // Removing from ns_a must not affect ns_b.
    client.whitelist_remove(&issuer, &issuer, &ns_a, &token, &investor);
    assert!(!client.is_whitelisted(&issuer, &ns_a, &token, &investor));
    assert!(client.is_whitelisted(&issuer, &ns_b, &token, &investor));
}

// ── Frozen / paused guards ────────────────────────────────────────────────────

#[test]
fn whitelist_remove_blocked_when_frozen_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let client = setup_offering(&env, &admin, &issuer, &ns, &token);

    let investor = Address::generate(&env);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);

    client.freeze();
    let result = client.try_whitelist_remove(&issuer, &issuer, &ns, &token, &investor);
    assert_err(result, RevoraError::ContractFrozen);

    // Frozen rejection must not mutate the whitelist.
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));
}

#[test]
fn whitelist_remove_blocked_when_paused_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let client = setup_offering(&env, &admin, &issuer, &ns, &token);

    let investor = Address::generate(&env);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);

    client.pause_admin(&admin);
    let result = client.try_whitelist_remove(&issuer, &issuer, &ns, &token, &investor);
    assert_err(result, RevoraError::ContractPaused);

    // Paused rejection must not mutate the whitelist.
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));
}
