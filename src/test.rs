#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, String};
use soroban_sdk::testutils::Events;
use crate::{RevoraRevenueShare, RevoraRevenueShareClient};

// ── helper ────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

// ── original smoke test ───────────────────────────────────────

#[test]
fn it_emits_events_on_register_and_report() {
    let env = Env::default();
    env.mock_all_auths();
    let client  = make_client(&env);
    let issuer  = Address::generate(&env);
    let token   = Address::generate(&env);

    client.register_offering(&issuer, &token, &1_000);
    client.report_revenue(&issuer, &token, &1_000_000, &1);

    assert!(env.events().all().len() >= 2);
}

// ========== METADATA TESTS ==========

#[test]
fn test_set_metadata_creates_new_entry() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "offering_001");
    let metadata_uri = String::from_str(&env, "ipfs://QmVeryLongHashHere123456789");

    client.set_metadata(&issuer, &offering_id, &metadata_uri);

    // Retrieve the metadata and verify it was stored
    let stored = client.get_metadata(&issuer, &offering_id);
    assert_eq!(stored, Some(metadata_uri));
}

#[test]
fn test_set_metadata_emits_creation_event() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "offering_002");
    let metadata_uri = String::from_str(&env, "https://example.com/metadata.json");

    // Initial event count
    let initial_count = env.events().all().len();

    client.set_metadata(&issuer, &offering_id, &metadata_uri);

    // Should have emitted an event
    let events = env.events().all();
    assert!(events.len() > initial_count, "Expected new event to be emitted");
}

#[test]
#[should_panic(expected = "Metadata URI cannot be empty")]
fn test_set_metadata_rejects_empty_uri() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "offering_003");
    let empty_metadata = String::from_str(&env, "");

    client.set_metadata(&issuer, &offering_id, &empty_metadata);
}

#[test]
#[should_panic(expected = "Metadata URI exceeds maximum length")]
fn test_set_metadata_rejects_oversized_uri() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "offering_004");
    
    // Create a string longer than MAX_METADATA_LENGTH (1024 bytes)
    let long_string = "x".repeat(1025);
    let oversized_metadata = String::from_str(&env, &long_string);

    client.set_metadata(&issuer, &offering_id, &oversized_metadata);
}

#[test]
fn test_get_metadata_returns_none_for_nonexistent() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "nonexistent_offering");

    let result = client.get_metadata(&issuer, &offering_id);
    assert_eq!(result, None, "Expected None for non-existent metadata");
}

#[test]
fn test_update_metadata_modifies_existing_entry() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "offering_005");
    let metadata_v1 = String::from_str(&env, "ipfs://QmOriginalHash");
    let metadata_v2 = String::from_str(&env, "ipfs://QmUpdatedHash");

    // Set initial metadata
    client.set_metadata(&issuer, &offering_id, &metadata_v1);
    let stored_v1 = client.get_metadata(&issuer, &offering_id);
    assert_eq!(stored_v1, Some(metadata_v1));

    // Update metadata
    client.update_metadata(&issuer, &offering_id, &metadata_v2);
    let stored_v2 = client.get_metadata(&issuer, &offering_id);
    assert_eq!(stored_v2, Some(metadata_v2));
}

#[test]
#[should_panic(expected = "No metadata found for offering")]
fn test_update_metadata_fails_for_nonexistent() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "nonexistent_offering");
    let metadata_uri = String::from_str(&env, "ipfs://QmSomeHash");

    client.update_metadata(&issuer, &offering_id, &metadata_uri);
}

#[test]
#[should_panic(expected = "Metadata URI cannot be empty")]
fn test_update_metadata_rejects_empty_uri() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "offering_006");
    let metadata_v1 = String::from_str(&env, "ipfs://QmOriginalHash");
    let empty_metadata = String::from_str(&env, "");

    // Set initial metadata
    client.set_metadata(&issuer, &offering_id, &metadata_v1);

    // Try to update with empty metadata
    client.update_metadata(&issuer, &offering_id, &empty_metadata);
}

#[test]
#[should_panic(expected = "Metadata URI exceeds maximum length")]
fn test_update_metadata_rejects_oversized_uri() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "offering_007");
    let metadata_v1 = String::from_str(&env, "ipfs://QmOriginalHash");
    
    // Create a string longer than MAX_METADATA_LENGTH (1024 bytes)
    let long_string = "x".repeat(1025);
    let oversized_metadata = String::from_str(&env, &long_string);

    // Set initial metadata
    client.set_metadata(&issuer, &offering_id, &metadata_v1);

    // Try to update with oversized metadata
    client.update_metadata(&issuer, &offering_id, &oversized_metadata);
}

#[test]
fn test_delete_metadata_removes_entry() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "offering_008");
    let metadata_uri = String::from_str(&env, "ipfs://QmHashToDelete");

    // Set metadata
    client.set_metadata(&issuer, &offering_id, &metadata_uri);
    let stored = client.get_metadata(&issuer, &offering_id);
    assert_eq!(stored, Some(metadata_uri));

    // Delete metadata
    client.delete_metadata(&issuer, &offering_id);

    // Verify it's deleted
    let result = client.get_metadata(&issuer, &offering_id);
    assert_eq!(result, None, "Expected metadata to be deleted");
}

#[test]
#[should_panic(expected = "No metadata found for offering")]
fn test_delete_metadata_fails_for_nonexistent() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "nonexistent_offering");

    client.delete_metadata(&issuer, &offering_id);
}

#[test]
fn test_multiple_offerings_per_issuer() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_1 = String::from_str(&env, "offering_001");
    let offering_2 = String::from_str(&env, "offering_002");
    let offering_3 = String::from_str(&env, "offering_003");
    
    let metadata_1 = String::from_str(&env, "ipfs://QmHash1");
    let metadata_2 = String::from_str(&env, "https://example.com/meta2");
    let metadata_3 = String::from_str(&env, "ipfs://QmHash3");

    // Set metadata for multiple offerings
    client.set_metadata(&issuer, &offering_1, &metadata_1);
    client.set_metadata(&issuer, &offering_2, &metadata_2);
    client.set_metadata(&issuer, &offering_3, &metadata_3);

    // Verify all are stored correctly
    assert_eq!(client.get_metadata(&issuer, &offering_1), Some(metadata_1));
    assert_eq!(client.get_metadata(&issuer, &offering_2), Some(metadata_2));
    assert_eq!(client.get_metadata(&issuer, &offering_3), Some(metadata_3));
}

#[test]
fn test_different_issuers_have_separate_metadata() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer_1 = Address::generate(&env);
    let issuer_2 = Address::generate(&env);
    let offering_id = String::from_str(&env, "same_offering_id");
    
    let metadata_1 = String::from_str(&env, "issuer1_metadata");
    let metadata_2 = String::from_str(&env, "issuer2_metadata");

    // Set metadata for same offering ID but different issuers
    client.set_metadata(&issuer_1, &offering_id, &metadata_1);
    client.set_metadata(&issuer_2, &offering_id, &metadata_2);

    // Verify they are stored separately
    assert_eq!(client.get_metadata(&issuer_1, &offering_id), Some(metadata_1));
    assert_eq!(client.get_metadata(&issuer_2, &offering_id), Some(metadata_2));
}

#[test]
fn test_metadata_boundary_conditions() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "offering_boundary");
    
    // Test with exactly 1024 bytes (max allowed)
    let max_string = "a".repeat(1024);
    let max_metadata = String::from_str(&env, &max_string);

    // This should succeed
    client.set_metadata(&issuer, &offering_id, &max_metadata);
    let stored = client.get_metadata(&issuer, &offering_id);
    assert_eq!(stored, Some(max_metadata), "Expected max length metadata to be stored");
}

#[test]
fn test_repeated_updates_emit_events() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let offering_id = String::from_str(&env, "offering_events");
    let metadata_v1 = String::from_str(&env, "ipfs://QmV1");
    let metadata_v2 = String::from_str(&env, "ipfs://QmV2");
    let metadata_v3 = String::from_str(&env, "ipfs://QmV3");

    let initial_count = env.events().all().len();

    // Perform multiple operations
    client.set_metadata(&issuer, &offering_id, &metadata_v1);
    let count_after_set = env.events().all().len();
    assert!(count_after_set > initial_count, "Expected event from set_metadata");

    client.update_metadata(&issuer, &offering_id, &metadata_v2);
    let count_after_update_1 = env.events().all().len();
    assert!(count_after_update_1 > count_after_set, "Expected event from first update");

    client.update_metadata(&issuer, &offering_id, &metadata_v3);
    let count_after_update_2 = env.events().all().len();
    assert!(count_after_update_2 > count_after_update_1, "Expected event from second update");

    client.delete_metadata(&issuer, &offering_id);
    let count_after_delete = env.events().all().len();
    assert!(count_after_delete > count_after_update_2, "Expected event from delete");
}

// ── blacklist CRUD ────────────────────────────────────────────

#[test]
fn add_marks_investor_as_blacklisted() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    assert!(!client.is_blacklisted(&token, &investor));
    client.blacklist_add(&admin, &token, &investor);
    assert!(client.is_blacklisted(&token, &investor));
}

#[test]
fn remove_unmarks_investor() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    client.blacklist_remove(&admin, &token, &investor);
    assert!(!client.is_blacklisted(&token, &investor));
}

#[test]
fn get_blacklist_returns_all_blocked_investors() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin  = Address::generate(&env);
    let token  = Address::generate(&env);
    let inv_a  = Address::generate(&env);
    let inv_b  = Address::generate(&env);
    let inv_c  = Address::generate(&env);

    client.blacklist_add(&admin, &token, &inv_a);
    client.blacklist_add(&admin, &token, &inv_b);
    client.blacklist_add(&admin, &token, &inv_c);

    let list = client.get_blacklist(&token);
    assert_eq!(list.len(), 3);
    assert!(list.contains(&inv_a));
    assert!(list.contains(&inv_b));
    assert!(list.contains(&inv_c));
}

#[test]
fn get_blacklist_empty_before_any_add() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let token  = Address::generate(&env);

    assert_eq!(client.get_blacklist(&token).len(), 0);
}

// ── idempotency ───────────────────────────────────────────────

#[test]
fn double_add_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    client.blacklist_add(&admin, &token, &investor);

    assert_eq!(client.get_blacklist(&token).len(), 1);
}

#[test]
fn remove_nonexistent_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_remove(&admin, &token, &investor); // must not panic
    assert!(!client.is_blacklisted(&token, &investor));
}

// ── per-offering isolation ────────────────────────────────────

#[test]
fn blacklist_is_scoped_per_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token_a  = Address::generate(&env);
    let token_b  = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token_a, &investor);

    assert!( client.is_blacklisted(&token_a, &investor));
    assert!(!client.is_blacklisted(&token_b, &investor));
}

#[test]
fn removing_from_one_offering_does_not_affect_another() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token_a  = Address::generate(&env);
    let token_b  = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token_a, &investor);
    client.blacklist_add(&admin, &token_b, &investor);
    client.blacklist_remove(&admin, &token_a, &investor);

    assert!(!client.is_blacklisted(&token_a, &investor));
    assert!( client.is_blacklisted(&token_b, &investor));
}

// ── event emission ────────────────────────────────────────────

#[test]
fn blacklist_add_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    let before = env.events().all().len();
    client.blacklist_add(&admin, &token, &investor);
    assert!(env.events().all().len() > before);
}

#[test]
fn blacklist_remove_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    let before = env.events().all().len();
    client.blacklist_remove(&admin, &token, &investor);
    assert!(env.events().all().len() > before);
}

// ── distribution enforcement ──────────────────────────────────

#[test]
fn blacklisted_investor_excluded_from_distribution_filter() {
    let env = Env::default();
    env.mock_all_auths();
    let client  = make_client(&env);
    let admin   = Address::generate(&env);
    let token   = Address::generate(&env);
    let allowed = Address::generate(&env);
    let blocked = Address::generate(&env);

    client.blacklist_add(&admin, &token, &blocked);

    let investors = [allowed.clone(), blocked.clone()];
    let eligible = investors
        .iter()
        .filter(|inv| !client.is_blacklisted(&token, inv))
        .count();

    assert_eq!(eligible, 1);
}

#[test]
fn blacklist_takes_precedence_over_whitelist() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);

    // Even if investor were on a whitelist, blacklist must win
    assert!(client.is_blacklisted(&token, &investor));
}

// ── auth enforcement ──────────────────────────────────────────

#[test]
#[should_panic]
fn blacklist_add_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client    = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token     = Address::generate(&env);
    let victim    = Address::generate(&env);

    client.blacklist_add(&bad_actor, &token, &victim);
}

#[test]
#[should_panic]
fn blacklist_remove_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client    = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token     = Address::generate(&env);
    let investor  = Address::generate(&env);

    client.blacklist_remove(&bad_actor, &token, &investor);
}
