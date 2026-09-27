//! # Adversarial coverage for `get_whitelist` — Issue #1107
//!
//! ## What is under test
//!
//! `get_whitelist(env, issuer, namespace, token) -> Vec<Address>` is a
//! public, read-only query that returns the full set of whitelisted
//! addresses for an offering, identified by the triple
//! `(issuer, namespace, token)`.  It carries **no `require_auth`** call
//! of its own, so every test below satisfies host auth via
//! `env.mock_all_auths()` to keep focus on the query semantics.
//!
//! ## Coverage map
//!
//! | Category                             | Test name                                                          |
//! |--------------------------------------|--------------------------------------------------------------------|
//! | Empty / uninitialized state          | `get_whitelist_returns_empty_vec_for_unregistered_offering`        |
//! | Empty after explicit register        | `get_whitelist_empty_after_register_no_adds`                       |
//! | Single entry                         | `get_whitelist_single_entry`                                       |
//! | Multiple entries membership          | `get_whitelist_returns_all_members`                                |
//! | Remove reduces list                  | `get_whitelist_reflects_removal`                                   |
//! | Full add/remove round-trip           | `get_whitelist_empty_after_all_removed`                            |
//! | Idempotent add (no duplicates)       | `get_whitelist_no_duplicate_after_double_add`                      |
//! | Namespace isolation                  | `get_whitelist_is_isolated_by_namespace`                           |
//! | Token isolation                      | `get_whitelist_is_isolated_by_token`                               |
//! | Issuer isolation                     | `get_whitelist_is_isolated_by_issuer`                              |
//! | Wrong-issuer query (ghost offering)  | `get_whitelist_wrong_issuer_returns_empty`                         |
//! | Wrong-namespace query                | `get_whitelist_wrong_namespace_returns_empty`                      |
//! | Wrong-token query                    | `get_whitelist_wrong_token_returns_empty`                          |
//! | State unchanged by read              | `get_whitelist_read_does_not_mutate_storage`                       |
//! | Cross-offering non-interference      | `get_whitelist_cross_offering_no_interference`                     |
//! | Blacklist has no effect on list      | `get_whitelist_unaffected_by_blacklist`                            |
//! | Frozen contract still readable       | `get_whitelist_readable_when_contract_frozen`                      |
//! | Paused contract still readable       | `get_whitelist_readable_when_contract_paused`                      |
//! | Frozen offering still readable       | `get_whitelist_readable_when_offering_frozen`                      |
//! | Admin-added entries visible          | `get_whitelist_entries_added_by_admin_are_visible`                 |
//! | Count consistency with is_whitelisted| `get_whitelist_count_consistent_with_is_whitelisted`               |

#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Instantiate a fresh contract and return its client.
fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Register a single offering under `symbol_short!("ns")` and return
/// `(issuer, token)`.  Requires `env.mock_all_auths()` to already be set.
fn register_offering(
    client: &RevoraRevenueShareClient<'_>,
    env: &Env,
) -> (Address, Address) {
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let payout = Address::generate(env);
    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000u32,
        &payout,
        &0u32,
        &symbol_short!(""),
        &0u32,
    );
    (issuer, token)
}

// ── 1. Empty / uninitialized state ───────────────────────────────────────────

/// Querying an offering that was never registered returns an empty Vec.
/// No panic, no error — the function degrades gracefully to an empty result.
#[test]
fn get_whitelist_returns_empty_vec_for_unregistered_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // Nothing registered; result must be an empty Vec, not a panic or error.
    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 0);
}

/// After registering an offering but before adding any whitelist entries,
/// `get_whitelist` must return an empty Vec.
#[test]
fn get_whitelist_empty_after_register_no_adds() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);

    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 0);
}

// ── 2. Happy-path population ──────────────────────────────────────────────────

/// Adding a single investor and then querying returns a one-element Vec.
#[test]
fn get_whitelist_single_entry() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);
    let investor = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);

    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 1);
    assert!(list.contains(&investor));
}

/// Adding multiple distinct investors — all must appear in the result.
#[test]
fn get_whitelist_returns_all_members() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);

    let inv_a = Address::generate(&env);
    let inv_b = Address::generate(&env);
    let inv_c = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_a);
    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_b);
    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_c);

    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 3);
    assert!(list.contains(&inv_a));
    assert!(list.contains(&inv_b));
    assert!(list.contains(&inv_c));
}

// ── 3. Removal reflected in query ────────────────────────────────────────────

/// Removing one of several investors must remove exactly that address.
#[test]
fn get_whitelist_reflects_removal() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);

    let inv_a = Address::generate(&env);
    let inv_b = Address::generate(&env);
    let inv_c = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_a);
    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_b);
    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_c);

    // Remove inv_b.
    client.whitelist_remove(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_b);

    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 2);
    assert!(list.contains(&inv_a));
    assert!(!list.contains(&inv_b), "removed investor must not appear in whitelist");
    assert!(list.contains(&inv_c));
}

/// Removing all investors one by one must leave an empty Vec.
#[test]
fn get_whitelist_empty_after_all_removed() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);

    let inv_a = Address::generate(&env);
    let inv_b = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_a);
    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_b);

    client.whitelist_remove(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_a);
    client.whitelist_remove(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_b);

    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 0, "whitelist must be empty after all entries removed");
}

// ── 4. Idempotency / de-duplication ──────────────────────────────────────────

/// Adding the same investor twice must not produce a duplicate in the result.
#[test]
fn get_whitelist_no_duplicate_after_double_add() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);
    let investor = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);
    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor); // idempotent

    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 1, "double add must not produce a duplicate entry");
    assert!(list.contains(&investor));
}

// ── 5. Namespace isolation ────────────────────────────────────────────────────

/// A whitelist entry added to `ns1` must NOT appear when querying `ns2`.
#[test]
fn get_whitelist_is_isolated_by_namespace() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    let investor = Address::generate(&env);

    // Register offering in namespace "ns1" only.
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns1"),
        &token,
        &1_000u32,
        &payout,
        &0u32,
        &symbol_short!(""),
        &0u32,
    );

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns1"), &token, &investor);

    // Query under a different namespace — must return empty.
    let list_ns2 = client.get_whitelist(&issuer, &symbol_short!("ns2"), &token);
    assert_eq!(list_ns2.len(), 0, "whitelist must be isolated per namespace");

    // Sanity: ns1 still has the entry.
    let list_ns1 = client.get_whitelist(&issuer, &symbol_short!("ns1"), &token);
    assert_eq!(list_ns1.len(), 1);
    assert!(list_ns1.contains(&investor));
}

// ── 6. Token isolation ────────────────────────────────────────────────────────

/// Whitelist entries for token_a must not bleed into token_b's query.
#[test]
fn get_whitelist_is_isolated_by_token() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout = Address::generate(&env);
    let investor = Address::generate(&env);

    // Register two distinct offerings under the same namespace.
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token_a,
        &1_000u32,
        &payout,
        &0u32,
        &symbol_short!(""),
        &0u32,
    );
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token_b,
        &1_000u32,
        &payout,
        &0u32,
        &symbol_short!(""),
        &0u32,
    );

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token_a, &investor);

    // token_b's whitelist must be untouched.
    let list_b = client.get_whitelist(&issuer, &symbol_short!("ns"), &token_b);
    assert_eq!(list_b.len(), 0, "whitelist must be isolated per token");

    // token_a's whitelist still has the entry.
    let list_a = client.get_whitelist(&issuer, &symbol_short!("ns"), &token_a);
    assert_eq!(list_a.len(), 1);
    assert!(list_a.contains(&investor));
}

// ── 7. Issuer isolation ───────────────────────────────────────────────────────

/// Whitelist entries for issuer_a must not appear under issuer_b's query,
/// even when namespace and token address are identical.
#[test]
fn get_whitelist_is_isolated_by_issuer() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    // Same token and namespace; only the issuer differs.
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    let investor = Address::generate(&env);

    client.register_offering(
        &issuer_a,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000u32,
        &payout,
        &0u32,
        &symbol_short!(""),
        &0u32,
    );

    client.whitelist_add(&issuer_a, &issuer_a, &symbol_short!("ns"), &token, &investor);

    // Querying under issuer_b must not see issuer_a's entries.
    let list_b = client.get_whitelist(&issuer_b, &symbol_short!("ns"), &token);
    assert_eq!(list_b.len(), 0, "whitelist must be isolated per issuer");

    // issuer_a's data is intact.
    let list_a = client.get_whitelist(&issuer_a, &symbol_short!("ns"), &token);
    assert_eq!(list_a.len(), 1);
    assert!(list_a.contains(&investor));
}

// ── 8. Boundary / wrong-key queries ──────────────────────────────────────────

/// Querying with the correct token and namespace but a non-issuer address
/// returns an empty Vec (the storage key simply does not exist under that
/// composite key — no error, no panic).
#[test]
fn get_whitelist_wrong_issuer_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);
    let investor = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);

    let attacker = Address::generate(&env);
    let list = client.get_whitelist(&attacker, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 0, "wrong issuer must return empty whitelist");
}

/// Querying with the wrong namespace returns empty even when the issuer and
/// token match an existing offering that has whitelist entries.
#[test]
fn get_whitelist_wrong_namespace_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);
    let investor = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);

    let list = client.get_whitelist(&issuer, &symbol_short!("other"), &token);
    assert_eq!(list.len(), 0, "wrong namespace must return empty whitelist");
}

/// Querying with the wrong token address returns empty.
#[test]
fn get_whitelist_wrong_token_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);
    let investor = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);

    let other_token = Address::generate(&env);
    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &other_token);
    assert_eq!(list.len(), 0, "wrong token must return empty whitelist");
}

// ── 9. Read-only — storage immutability ──────────────────────────────────────

/// Calling `get_whitelist` multiple times must not alter the stored set.
/// Verified by comparing results of two sequential reads.
#[test]
fn get_whitelist_read_does_not_mutate_storage() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);

    let inv_a = Address::generate(&env);
    let inv_b = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_a);
    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_b);

    // First read.
    let list1 = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    // Second read — must be identical.
    let list2 = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);

    assert_eq!(list1.len(), list2.len(), "consecutive reads must return equal lengths");
    assert!(list2.contains(&inv_a));
    assert!(list2.contains(&inv_b));
}

// ── 10. Cross-offering non-interference ──────────────────────────────────────

/// Operations on offering B must never affect offering A's whitelist.
#[test]
fn get_whitelist_cross_offering_no_interference() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout = Address::generate(&env);
    let investor_a = Address::generate(&env);
    let investor_b = Address::generate(&env);

    client.register_offering(
        &issuer_a,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token_a,
        &1_000u32,
        &payout,
        &0u32,
        &symbol_short!(""),
        &0u32,
    );
    client.register_offering(
        &issuer_b,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token_b,
        &1_000u32,
        &payout,
        &0u32,
        &symbol_short!(""),
        &0u32,
    );

    client.whitelist_add(&issuer_a, &issuer_a, &symbol_short!("ns"), &token_a, &investor_a);
    client.whitelist_add(&issuer_b, &issuer_b, &symbol_short!("ns"), &token_b, &investor_b);

    // Mutate offering B — remove its investor.
    client.whitelist_remove(&issuer_b, &issuer_b, &symbol_short!("ns"), &token_b, &investor_b);

    // Offering A must be untouched.
    let list_a = client.get_whitelist(&issuer_a, &symbol_short!("ns"), &token_a);
    assert_eq!(list_a.len(), 1);
    assert!(list_a.contains(&investor_a));

    // Offering B is empty.
    let list_b = client.get_whitelist(&issuer_b, &symbol_short!("ns"), &token_b);
    assert_eq!(list_b.len(), 0);
}

// ── 11. Blacklist has no effect on whitelist contents ─────────────────────────

/// Blacklisting an investor must not remove them from the whitelist.
/// `get_whitelist` reflects whitelist storage, not eligibility logic.
#[test]
fn get_whitelist_unaffected_by_blacklist() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);
    let investor = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);
    // Also blacklist the same address.
    client.blacklist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);

    // Whitelist storage must still contain the investor — blacklist is a
    // separate map; get_whitelist only reads the whitelist map.
    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert!(
        list.contains(&investor),
        "blacklisting must not remove an address from the whitelist storage"
    );
    assert_eq!(list.len(), 1);
}

// ── 12. Frozen / paused contract — reads remain unblocked ────────────────────

/// A frozen contract still allows read-only queries; `get_whitelist` must
/// succeed and return the correct entries even when state-mutation is blocked.
#[test]
fn get_whitelist_readable_when_contract_frozen() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    let issuer = admin.clone();
    let investor = Address::generate(&env);

    client.set_admin(&admin);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000u32,
        &payout,
        &0u32,
        &symbol_short!(""),
        &0u32,
    );
    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);

    // Freeze the contract.
    client.freeze();

    // Read must still succeed and return the pre-freeze entries.
    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 1);
    assert!(list.contains(&investor));
}

/// A paused contract still allows read-only queries.
#[test]
fn get_whitelist_readable_when_contract_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    let issuer = admin.clone();
    let investor = Address::generate(&env);

    client.set_admin(&admin);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000u32,
        &payout,
        &0u32,
        &symbol_short!(""),
        &0u32,
    );
    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);

    // Pause via admin tier.
    client.pause_admin(&admin);

    // Read must succeed regardless.
    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 1);
    assert!(list.contains(&investor));
}

/// A per-offering freeze must not block `get_whitelist` (read-only path).
#[test]
fn get_whitelist_readable_when_offering_frozen() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);
    let investor = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &investor);

    // Freeze the offering.
    client.freeze_offering(&issuer, &issuer, &symbol_short!("ns"), &token);

    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 1);
    assert!(list.contains(&investor));
}

// ── 13. Admin-initiated whitelist entries are visible ─────────────────────────

/// When the admin (not the issuer) adds addresses to the whitelist, those
/// entries must be returned by `get_whitelist` to confirm admin writes are
/// committed to the same storage map.
#[test]
fn get_whitelist_entries_added_by_admin_are_visible() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    let investor = Address::generate(&env);

    // Admin sets itself up and registers an offering for a separate issuer.
    client.set_admin(&admin);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000u32,
        &payout,
        &0u32,
        &symbol_short!(""),
        &0u32,
    );

    // Admin (not issuer) adds the investor.
    client.whitelist_add(&admin, &issuer, &symbol_short!("ns"), &token, &investor);

    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 1);
    assert!(
        list.contains(&investor),
        "admin-added whitelist entry must be visible via get_whitelist"
    );
}

// ── 14. Count consistency with is_whitelisted ────────────────────────────────

/// Every address returned by `get_whitelist` must also return `true` via
/// `is_whitelisted`, and addresses *not* in the list must return `false`.
#[test]
fn get_whitelist_count_consistent_with_is_whitelisted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, token) = register_offering(&client, &env);

    let inv_a = Address::generate(&env);
    let inv_b = Address::generate(&env);
    let not_listed = Address::generate(&env);

    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_a);
    client.whitelist_add(&issuer, &issuer, &symbol_short!("ns"), &token, &inv_b);

    let list = client.get_whitelist(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(list.len(), 2);

    // Every member returned by get_whitelist must be confirmed by is_whitelisted.
    for i in 0..list.len() {
        let addr = list.get(i).unwrap();
        assert!(
            client.is_whitelisted(&issuer, &symbol_short!("ns"), &token, &addr),
            "get_whitelist member must be confirmed by is_whitelisted"
        );
    }

    // An address absent from the list must not be confirmed.
    assert!(
        !client.is_whitelisted(&issuer, &symbol_short!("ns"), &token, &not_listed),
        "address not in whitelist must return false from is_whitelisted"
    );
}
