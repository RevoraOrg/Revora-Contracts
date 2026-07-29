#![cfg(test)]

use crate::{BlacklistEntryMeta, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::Events,
    Address, BytesN, Env, Vec,
};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn setup_offering(
    env: &Env,
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    token: &Address,
    ns: &soroban_sdk::Symbol,
) {
    client.initialize(issuer, &None::<Address>, &None::<bool>);
    client.register_offering(issuer, ns, token, &1000u32, token, &0_i128);
}

/// Helper to create a deterministic 32-byte hash for testing.
fn make_snapshot_hash(env: &Env, seed: u8) -> BytesN<32> {
    let input = soroban_sdk::Bytes::from_array(env, &[seed; 32]);
    env.crypto().sha256(&input)
}

/// Verifies that `blacklist_add_pinned` persists the snapshot hash and timestamp,
/// and emits the correct `bl_add_pn` event.
#[test]
fn test_blacklist_add_pinned_persists_meta() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);
    let ns = symbol_short!("def");
    let snapshot_hash = make_snapshot_hash(&env, 1);

    setup_offering(&env, &client, &issuer, &token, &ns);

    let attestation = crate::SanctionsAttestation {
        source: crate::Source::OFAC,
        ref_id: symbol_short!("list_v1"),
        attested_at: env.ledger().timestamp(),
    };

    // Add with snapshot hash
    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &ns,
        &token,
        &investor,
        &attestation,
        &snapshot_hash,
    );

    // Verify the investor is blacklisted
    assert!(client.is_blacklisted(&issuer, &ns, &token, &investor));

    // Verify the entry meta is persisted
    let meta = client.get_blacklist_entry_meta(&issuer, &ns, &token, &investor);
    assert!(meta.is_some(), "BlacklistEntryMeta should be persisted");
    let meta = meta.unwrap();
    assert_eq!(
        meta.snapshot_hash, snapshot_hash,
        "snapshot_hash should match the one provided"
    );
    assert_eq!(
        meta.added_ts,
        env.ledger().timestamp(),
        "added_ts should match the ledger timestamp"
    );

    // Verify the event was emitted with bl_add_pn symbol
    let all_events = env.events().all();
    let mut found_pinned_event = false;
    for i in 0..all_events.len() {
        let ev = all_events.get(i).unwrap();
        let topics = ev.topics();
        if !topics.is_empty() {
            let sym: soroban_sdk::Symbol = topics.get(0).unwrap();
            if sym == symbol_short!("bl_add_pn") {
                found_pinned_event = true;
                // Event data is (caller, investor, attestation, snapshot_hash)
                let data = ev.data();
                let event_caller: Address = data.get(0).unwrap();
                let event_investor: Address = data.get(1).unwrap();
                let event_hash: BytesN<32> = data.get(3).unwrap();
                assert_eq!(event_caller, issuer);
                assert_eq!(event_investor, investor);
                assert_eq!(event_hash, snapshot_hash);
            }
        }
    }
    assert!(found_pinned_event, "Expected bl_add_pn event to be emitted");
}

/// Verifies that `blacklist_add_pinned` is idempotent (duplicate calls succeed
/// without changing the stored meta).
#[test]
fn test_blacklist_add_pinned_idempotent() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);
    let ns = symbol_short!("def");
    let hash_a = make_snapshot_hash(&env, 1);
    let hash_b = make_snapshot_hash(&env, 2);

    setup_offering(&env, &client, &issuer, &token, &ns);

    let attestation = crate::SanctionsAttestation {
        source: crate::Source::OFAC,
        ref_id: symbol_short!("list_v1"),
        attested_at: env.ledger().timestamp(),
    };

    // First add with hash_a
    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &ns,
        &token,
        &investor,
        &attestation,
        &hash_a,
    );

    // Second add with hash_b - should not overwrite (idempotent)
    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &ns,
        &token,
        &investor,
        &attestation,
        &hash_b,
    );

    // Verify the meta still has hash_a (first add is persisted)
    let meta = client.get_blacklist_entry_meta(&issuer, &ns, &token, &investor);
    assert!(meta.is_some());
    assert_eq!(meta.unwrap().snapshot_hash, hash_a);

    // Verify only ONE entry in the blacklist
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 1);
}

/// Verifies that two different investors can have different snapshot hashes pinned.
#[test]
fn test_blacklist_add_pinned_two_entries_different_hashes() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor_a = Address::generate(&env);
    let investor_b = Address::generate(&env);
    let ns = symbol_short!("def");
    let hash_a = make_snapshot_hash(&env, 1);
    let hash_b = make_snapshot_hash(&env, 2);

    setup_offering(&env, &client, &issuer, &token, &ns);

    let attestation = crate::SanctionsAttestation {
        source: crate::Source::OFAC,
        ref_id: symbol_short!("list_v1"),
        attested_at: env.ledger().timestamp(),
    };

    // Add investor_a with hash_a
    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &ns,
        &token,
        &investor_a,
        &attestation,
        &hash_a,
    );

    // Add investor_b with hash_b
    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &ns,
        &token,
        &investor_b,
        &attestation,
        &hash_b,
    );

    // Verify each has the correct hash
    let meta_a = client.get_blacklist_entry_meta(&issuer, &ns, &token, &investor_a).unwrap();
    let meta_b = client.get_blacklist_entry_meta(&issuer, &ns, &token, &investor_b).unwrap();
    assert_eq!(meta_a.snapshot_hash, hash_a, "investor_a should have hash_a");
    assert_eq!(meta_b.snapshot_hash, hash_b, "investor_b should have hash_b");

    // Verify both entries are in the blacklist
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 2);
}

/// Verifies that removing a blacklisted entry also removes the pinned meta.
#[test]
fn test_blacklist_remove_cleans_up_meta() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);
    let ns = symbol_short!("def");
    let snapshot_hash = make_snapshot_hash(&env, 1);

    setup_offering(&env, &client, &issuer, &token, &ns);

    let attestation = crate::SanctionsAttestation {
        source: crate::Source::OFAC,
        ref_id: symbol_short!("list_v1"),
        attested_at: env.ledger().timestamp(),
    };

    // Add with snapshot hash
    client.blacklist_add_pinned(
        &issuer,
        &issuer,
        &ns,
        &token,
        &investor,
        &attestation,
        &snapshot_hash,
    );

    // Verify meta exists
    assert!(client.get_blacklist_entry_meta(&issuer, &ns, &token, &investor).is_some());

    // Remove the investor
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &investor);

    // Verify meta is cleaned up
    assert!(
        client.get_blacklist_entry_meta(&issuer, &ns, &token, &investor).is_none(),
        "meta should be removed after blacklist_remove"
    );

    // Verify the blacklist is empty
    assert!(client.get_blacklist(&issuer, &ns, &token).is_empty());
}

/// Verifies that `blacklist_remove_many` cleans up meta for all removed entries.
#[test]
fn test_blacklist_remove_many_cleans_up_meta() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor_a = Address::generate(&env);
    let investor_b = Address::generate(&env);
    let ns = symbol_short!("def");
    let hash_a = make_snapshot_hash(&env, 1);
    let hash_b = make_snapshot_hash(&env, 2);

    setup_offering(&env, &client, &issuer, &token, &ns);

    let attestation = crate::SanctionsAttestation {
        source: crate::Source::OFAC,
        ref_id: symbol_short!("list_v1"),
        attested_at: env.ledger().timestamp(),
    };

    client.blacklist_add_pinned(&issuer, &issuer, &ns, &token, &investor_a, &attestation, &hash_a);
    client.blacklist_add_pinned(&issuer, &issuer, &ns, &token, &investor_b, &attestation, &hash_b);

    // Verify both have meta
    assert!(client.get_blacklist_entry_meta(&issuer, &ns, &token, &investor_a).is_some());
    assert!(client.get_blacklist_entry_meta(&issuer, &ns, &token, &investor_b).is_some());

    // Remove both
    let investors: Vec<Address> = Vec::from_array(&env, [investor_a.clone(), investor_b.clone()]);
    client.blacklist_remove_many(&issuer, &issuer, &ns, &token, &investors);

    // Verify both meta are cleaned
    assert!(client.get_blacklist_entry_meta(&issuer, &ns, &token, &investor_a).is_none());
    assert!(client.get_blacklist_entry_meta(&issuer, &ns, &token, &investor_b).is_none());
}

/// Verifies that `get_blacklist_entry_meta` returns `None` for addresses
/// added via the regular (non-pinned) `blacklist_add`.
#[test]
fn test_regular_blacklist_add_has_no_meta() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);
    let ns = symbol_short!("def");

    setup_offering(&env, &client, &issuer, &token, &ns);

    // Add via regular blacklist_add (no snapshot hash)
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    // Verify no meta exists for this entry
    assert!(
        client.get_blacklist_entry_meta(&issuer, &ns, &token, &investor).is_none(),
        "regular blacklist_add should not create BlacklistEntryMeta"
    );
}
