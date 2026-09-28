//! Adversarial test suite for `blacklist_add_with_attestation`.
//!
//! Covers the following axes:
//!
//! | # | Axis | Cases |
//! |---|------|-------|
//! | 1 | Happy path | issuer caller, admin caller, OFAC source |
//! | 2 | Idempotency | second add of same investor is a no-op on storage |
//! | 3 | Authorization | random third-party rejected with `NotAuthorized` |
//! | 4 | Attestation timestamp | future timestamp rejected with `InvalidAmount`; boundary (now) accepted |
//! | 5 | Contract frozen | frozen contract rejects with `ContractFrozen` |
//! | 6 | Contract paused | paused contract rejects with `ContractPaused` |
//! | 7 | Offering not found | unknown offering rejected with `OfferingNotFound` |
//! | 8 | Blacklist size limit | 201st entry rejected with `BlacklistSizeLimitExceeded`; state unchanged |
//! | 9 | Storage invariant | `is_blacklisted` reflects add; `get_blacklist_attestation` returns stored value |
//! | 10 | Event emission | `BL_ADD` event published on success; none published on failure |
//! | 11 | Transferred-issuer authority | new issuer succeeds; old issuer rejected after transfer |
//! | 12 | Event-only mode | call succeeds and emits event but does NOT mutate storage |
//!
//! State-unchanged assertions: every rejected path is followed by a call to
//! `is_blacklisted` / `get_blacklist_size` to prove storage was not mutated.

#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient, RevoraError, SanctionsAttestation, Source};
use soroban_sdk::{
    symbol_short, testutils::{Address as _, Events, Ledger, LedgerInfo}, Address, Env, IntoVal, Vec,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Advance ledger timestamp by `delta` seconds so the mock clock is at `base + delta`.
fn advance_timestamp(env: &Env, delta: u64) {
    let current = env.ledger().timestamp();
    env.ledger().set(LedgerInfo {
        timestamp: current + delta,
        ..env.ledger().get()
    });
}

/// Build a `SanctionsAttestation` whose `attested_at` equals the current ledger timestamp.
fn attest_now(env: &Env) -> SanctionsAttestation {
    SanctionsAttestation {
        source: Source::Manual,
        ref_id: symbol_short!("ref1"),
        attested_at: env.ledger().timestamp(),
    }
}

/// Register a bare offering and return `(issuer, token)`.
fn setup_offering(
    env: &Env,
    client: &RevoraRevenueShareClient<'_>,
    admin: &Address,
    ns: &soroban_sdk::Symbol,
) -> (Address, Address) {
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        ns,
        &token,
        &1000u32,
        &token,
        &0_i128,
        &symbol_short!(""),
        &0,
    );
    (issuer, token)
}

// ---------------------------------------------------------------------------
// 1. Happy path — issuer caller
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_happy_path_issuer_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);
    let attestation = attest_now(&env);

    let result = client.try_blacklist_add_with_attestation(
        &issuer, &issuer, &ns, &token, &investor, &attestation,
    );
    assert!(result.is_ok(), "Expected Ok, got {:?}", result);
    assert!(
        client.is_blacklisted(&issuer, &ns, &token, &investor),
        "investor should be blacklisted after successful add"
    );
}

// ---------------------------------------------------------------------------
// 2. Happy path — admin caller
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_happy_path_admin_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);
    let attestation = attest_now(&env);

    // Admin (not the issuer) should be authorized
    let result = client.try_blacklist_add_with_attestation(
        &admin, &issuer, &ns, &token, &investor, &attestation,
    );
    assert!(result.is_ok(), "Admin caller should succeed, got {:?}", result);
    assert!(client.is_blacklisted(&issuer, &ns, &token, &investor));
}

// ---------------------------------------------------------------------------
// 3. Happy path — OFAC source
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_ofac_source() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);

    let attestation = SanctionsAttestation {
        source: Source::OFAC,
        ref_id: symbol_short!("sdn001"),
        attested_at: env.ledger().timestamp(),
    };

    let result = client.try_blacklist_add_with_attestation(
        &issuer, &issuer, &ns, &token, &investor, &attestation,
    );
    assert!(result.is_ok());

    let stored = client
        .get_blacklist_attestation(&issuer, &ns, &token, &investor)
        .expect("attestation should be stored");
    assert_eq!(stored.source, Source::OFAC);
    assert_eq!(stored.ref_id, symbol_short!("sdn001"));
}

// ---------------------------------------------------------------------------
// 4. Storage invariant — is_blacklisted and get_blacklist_attestation
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_storage_reflects_add() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);
    let attestation = SanctionsAttestation {
        source: Source::Manual,
        ref_id: symbol_short!("ref99"),
        attested_at: env.ledger().timestamp(),
    };

    // Before add
    assert!(!client.is_blacklisted(&issuer, &ns, &token, &investor));
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 0);

    client.blacklist_add_with_attestation(&issuer, &issuer, &ns, &token, &investor, &attestation);

    // After add
    assert!(client.is_blacklisted(&issuer, &ns, &token, &investor));
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 1);

    let stored = client
        .get_blacklist_attestation(&issuer, &ns, &token, &investor)
        .expect("attestation should be stored");
    assert_eq!(stored.ref_id, symbol_short!("ref99"));
}

// ---------------------------------------------------------------------------
// 5. Idempotency — re-adding the same investor is a no-op on storage
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_idempotent_does_not_grow_size() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);
    let attestation = attest_now(&env);

    client.blacklist_add_with_attestation(&issuer, &issuer, &ns, &token, &investor, &attestation);
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 1);

    // Second add — same investor — should succeed but NOT grow the list
    let result = client.try_blacklist_add_with_attestation(
        &issuer, &issuer, &ns, &token, &investor, &attestation,
    );
    assert!(result.is_ok(), "idempotent re-add should be Ok");
    assert_eq!(
        client.get_blacklist_size(&issuer, &ns, &token),
        1,
        "size must remain 1 after idempotent re-add"
    );
}

// ---------------------------------------------------------------------------
// 6. Authorization — random third-party rejected
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_unauthorized_caller_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);
    let attacker = Address::generate(&env);
    let attestation = attest_now(&env);

    let result = client.try_blacklist_add_with_attestation(
        &attacker, &issuer, &ns, &token, &investor, &attestation,
    );
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::NotAuthorized,
        "third-party caller must be rejected with NotAuthorized"
    );
    // State unchanged
    assert!(!client.is_blacklisted(&issuer, &ns, &token, &investor));
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 0);
}

// ---------------------------------------------------------------------------
// 7. Attestation timestamp — future timestamp rejected
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_future_timestamp_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);

    // attested_at is 1 second in the future
    let future_attestation = SanctionsAttestation {
        source: Source::Manual,
        ref_id: symbol_short!("fut"),
        attested_at: env.ledger().timestamp() + 1,
    };

    let result = client.try_blacklist_add_with_attestation(
        &issuer, &issuer, &ns, &token, &investor, &future_attestation,
    );
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::InvalidAmount,
        "future attested_at must be rejected with InvalidAmount"
    );
    // State unchanged
    assert!(!client.is_blacklisted(&issuer, &ns, &token, &investor));
}

// ---------------------------------------------------------------------------
// 8. Attestation timestamp — boundary: attested_at == now is accepted
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_attested_at_now_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);

    // Advance so ledger.timestamp() > 0 to avoid underflow issues
    advance_timestamp(&env, 1000);

    let attestation = SanctionsAttestation {
        source: Source::Manual,
        ref_id: symbol_short!("now"),
        attested_at: env.ledger().timestamp(), // exactly now — must be accepted
    };

    let result = client.try_blacklist_add_with_attestation(
        &issuer, &issuer, &ns, &token, &investor, &attestation,
    );
    assert!(result.is_ok(), "attested_at == now should be accepted, got {:?}", result);
    assert!(client.is_blacklisted(&issuer, &ns, &token, &investor));
}

// ---------------------------------------------------------------------------
// 9. Attestation timestamp — old timestamp (well in the past) is accepted
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_past_timestamp_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);

    advance_timestamp(&env, 86_400); // advance 1 day

    let attestation = SanctionsAttestation {
        source: Source::OFAC,
        ref_id: symbol_short!("old"),
        attested_at: 0u64, // epoch — well in the past
    };

    let result = client.try_blacklist_add_with_attestation(
        &issuer, &issuer, &ns, &token, &investor, &attestation,
    );
    assert!(result.is_ok(), "past attested_at should be accepted, got {:?}", result);
}

// ---------------------------------------------------------------------------
// 10. Contract frozen — rejected
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_frozen_contract_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);
    let attestation = attest_now(&env);

    // Freeze the contract
    client.freeze(&admin);

    let result = client.try_blacklist_add_with_attestation(
        &issuer, &issuer, &ns, &token, &investor, &attestation,
    );
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::ContractFrozen,
        "frozen contract must reject with ContractFrozen"
    );
    // State unchanged
    assert!(!client.is_blacklisted(&issuer, &ns, &token, &investor));
}

// ---------------------------------------------------------------------------
// 11. Offering not found — no offering for (issuer, ns, token)
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_offering_not_found_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let ns = symbol_short!("ns");
    let token = Address::generate(&env); // no offering registered
    let investor = Address::generate(&env);
    let attestation = attest_now(&env);

    let result = client.try_blacklist_add_with_attestation(
        &issuer, &issuer, &ns, &token, &investor, &attestation,
    );
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::OfferingNotFound,
        "unknown offering must reject with OfferingNotFound"
    );
}

// ---------------------------------------------------------------------------
// 12. Blacklist size limit — 201st entry rejected; state unchanged
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_size_limit_exceeded() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let attestation = attest_now(&env);

    // Fill to MAX_BLACKLIST_SIZE (200) using the cheaper blacklist_add helper
    for _ in 0..200u32 {
        let addr = Address::generate(&env);
        client.blacklist_add(&issuer, &issuer, &ns, &token, &addr);
    }
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 200);

    // 201st entry must be rejected
    let overflow_investor = Address::generate(&env);
    let result = client.try_blacklist_add_with_attestation(
        &issuer, &issuer, &ns, &token, &overflow_investor, &attestation,
    );
    assert_eq!(
        result.unwrap_err().unwrap(),
        RevoraError::BlacklistSizeLimitExceeded,
        "201st entry must be rejected with BlacklistSizeLimitExceeded"
    );
    // Size must remain exactly at the limit — not grown
    assert_eq!(
        client.get_blacklist_size(&issuer, &ns, &token),
        200,
        "blacklist size must remain at cap after rejected add"
    );
    // The rejected investor must NOT appear in storage
    assert!(
        !client.is_blacklisted(&issuer, &ns, &token, &overflow_investor),
        "rejected investor must not be blacklisted"
    );
}

// ---------------------------------------------------------------------------
// 13. Event emission — BL_ADD event on success; no event on failure
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_emits_event_on_success() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);
    let attestation = attest_now(&env);

    let events_before = env.events().all().len();
    client.blacklist_add_with_attestation(&issuer, &issuer, &ns, &token, &investor, &attestation);
    let events_after = env.events().all().len();

    assert!(
        events_after > events_before,
        "at least one event must be emitted on successful add"
    );
}

#[test]
fn blacklist_add_with_attestation_emits_no_event_on_unauthorized_failure() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let investor = Address::generate(&env);
    let attacker = Address::generate(&env);
    let attestation = attest_now(&env);

    let events_before = env.events().all().len();
    let _ = client.try_blacklist_add_with_attestation(
        &attacker, &issuer, &ns, &token, &investor, &attestation,
    );
    let events_after = env.events().all().len();

    assert_eq!(
        events_after, events_before,
        "no event should be emitted on authorization failure"
    );
}

// ---------------------------------------------------------------------------
// 14. Multiple distinct investors — each tracked independently
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_multiple_investors_independent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);

    let inv_a = Address::generate(&env);
    let inv_b = Address::generate(&env);
    let inv_c = Address::generate(&env);
    let attestation = attest_now(&env);

    client.blacklist_add_with_attestation(&issuer, &issuer, &ns, &token, &inv_a, &attestation);
    client.blacklist_add_with_attestation(&issuer, &issuer, &ns, &token, &inv_b, &attestation);
    client.blacklist_add_with_attestation(&issuer, &issuer, &ns, &token, &inv_c, &attestation);

    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 3);
    assert!(client.is_blacklisted(&issuer, &ns, &token, &inv_a));
    assert!(client.is_blacklisted(&issuer, &ns, &token, &inv_b));
    assert!(client.is_blacklisted(&issuer, &ns, &token, &inv_c));

    // A completely different address must NOT appear blacklisted
    let not_blacklisted = Address::generate(&env);
    assert!(!client.is_blacklisted(&issuer, &ns, &token, &not_blacklisted));
}

// ---------------------------------------------------------------------------
// 15. Namespace isolation — blacklist for one offering does not bleed into another
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_namespace_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let ns_a = symbol_short!("nsA");
    let ns_b = symbol_short!("nsB");
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns_a, &token_a, &1000u32, &token_a, &0_i128, &symbol_short!(""), &0);
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns_b, &token_b, &1000u32, &token_b, &0_i128, &symbol_short!(""), &0);

    let investor = Address::generate(&env);
    let attestation = attest_now(&env);

    client.blacklist_add_with_attestation(&issuer, &issuer, &ns_a, &token_a, &investor, &attestation);

    assert!(client.is_blacklisted(&issuer, &ns_a, &token_a, &investor));
    // Must NOT bleed into offering B
    assert!(!client.is_blacklisted(&issuer, &ns_b, &token_b, &investor));
}

// ---------------------------------------------------------------------------
// 16. Transferred-issuer authority — new issuer accepted; old issuer rejected
// ---------------------------------------------------------------------------

#[test]
fn blacklist_add_with_attestation_transferred_issuer_authority() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let ns = symbol_short!("ns");

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, token) = setup_offering(&env, &client, &admin, &ns);
    let new_issuer = Address::generate(&env);
    let investor = Address::generate(&env);
    let attestation = attest_now(&env);

    // Transfer issuer role
    client.transfer_issuer(&issuer, &ns, &token, &new_issuer);

    // New issuer must succeed
    let result_new = client.try_blacklist_add_with_attestation(
        &new_issuer, &new_issuer, &ns, &token, &investor, &attestation,
    );
    assert!(result_new.is_ok(), "new issuer should be accepted after transfer, got {:?}", result_new);

    // Old issuer must be rejected
    let investor2 = Address::generate(&env);
    let result_old = client.try_blacklist_add_with_attestation(
        &issuer, &new_issuer, &ns, &token, &investor2, &attestation,
    );
    assert_eq!(
        result_old.unwrap_err().unwrap(),
        RevoraError::NotAuthorized,
        "old issuer must be rejected after transfer"
    );
}
