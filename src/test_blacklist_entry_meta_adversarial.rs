//! Adversarial coverage for `get_blacklist_entry_meta` (#1102).
//!
//! `get_blacklist_entry_meta` is the compliance-facing read path that links a
//! blacklist entry back to the signed off-chain OFAC snapshot it was taken from.
//! It must return `Some(BlacklistEntryMeta)` **only** for entries created by
//! `blacklist_add_pinned`, must never leak an entry across offering boundaries,
//! and must be cleaned up together with the blacklist entry itself.  The
//! rejection paths that guard the writer side are pinned here too, because a
//! rejected write that still mutates the meta map would be a compliance bug.
//!
//! Coverage matrix
//!
//! | Scenario                                                     | Expected                                     |
//! |--------------------------------------------------------------|----------------------------------------------|
//! | No blacklist activity at all                                  | `None`                                        |
//! | Entry added via plain `blacklist_add` (no snapshot)           | `None`                                        |
//! | Entry added via `blacklist_add_pinned`                        | `Some`, hash + ledger timestamp match         |
//! | Duplicate pinned add with a different hash                    | original meta retained, blacklist size is 1   |
//! | Sibling offering / different namespace / different issuer     | `None` (no cross-offering leakage)            |
//! | `blacklist_remove`                                            | entry and meta both cleared                   |
//! | `blacklist_remove_many`                                       | meta cleared for every removed investor       |
//! | Re-pin after removal                                          | fresh meta recorded                           |
//! | Pinned add with a future attestation                          | `InvalidAmount`, no entry, no meta            |
//! | Pinned add from a non-issuer caller                           | `NotAuthorized`, no entry, no meta            |

#![cfg(test)]

use crate::{
    RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, SanctionsAttestation, Source,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Vec,
};

/// Deterministic 32-byte snapshot hash so assertions never depend on randomness.
fn snapshot_hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn attestation(env: &Env) -> SanctionsAttestation {
    SanctionsAttestation {
        source: Source::OFAC,
        ref_id: symbol_short!("list_v1"),
        attested_at: env.ledger().timestamp(),
    }
}

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

/// Register a second offering for the same issuer + namespace under a new token.
fn register_sibling(client: &RevoraRevenueShareClient<'_>, env: &Env, issuer: &Address) -> Address {
    let token_b = Address::generate(env);
    let payout_b = Address::generate(env);
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        &symbol_short!("def"),
        &token_b,
        &5_000,
        &payout_b,
        &0,
        &symbol_short!(""),
        &0u32,
    );
    token_b
}

#[test]
fn meta_is_none_before_any_blacklist_activity() {
    let (env, client, issuer, token) = setup_offering();
    let investor = Address::generate(&env);
    let unknown = Address::generate(&env);

    assert!(client
        .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor)
        .is_none());
    assert!(
        client
            .get_blacklist_entry_meta(&unknown, &symbol_short!("def"), &token, &investor)
            .is_none(),
        "an unknown offering must not expose any pinned metadata"
    );
}

#[test]
fn regular_blacklist_add_records_no_meta() {
    let (env, client, issuer, token) = setup_offering();
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &token, &investor);

    assert!(client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));
    assert!(
        client
            .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor)
            .is_none(),
        "an entry added without a snapshot hash must not expose metadata"
    );
}

#[test]
fn pinned_add_persists_hash_and_ledger_timestamp() {
    let (env, client, issuer, token) = setup_offering();
    let investor = Address::generate(&env);
    let hash = snapshot_hash(&env, 7);

    env.ledger().with_mut(|l| l.timestamp = 1_700_000_000);
    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor,
        &attestation(&env),
        &hash,
    );

    let meta = client
        .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor)
        .expect("a pinned add must persist metadata");
    assert_eq!(meta.snapshot_hash, hash);
    assert_eq!(meta.added_ts, 1_700_000_000, "added_ts must be the ledger timestamp of the add");
    assert!(client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));
}

#[test]
fn duplicate_pinned_add_keeps_the_first_meta() {
    let (env, client, issuer, token) = setup_offering();
    let investor = Address::generate(&env);
    let first_hash = snapshot_hash(&env, 1);
    let second_hash = snapshot_hash(&env, 2);

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor,
        &attestation(&env),
        &first_hash,
    );

    // A later, differently-pinned duplicate must not overwrite the audit record.
    env.ledger().with_mut(|l| l.timestamp = 5_000);
    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor,
        &attestation(&env),
        &second_hash,
    );

    let meta =
        client.get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor).unwrap();
    assert_eq!(meta.snapshot_hash, first_hash, "the original pin must win");
    assert_eq!(meta.added_ts, 1_000, "the original timestamp must win");
    assert_eq!(
        client.get_blacklist(&issuer, &symbol_short!("def"), &token).len(),
        1,
        "an idempotent duplicate must not grow the blacklist"
    );
}

#[test]
fn meta_is_scoped_to_the_exact_offering_identity() {
    let (env, client, issuer, token) = setup_offering();
    let token_b = register_sibling(&client, &env, &issuer);
    let investor = Address::generate(&env);
    let hash = snapshot_hash(&env, 9);

    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor,
        &attestation(&env),
        &hash,
    );

    assert!(client
        .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor)
        .is_some());

    assert!(
        client
            .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token_b, &investor)
            .is_none(),
        "a sibling offering under the same issuer/namespace must not inherit the pin"
    );
    assert!(
        client
            .get_blacklist_entry_meta(&issuer, &symbol_short!("other"), &token, &investor)
            .is_none(),
        "a different namespace must not inherit the pin"
    );

    let other_issuer = Address::generate(&env);
    assert!(
        client
            .get_blacklist_entry_meta(&other_issuer, &symbol_short!("def"), &token, &investor)
            .is_none(),
        "a different issuer must not inherit the pin"
    );
}

#[test]
fn blacklist_remove_clears_the_pinned_meta() {
    let (env, client, issuer, token) = setup_offering();
    let investor = Address::generate(&env);

    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor,
        &attestation(&env),
        &snapshot_hash(&env, 3),
    );
    assert!(client
        .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor)
        .is_some());

    client.blacklist_remove(&issuer, &issuer, &symbol_short!("def"), &token, &investor);

    assert!(!client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));
    assert!(
        client
            .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor)
            .is_none(),
        "removing the entry must also clear the pinned metadata"
    );
}

#[test]
fn blacklist_remove_many_clears_every_pinned_meta() {
    let (env, client, issuer, token) = setup_offering();
    let investor_a = Address::generate(&env);
    let investor_b = Address::generate(&env);

    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor_a,
        &attestation(&env),
        &snapshot_hash(&env, 4),
    );
    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor_b,
        &attestation(&env),
        &snapshot_hash(&env, 5),
    );

    let investors = Vec::from_array(&env, [investor_a.clone(), investor_b.clone()]);
    client.blacklist_remove_many(&issuer, &issuer, &symbol_short!("def"), &token, &investors);

    assert!(client
        .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor_a)
        .is_none());
    assert!(client
        .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor_b)
        .is_none());
    assert!(client.get_blacklist(&issuer, &symbol_short!("def"), &token).is_empty());
}

#[test]
fn re_pinning_after_removal_records_a_fresh_meta() {
    let (env, client, issuer, token) = setup_offering();
    let investor = Address::generate(&env);
    let first_hash = snapshot_hash(&env, 10);
    let second_hash = snapshot_hash(&env, 11);

    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor,
        &attestation(&env),
        &first_hash,
    );
    client.blacklist_remove(&issuer, &issuer, &symbol_short!("def"), &token, &investor);

    env.ledger().with_mut(|l| l.timestamp = 42);
    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor,
        &attestation(&env),
        &second_hash,
    );

    let meta =
        client.get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor).unwrap();
    assert_eq!(meta.snapshot_hash, second_hash, "a fresh pin must replace the old one");
    assert_eq!(meta.added_ts, 42);
}

#[test]
fn pinned_and_regular_entries_coexist_without_cross_contamination() {
    let (env, client, issuer, token) = setup_offering();
    let pinned_investor = Address::generate(&env);
    let regular_investor = Address::generate(&env);

    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &symbol_short!("def"),
        &token,
        &pinned_investor,
        &attestation(&env),
        &snapshot_hash(&env, 12),
    );
    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &token, &regular_investor);

    assert!(client
        .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &pinned_investor)
        .is_some());
    assert!(
        client
            .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &regular_investor)
            .is_none(),
        "a regular entry must not pick up the pinned entry's metadata"
    );
    assert_eq!(client.get_blacklist(&issuer, &symbol_short!("def"), &token).len(), 2);
}

#[test]
fn pending_attestation_is_rejected_and_records_nothing() {
    let (env, client, issuer, token) = setup_offering();
    let investor = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let future = SanctionsAttestation {
        source: Source::OFAC,
        ref_id: symbol_short!("list_v1"),
        attested_at: 1_001,
    };

    assert_eq!(
        client.try_blacklist_add_pinned(
            &issuer,
            &issuer,
            &symbol_short!("def"),
            &token,
            &investor,
            &future,
            &snapshot_hash(&env, 13),
        ),
        Err(Ok(RevoraError::InvalidAmount)),
        "an attestation dated in the future must be refused"
    );

    assert!(!client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));
    assert!(client
        .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor)
        .is_none());
}

#[test]
fn non_issuer_pinned_add_is_rejected_and_records_nothing() {
    let (env, client, issuer, token) = setup_offering();
    let investor = Address::generate(&env);
    let stranger = Address::generate(&env);

    assert_eq!(
        client.try_blacklist_add_pinned(
            &stranger,
            &issuer,
            &symbol_short!("def"),
            &token,
            &investor,
            &attestation(&env),
            &snapshot_hash(&env, 14),
        ),
        Err(Ok(RevoraError::NotAuthorized)),
        "only the issuer or the contract admin may pin a blacklist entry"
    );

    assert!(!client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));
    assert!(client
        .get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor)
        .is_none());
}

#[test]
fn meta_is_none_for_an_offering_with_no_registered_blacklist() {
    let (_env, client, issuer, token) = setup_offering();
    let investor = Address::generate(&_env);

    // Reading before any add is the documented empty-result contract.
    let meta = client.get_blacklist_entry_meta(&issuer, &symbol_short!("def"), &token, &investor);
    assert!(meta.is_none());
}
