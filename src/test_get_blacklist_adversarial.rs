//! Adversarial test coverage for `get_blacklist`.
//!
//! # Related Issue
//! #1099 — Add adversarial coverage for `get_blacklist` in lib.
//!
//! # What is tested
//!
//! `get_blacklist(env, issuer, namespace, token) -> Vec<Address>` is a read-only
//! method that returns the insertion-ordered blacklist for an offering. It has no
//! auth guard and returns an empty `Vec` for any unknown key.  The source of truth
//! is `DataKey::BlacklistOrder(offering_id)` — a `Vec<Address>` maintained by both
//! `blacklist_add` (append) and `blacklist_remove` (filter-rebuild).
//!
//! ## Coverage matrix
//!
//! | # | Scenario | Invariant verified |
//! |---|---|---|
//! | 1 | Unknown (issuer, namespace, token) | Returns empty Vec, no panic |
//! | 2 | After registering offering but before any add | Returns empty Vec |
//! | 3 | Single entry | Returns exactly that one address |
//! | 4 | Multiple entries | Insertion order preserved (FIFO) |
//! | 5 | Idempotent add (same address twice) | No duplicate appears in list |
//! | 6 | Remove from head of list | Tail preserved in original relative order |
//! | 7 | Remove from middle of list | Head and tail preserved in order |
//! | 8 | Remove from tail of list | Head preserved in original relative order |
//! | 9 | Remove non-existent address | No panic; list unchanged |
//! | 10 | Add → remove → re-add cycle | Re-added address appears at tail |
//! | 11 | Cross-token isolation | Different token key → independent list |
//! | 12 | Cross-namespace isolation | Different namespace → independent list |
//! | 13 | Cross-issuer isolation | Different issuer → independent list |
//! | 14 | Consistency with `is_blacklisted` | Every address in list returns true |
//! | 15 | Consistency with `get_blacklist_size` | len matches size counter |
//! | 16 | Empty list after removing all entries | Returns empty Vec, not an error |
//! | 17 | Large list near MAX_BLACKLIST_SIZE | All entries present, order correct |

#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

// ─── helpers ────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Register a minimal offering so `blacklist_add` can resolve the current issuer.
fn register_offering(
    client: &RevoraRevenueShareClient<'_>,
    env: &Env,
    issuer: &Address,
    ns: &soroban_sdk::Symbol,
    token: &Address,
) {
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        ns,
        token,
        &1_000u32,
        token, // payout asset = token for simplicity
        &0_i128,
        &symbol_short!(""),
        &0,
    );
}

// ─── 1. Unknown (issuer, namespace, token) returns empty Vec ────────────────

/// # Test: get_blacklist on unknown offering returns empty Vec
///
/// `get_blacklist` reads `DataKey::BlacklistOrder(offering_id)`. When no entry
/// has ever been written under that key (no offering registered, no add ever
/// called), `persistent().get()` returns `None` and the method falls back to
/// `Vec::new(&env)`. This must not panic.
#[test]
fn get_blacklist_unknown_offering_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 0, "Expected empty vec for unknown offering");
}

// ─── 2. Registered offering with no entries returns empty Vec ───────────────

/// # Test: get_blacklist is empty right after offering registration
///
/// Registering an offering writes `OfferItem` / `OfferCount` but does NOT
/// touch `BlacklistOrder`. The list must be empty until the first `blacklist_add`.
#[test]
fn get_blacklist_registered_offering_no_entries_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer, &ns, &token);

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 0, "Offering has no blacklist entries yet");
}

// ─── 3. Single entry ────────────────────────────────────────────────────────

/// # Test: get_blacklist returns the single added address
///
/// After a single `blacklist_add`, the list contains exactly that address.
#[test]
fn get_blacklist_single_entry() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");
    let investor = Address::generate(&env);

    register_offering(&client, &env, &issuer, &ns, &token);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 1);
    assert_eq!(list.get(0).unwrap(), investor);
}

// ─── 4. Multiple entries — insertion order preserved ────────────────────────

/// # Test: get_blacklist preserves FIFO insertion order
///
/// `blacklist_add` appends each new address to `BlacklistOrder`.  The returned
/// vec must appear in the exact order the addresses were added, regardless of
/// any internal Map ordering that `DataKey::Blacklist` may use.
#[test]
fn get_blacklist_multiple_entries_insertion_order_preserved() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer, &ns, &token);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    let d = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &a);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &b);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &c);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &d);

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 4);
    assert_eq!(list.get(0).unwrap(), a, "index 0 should be first added");
    assert_eq!(list.get(1).unwrap(), b, "index 1 should be second added");
    assert_eq!(list.get(2).unwrap(), c, "index 2 should be third added");
    assert_eq!(list.get(3).unwrap(), d, "index 3 should be fourth added");
}

// ─── 5. Idempotent add — no duplicate in order list ─────────────────────────

/// # Test: Adding the same address twice does not create a duplicate in the list
///
/// `do_blacklist_add` checks `map.contains_key(investor)` before touching the
/// order vec.  If the address is already present it skips the push.  The result
/// must be a list of length 1, not 2.
#[test]
fn get_blacklist_idempotent_add_no_duplicate() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");
    let investor = Address::generate(&env);

    register_offering(&client, &env, &issuer, &ns, &token);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);
    // Second add of the same address — must be a no-op for the order vec.
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 1, "Duplicate add must not create a second entry");
    assert_eq!(list.get(0).unwrap(), investor);
}

// ─── 6. Remove from head ────────────────────────────────────────────────────

/// # Test: Removing the first-added address leaves the tail in original order
///
/// `blacklist_remove` rebuilds `BlacklistOrder` by iterating the old vec and
/// keeping only addresses still present in the map. Removing the head element
/// must shift all remaining entries down with their relative order preserved.
#[test]
fn get_blacklist_remove_from_head_preserves_tail_order() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer, &ns, &token);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &a);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &b);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &c);

    client.blacklist_remove(&issuer, &issuer, &ns, &token, &a);

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 2);
    assert_eq!(list.get(0).unwrap(), b, "b should shift to index 0 after head removal");
    assert_eq!(list.get(1).unwrap(), c, "c should shift to index 1 after head removal");
}

// ─── 7. Remove from middle ──────────────────────────────────────────────────

/// # Test: Removing a middle element leaves head and tail in original order
///
/// The rebuild loop must skip only the removed address. Head and tail survive
/// at indices 0 and 1 respectively.
#[test]
fn get_blacklist_remove_from_middle_preserves_head_and_tail() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer, &ns, &token);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &a);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &b);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &c);

    client.blacklist_remove(&issuer, &issuer, &ns, &token, &b);

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 2);
    assert_eq!(list.get(0).unwrap(), a, "a should remain at index 0");
    assert_eq!(list.get(1).unwrap(), c, "c should be at index 1 after middle removal");
}

// ─── 8. Remove from tail ────────────────────────────────────────────────────

/// # Test: Removing the last-added address leaves the head in original order
///
/// Tail removal is the simplest rebuild case: the loop copies the first N-1
/// entries unchanged and the removed address is simply not re-appended.
#[test]
fn get_blacklist_remove_from_tail_preserves_head() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer, &ns, &token);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &a);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &b);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &c);

    client.blacklist_remove(&issuer, &issuer, &ns, &token, &c);

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 2);
    assert_eq!(list.get(0).unwrap(), a);
    assert_eq!(list.get(1).unwrap(), b);
}

// ─── 9. Remove non-existent address — list unchanged, no panic ──────────────

/// # Test: Removing an address that was never added is a safe no-op
///
/// `blacklist_remove` calls `map.remove(investor)` which is idempotent on a
/// `Map`, and then rebuilds the order vec. Neither the map nor the order vec
/// should be corrupted by removing a key that was never present.
#[test]
fn get_blacklist_remove_nonexistent_is_noop() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");
    let investor = Address::generate(&env);
    let stranger = Address::generate(&env); // never added

    register_offering(&client, &env, &issuer, &ns, &token);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    // Remove an address that is not in the blacklist — must not panic.
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &stranger);

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 1, "List should be unchanged after removing a non-existent address");
    assert_eq!(list.get(0).unwrap(), investor);
}

// ─── 10. Add → remove → re-add cycle ────────────────────────────────────────

/// # Test: Re-adding a previously removed address appends it at the tail
///
/// After an address is removed and then re-added, `blacklist_add` treats it as
/// a fresh insertion (it is no longer in the map) and appends it to the end of
/// `BlacklistOrder`. The re-added address must appear at the last position.
#[test]
fn get_blacklist_add_remove_readd_appears_at_tail() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer, &ns, &token);

    let a = Address::generate(&env);
    let b = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &a);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &b);

    // Remove a, then re-add — should end up at index 1 (after b).
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &a);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &a);

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 2);
    assert_eq!(list.get(0).unwrap(), b, "b should stay at index 0");
    assert_eq!(list.get(1).unwrap(), a, "re-added a should appear at tail");
}

// ─── 11. Cross-token isolation ───────────────────────────────────────────────

/// # Test: Blacklist for one token is independent of another token's blacklist
///
/// `OfferingId` includes the token address, so two offerings with the same
/// issuer and namespace but different tokens must have separate
/// `BlacklistOrder` storage slots.
#[test]
fn get_blacklist_cross_token_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer, &ns, &token_a);
    register_offering(&client, &env, &issuer, &ns, &token_b);

    let investor_a = Address::generate(&env);
    let investor_b = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token_a, &investor_a);
    client.blacklist_add(&issuer, &issuer, &ns, &token_b, &investor_b);

    let list_a = client.get_blacklist(&issuer, &ns, &token_a);
    let list_b = client.get_blacklist(&issuer, &ns, &token_b);

    // token_a's list must only contain investor_a.
    assert_eq!(list_a.len(), 1);
    assert_eq!(list_a.get(0).unwrap(), investor_a);

    // token_b's list must only contain investor_b.
    assert_eq!(list_b.len(), 1);
    assert_eq!(list_b.get(0).unwrap(), investor_b);
}

// ─── 12. Cross-namespace isolation ───────────────────────────────────────────

/// # Test: Same issuer and token with different namespaces have independent blacklists
///
/// Namespace is part of `OfferingId`, so `DataKey::BlacklistOrder` keys are
/// distinct for different namespaces even when issuer and token match.
#[test]
fn get_blacklist_cross_namespace_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns1 = symbol_short!("ns1");
    let ns2 = symbol_short!("ns2");

    register_offering(&client, &env, &issuer, &ns1, &token);
    register_offering(&client, &env, &issuer, &ns2, &token);

    let investor1 = Address::generate(&env);
    let investor2 = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns1, &token, &investor1);
    client.blacklist_add(&issuer, &issuer, &ns2, &token, &investor2);

    let list1 = client.get_blacklist(&issuer, &ns1, &token);
    let list2 = client.get_blacklist(&issuer, &ns2, &token);

    assert_eq!(list1.len(), 1, "ns1 list must be independent");
    assert_eq!(list1.get(0).unwrap(), investor1);

    assert_eq!(list2.len(), 1, "ns2 list must be independent");
    assert_eq!(list2.get(0).unwrap(), investor2);
}

// ─── 13. Cross-issuer isolation ──────────────────────────────────────────────

/// # Test: Two issuers using the same token have independent blacklists
///
/// Issuer address is part of `OfferingId`, so two offerings from different
/// issuers must never share their `BlacklistOrder` storage.
#[test]
fn get_blacklist_cross_issuer_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer_x = Address::generate(&env);
    let issuer_y = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer_x, &ns, &token);
    register_offering(&client, &env, &issuer_y, &ns, &token);

    let investor_x = Address::generate(&env);
    let investor_y = Address::generate(&env);

    client.blacklist_add(&issuer_x, &issuer_x, &ns, &token, &investor_x);
    client.blacklist_add(&issuer_y, &issuer_y, &ns, &token, &investor_y);

    let list_x = client.get_blacklist(&issuer_x, &ns, &token);
    let list_y = client.get_blacklist(&issuer_y, &ns, &token);

    assert_eq!(list_x.len(), 1);
    assert_eq!(list_x.get(0).unwrap(), investor_x);

    assert_eq!(list_y.len(), 1);
    assert_eq!(list_y.get(0).unwrap(), investor_y);
}

// ─── 14. Consistency with is_blacklisted ─────────────────────────────────────

/// # Test: Every address returned by get_blacklist is_blacklisted == true
///
/// `is_blacklisted` reads `DataKey::Blacklist` (the `Map`), while `get_blacklist`
/// reads `DataKey::BlacklistOrder` (the `Vec`). Both are written atomically by
/// `blacklist_add` and both are cleaned by `blacklist_remove`. They must agree.
#[test]
fn get_blacklist_every_entry_is_blacklisted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer, &ns, &token);

    let investors: std::vec::Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();

    for inv in &investors {
        client.blacklist_add(&issuer, &issuer, &ns, &token, inv);
    }

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 5);
    for i in 0..list.len() {
        let addr = list.get(i).unwrap();
        assert!(
            client.is_blacklisted(&issuer, &ns, &token, &addr),
            "Address at index {i} from get_blacklist must be reported as blacklisted by is_blacklisted",
        );
    }
}

// ─── 15. Consistency with get_blacklist_size ──────────────────────────────────

/// # Test: get_blacklist().len() == get_blacklist_size() at every mutation step
///
/// `get_blacklist_size` reads the `Map` length from `DataKey::Blacklist`.
/// `get_blacklist` reads the `Vec` length from `DataKey::BlacklistOrder`.
/// Both counts must agree after every add and remove.
#[test]
fn get_blacklist_len_matches_get_blacklist_size() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer, &ns, &token);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    // Step 0: empty.
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 0);
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 0);

    // Step 1: add a.
    client.blacklist_add(&issuer, &issuer, &ns, &token, &a);
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 1);
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 1);

    // Step 2: add b.
    client.blacklist_add(&issuer, &issuer, &ns, &token, &b);
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 2);
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 2);

    // Step 3: add c.
    client.blacklist_add(&issuer, &issuer, &ns, &token, &c);
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 3);
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 3);

    // Step 4: remove b.
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &b);
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 2);
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 2);

    // Step 5: remove a.
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &a);
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 1);
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 1);

    // Step 6: remove c.
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &c);
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 0);
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 0);
}

// ─── 16. Empty list after removing all entries ───────────────────────────────

/// # Test: get_blacklist returns empty Vec (not an error) after all entries removed
///
/// Removing all entries reconstructs `BlacklistOrder` as an empty `Vec` and
/// writes it back. Subsequent `get_blacklist` must return an empty vec, not
/// `None` / panic.
#[test]
fn get_blacklist_empty_after_all_entries_removed() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer, &ns, &token);

    let a = Address::generate(&env);
    let b = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &a);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &b);

    client.blacklist_remove(&issuer, &issuer, &ns, &token, &a);
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &b);

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), 0, "Expected empty vec after all entries removed");
}

// ─── 17. Large list — order integrity near MAX_BLACKLIST_SIZE ─────────────────

/// # Test: get_blacklist returns all entries with correct order for a large list
///
/// `MAX_BLACKLIST_SIZE` is 200. We add a batch of 30 entries and verify that
/// `get_blacklist` returns all of them in insertion order. This exercises the
/// code path where `BlacklistOrder` is a larger vec and validates that no
/// entries are lost or reordered due to storage serialization.
///
/// We keep the batch well below 200 to avoid hitting the cap while still
/// meaningfully exercising the order-vec with more entries than the typical
/// test.
#[test]
fn get_blacklist_large_list_insertion_order_preserved() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    register_offering(&client, &env, &issuer, &ns, &token);

    const N: u32 = 30;
    let mut added: std::vec::Vec<Address> = std::vec::Vec::new();
    for _ in 0..N {
        let addr = Address::generate(&env);
        client.blacklist_add(&issuer, &issuer, &ns, &token, &addr);
        added.push(addr);
    }

    let list = client.get_blacklist(&issuer, &ns, &token);

    assert_eq!(list.len(), N, "All {N} entries must be present");
    for i in 0..N {
        assert_eq!(
            list.get(i).unwrap(),
            added[i as usize],
            "Entry at index {i} must match insertion order",
        );
    }
}
