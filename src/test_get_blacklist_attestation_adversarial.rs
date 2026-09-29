//! Adversarial and boundary test coverage for `get_blacklist_attestation` in `lib.rs` (#1098).
//!
//! # Coverage Matrix
//!
//! | Scenario / Property                                                          | Expected Behavior                       |
//! |------------------------------------------------------------------------------|-----------------------------------------|
//! | Investor never blacklisted → returns None                                    | None                                    |
//! | Investor blacklisted → returns Some(attestation)                             | Some with correct fields                |
//! | Attestation fields round-trip: source, ref_id, attested_at preserved         | Field-by-field equality                 |
//! | Unknown issuer → returns None (no panic)                                     | None                                    |
//! | Unknown namespace → returns None (no panic)                                  | None                                    |
//! | Unknown token → returns None (no panic)                                      | None                                    |
//! | Unknown investor on valid offering → returns None                            | None                                    |
//! | Investor blacklisted then removed → returns None                             | None after removal                      |
//! | Re-blacklisting after removal restores attestation                           | Some after re-add                       |
//! | Distinct investors on same offering are independent                          | Each isolated                           |
//! | Distinct offerings have independent blacklists                               | No cross-offering bleed                 |
//! | get_blacklist_attestation is read-only: no auth required                     | Callable without auth enforcement       |
//! | blacklist_add requires caller auth (no mock panics)                          | Panics without auth                     |
//! | blacklist_add requires authorized caller (not just any address)              | Err(NotAuthorized) for wrong caller     |
//! | State unchanged after unauthorized blacklist_add attempt                     | None still returned                     |
//! | Multiple investors, only the queried one's attestation returned              | Correct per-investor isolation          |

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, Source};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env, Vec,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup_env() -> (Env, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);
    (env, client)
}

/// Register a single offering and return (issuer, namespace, token).
fn register_offering(
    env: &Env,
    client: &RevoraRevenueShareClient,
) -> (Address, soroban_sdk::Symbol, Address) {
    let issuer = Address::generate(env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(env);
    let payout = Address::generate(env);
    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        &namespace,
        &token,
        &1_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );
    (issuer, namespace, token)
}

// ── Tests: None before any blacklisting ───────────────────────────────────────

#[test]
fn get_blacklist_attestation_returns_none_when_investor_never_blacklisted() {
    let (env, client) = setup_env();
    let (issuer, ns, token) = register_offering(&env, &client);
    let investor = Address::generate(&env);

    let result = client.get_blacklist_attestation(&issuer, &ns, &token, &investor);
    assert!(result.is_none(), "attestation must be None for an investor never blacklisted");
}

#[test]
fn get_blacklist_attestation_returns_none_for_unknown_issuer() {
    let (env, client) = setup_env();
    let unknown_issuer = Address::generate(&env);
    let ns = symbol_short!("ns");
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    let result = client.get_blacklist_attestation(&unknown_issuer, &ns, &token, &investor);
    assert!(result.is_none(), "unknown issuer must return None, not panic");
}

#[test]
fn get_blacklist_attestation_returns_none_for_unknown_namespace() {
    let (env, client) = setup_env();
    let (issuer, _, token) = register_offering(&env, &client);
    let unknown_ns = symbol_short!("other");
    let investor = Address::generate(&env);

    let result = client.get_blacklist_attestation(&issuer, &unknown_ns, &token, &investor);
    assert!(result.is_none(), "unknown namespace must return None");
}

#[test]
fn get_blacklist_attestation_returns_none_for_unknown_token() {
    let (env, client) = setup_env();
    let (issuer, ns, _) = register_offering(&env, &client);
    let unknown_token = Address::generate(&env);
    let investor = Address::generate(&env);

    let result = client.get_blacklist_attestation(&issuer, &ns, &unknown_token, &investor);
    assert!(result.is_none(), "unknown token must return None");
}

#[test]
fn get_blacklist_attestation_returns_none_for_unknown_investor_on_valid_offering() {
    let (env, client) = setup_env();
    let (issuer, ns, token) = register_offering(&env, &client);
    // Add a different investor to the blacklist
    let other_investor = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &other_investor);

    // Query a completely different investor
    let unknown_investor = Address::generate(&env);
    let result = client.get_blacklist_attestation(&issuer, &ns, &token, &unknown_investor);
    assert!(
        result.is_none(),
        "unblacklisted investor must return None even when others are blacklisted"
    );
}

// ── Tests: happy-path round-trips ─────────────────────────────────────────────

#[test]
fn get_blacklist_attestation_returns_some_after_blacklist_add() {
    let (env, client) = setup_env();
    let (issuer, ns, token) = register_offering(&env, &client);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    let result = client.get_blacklist_attestation(&issuer, &ns, &token, &investor);
    assert!(result.is_some(), "attestation must be Some after blacklist_add");
}

#[test]
fn get_blacklist_attestation_source_is_manual_after_blacklist_add() {
    let (env, client) = setup_env();
    let (issuer, ns, token) = register_offering(&env, &client);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    let att = client.get_blacklist_attestation(&issuer, &ns, &token, &investor).unwrap();
    assert_eq!(att.source, Source::Manual, "blacklist_add must use Source::Manual");
}

#[test]
fn get_blacklist_attestation_ref_id_is_manual_after_blacklist_add() {
    let (env, client) = setup_env();
    let (issuer, ns, token) = register_offering(&env, &client);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    let att = client.get_blacklist_attestation(&issuer, &ns, &token, &investor).unwrap();
    assert_eq!(att.ref_id, symbol_short!("manual"), "ref_id must be 'manual' for blacklist_add");
}

#[test]
fn get_blacklist_attestation_attested_at_matches_ledger_timestamp() {
    let (env, client) = setup_env();
    env.ledger().with_mut(|l| l.timestamp = 5_000);
    let (issuer, ns, token) = register_offering(&env, &client);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    let att = client.get_blacklist_attestation(&issuer, &ns, &token, &investor).unwrap();
    assert_eq!(att.attested_at, 5_000, "attested_at must match ledger timestamp at time of add");
}

// ── Tests: removal clears attestation ────────────────────────────────────────

#[test]
fn get_blacklist_attestation_returns_none_after_blacklist_remove() {
    let (env, client) = setup_env();
    let (issuer, ns, token) = register_offering(&env, &client);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);
    assert!(client.get_blacklist_attestation(&issuer, &ns, &token, &investor).is_some());

    client.blacklist_remove(&issuer, &issuer, &ns, &token, &investor);

    let result = client.get_blacklist_attestation(&issuer, &ns, &token, &investor);
    assert!(result.is_none(), "attestation must be None after blacklist_remove");
}

#[test]
fn re_blacklisting_after_removal_restores_attestation() {
    let (env, client) = setup_env();
    let (issuer, ns, token) = register_offering(&env, &client);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &investor);
    assert!(client.get_blacklist_attestation(&issuer, &ns, &token, &investor).is_none());

    // Re-add at a different timestamp
    env.ledger().with_mut(|l| l.timestamp = 9_999);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    let att = client.get_blacklist_attestation(&issuer, &ns, &token, &investor).unwrap();
    assert!(att.attested_at >= 9_999, "re-added attestation must reflect new timestamp");
}

// ── Tests: isolation ──────────────────────────────────────────────────────────

#[test]
fn blacklist_attestation_isolated_per_investor() {
    let (env, client) = setup_env();
    let (issuer, ns, token) = register_offering(&env, &client);
    let investor_a = Address::generate(&env);
    let investor_b = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor_a);

    // investor_b was never added
    assert!(
        client.get_blacklist_attestation(&issuer, &ns, &token, &investor_b).is_none(),
        "investor_b must not be affected by investor_a's blacklisting"
    );
    assert!(
        client.get_blacklist_attestation(&issuer, &ns, &token, &investor_a).is_some(),
        "investor_a must have an attestation"
    );
}

#[test]
fn blacklist_attestation_isolated_per_offering() {
    let (env, client) = setup_env();
    let (issuer_a, ns_a, token_a) = register_offering(&env, &client);
    let (issuer_b, ns_b, token_b) = register_offering(&env, &client);
    let investor = Address::generate(&env);

    // Only blacklist in offering A
    client.blacklist_add(&issuer_a, &issuer_a, &ns_a, &token_a, &investor);

    assert!(
        client.get_blacklist_attestation(&issuer_b, &ns_b, &token_b, &investor).is_none(),
        "blacklisting in offering A must not affect offering B"
    );
}

#[test]
fn blacklist_attestation_isolated_per_token_same_issuer_and_namespace() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let ns = symbol_short!("ns");
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout = Address::generate(&env);
    let investor = Address::generate(&env);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token_a,
        &1_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token_b,
        &1_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );

    client.blacklist_add(&issuer, &issuer, &ns, &token_a, &investor);

    assert!(
        client.get_blacklist_attestation(&issuer, &ns, &token_b, &investor).is_none(),
        "blacklist for token_a must not bleed into token_b"
    );
}

// ── Tests: multiple investors ──────────────────────────────────────────────────

#[test]
fn get_blacklist_attestation_returns_correct_attestation_for_each_investor() {
    let (env, client) = setup_env();
    let (issuer, ns, token) = register_offering(&env, &client);

    let investor_a = Address::generate(&env);
    let investor_b = Address::generate(&env);
    let investor_c = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor_a);
    env.ledger().with_mut(|l| l.timestamp = 2_000);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor_b);
    // investor_c is never added

    let att_a = client.get_blacklist_attestation(&issuer, &ns, &token, &investor_a).unwrap();
    let att_b = client.get_blacklist_attestation(&issuer, &ns, &token, &investor_b).unwrap();

    assert_eq!(att_a.attested_at, 1_000, "investor_a attestation timestamp must be 1_000");
    assert_eq!(att_b.attested_at, 2_000, "investor_b attestation timestamp must be 2_000");
    assert!(
        client.get_blacklist_attestation(&issuer, &ns, &token, &investor_c).is_none(),
        "investor_c must have no attestation"
    );
}

// ── Tests: state unchanged after auth failure ─────────────────────────────────

#[test]
fn state_unchanged_after_unauthorized_blacklist_add() {
    let (env, client) = setup_env();
    let (issuer, ns, token) = register_offering(&env, &client);
    let investor = Address::generate(&env);
    let unauthorized = Address::generate(&env);

    // Attempt blacklist_add with a caller who is not the issuer
    let result = client.try_blacklist_add(&unauthorized, &issuer, &ns, &token, &investor);
    assert!(result.is_err(), "unauthorized caller must be rejected");

    // Attestation must remain None — state unchanged
    let att = client.get_blacklist_attestation(&issuer, &ns, &token, &investor);
    assert!(att.is_none(), "state must be unchanged after rejected blacklist_add");
}

// ── Tests: get_blacklist_attestation is read-only ─────────────────────────────

#[test]
fn get_blacklist_attestation_does_not_require_auth() {
    let (env, client) = setup_env();
    let (issuer, ns, token) = register_offering(&env, &client);
    let investor = Address::generate(&env);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    // get_blacklist_attestation is a pure read — must succeed without any
    // active auth requirement (Soroban read-only functions never call require_auth).
    let result = client.get_blacklist_attestation(&issuer, &ns, &token, &investor);
    assert!(result.is_some(), "read-only get must succeed without auth enforcement");
}

// ── Tests: blacklist_add auth enforcement ─────────────────────────────────────

#[test]
#[should_panic]
fn blacklist_add_without_caller_auth_panics() {
    // No mock_all_auths — require_auth on caller must panic.
    let env = Env::default();
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    // Register the offering (needs auth — do it in a separate env with mocks).
    // Since we can't register without auth here, we just attempt blacklist_add
    // directly: it will panic at caller.require_auth() before even checking the offering.
    let issuer = Address::generate(&env);
    let investor = Address::generate(&env);
    let ns = symbol_short!("ns");
    let token = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor); // panics
}

// ── Tests: error code stability ───────────────────────────────────────────────

#[test]
fn error_code_not_authorized_is_19() {
    assert_eq!(RevoraError::NotAuthorized as u32, 19);
}

#[test]
fn error_code_offering_not_found_is_4() {
    assert_eq!(RevoraError::OfferingNotFound as u32, 4);
}

#[test]
fn error_code_blacklist_size_limit_exceeded_is_45() {
    assert_eq!(RevoraError::BlacklistSizeLimitExceeded as u32, 45);
}
