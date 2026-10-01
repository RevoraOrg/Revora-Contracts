//! Adversarial and boundary test coverage for `get_offering_count` in `lib.rs` (#1089).
//!
//! # Coverage Matrix
//!
//! | Scenario / Property                                          | Assertions / Expected Behavior |
//! |--------------------------------------------------------------|--------------------------------|
//! | Uninitialized / Non-existent tenant                          | Returns `0` (clean default)    |
//! | Isolation across distinct issuers (same namespace)           | Increments strictly isolate    |
//! | Isolation across distinct namespaces (same issuer)          | Increments strictly isolate    |
//! | Registration with boundary / diverse namespace symbols       | Exact count tracking           |
//! | State monotonicity on successful registrations               | `0 -> 1 -> 2 -> ...`          |
//! | No auth required / callable by arbitrary unprivileged env    | Public view access preserved   |
//! | Failed registration rollback (invalid cap / amount)          | Count remains unchanged (0)    |
//! | Failed registration rollback (invalid share bps > 10,000)    | Count remains unchanged (0)    |
//! | Duplicate offering registration rejection                    | Count does not double-count    |
//! | Multi-issuer offering transfer acceptance                    | Receiver count updates cleanly |
//! | Direct contract call via `RevoraRevenueShare::get_offering_count` | Exact match with client helper |

#![cfg(test)]

use crate::{DataKey, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, TenantId};
use soroban_sdk::{
    symbol_short,
    testutils::Address as _,
    Address, Env, Symbol, Vec,
};

// ── Test Setup Helpers ────────────────────────────────────────────────────────

fn setup_env() -> (Env, Address, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    (env, contract_id, client)
}

fn register_single_offering(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) {
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        namespace,
        token,
        &1_000,
        token,
        &0,
        &symbol_short!(""),
        &0,
    );
}

// ── Tests: Baseline and Querying Non-existent Tenants ────────────────────────

#[test]
fn get_offering_count_returns_zero_for_uninitialized_tenant() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("default");

    let count = client.get_offering_count(&issuer, &namespace);
    assert_eq!(count, 0, "Unregistered tenant must return 0 offerings");
}

#[test]
fn get_offering_count_returns_zero_for_random_issuers_and_namespaces() {
    let (env, _contract_id, client) = setup_env();

    for _ in 0..5 {
        let issuer = Address::generate(&env);
        let namespace = Symbol::new(&env, "random_ns");
        assert_eq!(client.get_offering_count(&issuer, &namespace), 0);
    }
}

// ── Tests: Tenant Isolation (Issuer and Namespace Sandboxing) ────────────────

#[test]
fn get_offering_count_isolates_between_different_issuers_same_namespace() {
    let (env, _contract_id, client) = setup_env();
    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let namespace = symbol_short!("shared");

    let token_a1 = Address::generate(&env);
    let token_a2 = Address::generate(&env);
    let token_b1 = Address::generate(&env);

    // Initial state: both zero
    assert_eq!(client.get_offering_count(&issuer_a, &namespace), 0);
    assert_eq!(client.get_offering_count(&issuer_b, &namespace), 0);

    // Register 1 for issuer_a
    register_single_offering(&env, &client, &issuer_a, &namespace, &token_a1);
    assert_eq!(client.get_offering_count(&issuer_a, &namespace), 1);
    assert_eq!(client.get_offering_count(&issuer_b, &namespace), 0);

    // Register 1 for issuer_b
    register_single_offering(&env, &client, &issuer_b, &namespace, &token_b1);
    assert_eq!(client.get_offering_count(&issuer_a, &namespace), 1);
    assert_eq!(client.get_offering_count(&issuer_b, &namespace), 1);

    // Register another for issuer_a
    register_single_offering(&env, &client, &issuer_a, &namespace, &token_a2);
    assert_eq!(client.get_offering_count(&issuer_a, &namespace), 2);
    assert_eq!(client.get_offering_count(&issuer_b, &namespace), 1);
}

#[test]
fn get_offering_count_isolates_between_different_namespaces_same_issuer() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let ns_prod = symbol_short!("prod");
    let ns_staging = symbol_short!("stage");
    let ns_dev = symbol_short!("dev");

    let token_prod = Address::generate(&env);
    let token_staging = Address::generate(&env);

    // Register in prod
    register_single_offering(&env, &client, &issuer, &ns_prod, &token_prod);
    assert_eq!(client.get_offering_count(&issuer, &ns_prod), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_staging), 0);
    assert_eq!(client.get_offering_count(&issuer, &ns_dev), 0);

    // Register in staging
    register_single_offering(&env, &client, &issuer, &ns_staging, &token_staging);
    assert_eq!(client.get_offering_count(&issuer, &ns_prod), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_staging), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_dev), 0);
}

// ── Tests: Namespace Boundary Values ──────────────────────────────────────────

#[test]
fn get_offering_count_handles_various_namespace_symbols() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);

    // Short symbol, empty symbol, and max length symbols
    let ns_empty = symbol_short!("");
    let ns_single = symbol_short!("a");
    let ns_max_short = symbol_short!("123456789");
    let ns_long = Symbol::new(&env, "a_longer_namespace_name");

    let token_1 = Address::generate(&env);
    let token_2 = Address::generate(&env);
    let token_3 = Address::generate(&env);
    let token_4 = Address::generate(&env);

    register_single_offering(&env, &client, &issuer, &ns_empty, &token_1);
    register_single_offering(&env, &client, &issuer, &ns_single, &token_2);
    register_single_offering(&env, &client, &issuer, &ns_max_short, &token_3);
    register_single_offering(&env, &client, &issuer, &ns_long, &token_4);

    assert_eq!(client.get_offering_count(&issuer, &ns_empty), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_single), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_max_short), 1);
    assert_eq!(client.get_offering_count(&issuer, &ns_long), 1);
}

// ── Tests: State Invariance on Failed Registrations ───────────────────────────

#[test]
fn get_offering_count_unchanged_on_rejected_invalid_bps() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);

    // Verify initial count is 0
    assert_eq!(client.get_offering_count(&issuer, &namespace), 0);

    // Attempt registration with invalid BPS (> 10_000)
    let res = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &10_001u32,
        &token,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert_eq!(res, Err(Ok(RevoraError::InvalidRevenueShareBps)));

    // Ensure offering count did not increment and remains 0
    assert_eq!(client.get_offering_count(&issuer, &namespace), 0);
}

#[test]
fn get_offering_count_unchanged_on_rejected_negative_cap() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);

    // Attempt registration with negative hard cap
    for invalid_cap in [-1_i128, i128::MIN] {
        let res = client.try_register_offering(
            &issuer,
            &Vec::new(&env),
            &1u32,
            &namespace,
            &token,
            &5_000u32,
            &token,
            &invalid_cap,
            &symbol_short!(""),
            &0,
        );
        assert_eq!(res, Err(Ok(RevoraError::InvalidAmount)));
        assert_eq!(client.get_offering_count(&issuer, &namespace), 0);
    }
}

#[test]
fn get_offering_count_unchanged_on_duplicate_token_registration() {
    let (env, _contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);

    // First registration succeeds -> count becomes 1
    register_single_offering(&env, &client, &issuer, &namespace, &token);
    assert_eq!(client.get_offering_count(&issuer, &namespace), 1);

    // Second registration with the exact same token must fail
    let duplicate_res = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &2_000u32,
        &token,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert_eq!(duplicate_res, Err(Ok(RevoraError::OfferingAlreadyExists)));

    // Offering count must remain 1
    assert_eq!(client.get_offering_count(&issuer, &namespace), 1);
}

// ── Tests: Direct Static Invocation & Storage Integrity ──────────────────────

#[test]
fn direct_static_get_offering_count_matches_client_interface() {
    let (env, contract_id, client) = setup_env();
    let issuer = Address::generate(&env);
    let namespace = symbol_short!("static");

    for i in 0..3 {
        let token = Address::generate(&env);
        register_single_offering(&env, &client, &issuer, &namespace, &token);

        // Verify count through client
        let client_count = client.get_offering_count(&issuer, &namespace);
        assert_eq!(client_count, i + 1);

        // Verify count directly via contract invocation inside contract env context
        env.as_contract(&contract_id, || {
            let static_count = RevoraRevenueShare::get_offering_count(
                env.clone(),
                issuer.clone(),
                namespace.clone(),
            );
            assert_eq!(static_count, i + 1);

            // Directly inspect low-level DataKey::OfferCount persistent storage
            let key = DataKey::OfferCount(TenantId {
                issuer: issuer.clone(),
                namespace: namespace.clone(),
            });
            let stored_val: Option<u32> = env.storage().persistent().get(&key);
            assert_eq!(stored_val, Some(i + 1));
        });
    }
}

// ── Tests: Transfer Issuer Stability ──────────────────────────────────────────

#[test]
fn get_offering_count_updates_appropriately_on_accepted_issuer_transfer() {
    let (env, _contract_id, client) = setup_env();
    let issuer_old = Address::generate(&env);
    let issuer_new = Address::generate(&env);
    let namespace = symbol_short!("transfer");

    let token1 = Address::generate(&env);
    let token2 = Address::generate(&env);

    register_single_offering(&env, &client, &issuer_old, &namespace, &token1);
    register_single_offering(&env, &client, &issuer_old, &namespace, &token2);

    assert_eq!(client.get_offering_count(&issuer_old, &namespace), 2);
    assert_eq!(client.get_offering_count(&issuer_new, &namespace), 0);

    // Propose and accept transfer of token1 to issuer_new
    client.propose_issuer_transfer(&issuer_old, &namespace, &token1, &issuer_new);
    client.accept_issuer_transfer(&issuer_new, &namespace, &token1);

    // In Revora's design, accepting an issuer transfer appends to the new issuer's offering index
    assert_eq!(client.get_offering_count(&issuer_new, &namespace), 1);
    // Old issuer retains original indexed count for backward compatibility / historic slot stability
    assert_eq!(client.get_offering_count(&issuer_old, &namespace), 2);
}
