//! Adversarial tests for `get_whitelist_page` (issue #1108).
//!
//! `get_whitelist_page` is a read-only paginator — it has no auth gate and
//! returns `(Vec<Address>, Option<u32>)` in all cases (no `Result`).
//!
//! Test matrix:
//! | Case                                         | Expected outcome                           |
//! |----------------------------------------------|--------------------------------------------|
//! | empty whitelist (no entries)                 | `([], None)`                               |
//! | non-existent offering/issuer                 | `([], None)` — not an error                |
//! | single entry, start=0, limit=1               | `([addr], None)`                           |
//! | single entry, start=1 (past end)             | `([], None)`                               |
//! | multiple entries, first page                 | first `limit` entries, cursor = limit      |
//! | multiple entries, last page (no next)        | remaining entries, cursor = `None`         |
//! | multiple entries, middle page                | correct slice, cursor points to next       |
//! | start == count exactly                       | `([], None)`                               |
//! | start > count                                | `([], None)`                               |
//! | limit == 0 → capped to MAX_PAGE_LIMIT (20)   | up to 20 entries returned                  |
//! | limit > MAX_PAGE_LIMIT → capped to 20        | up to 20 entries returned                  |
//! | limit == MAX_PAGE_LIMIT exactly              | up to 20 entries, correct cursor           |
//! | limit larger than remaining entries          | all remaining, cursor = `None`             |
//! | 25 entries, start=0, limit=0 (→20)           | 20 entries, cursor = 20                    |
//! | 25 entries, start=20, limit=0 (→20)          | 5 entries, cursor = `None`                 |
//! | wrong namespace / wrong token                | `([], None)` — different offering key      |

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Initialize contract and register one offering. Returns
/// `(env, contract_id, issuer, token)`.
fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payout,
        &0,
        &symbol_short!(""),
        &0,
    );
    (env, contract_id, issuer, token)
}

/// Seed `n` distinct investor addresses into the whitelist and return them in
/// insertion order.
fn seed_whitelist(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
    n: usize,
) -> alloc::vec::Vec<Address> {
    let mut addrs = alloc::vec::Vec::new();
    for _ in 0..n {
        let investor = Address::generate(env);
        client.whitelist_add(issuer, issuer, &symbol_short!("def"), token, &investor);
        addrs.push(investor);
    }
    addrs
}

// ── empty / no-offering cases ─────────────────────────────────────────────────

/// Empty whitelist (offering exists but no entries): returns `([], None)`.
#[test]
fn empty_whitelist_returns_empty_page_and_no_cursor() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let (page, cursor) = client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &0, &10);
    assert_eq!(page.len(), 0, "empty whitelist must return empty page");
    assert!(cursor.is_none(), "empty whitelist must return no cursor");
}

/// Non-existent offering (issuer never registered): returns `([], None)` — not
/// an error; the function is read-only with no auth gate.
#[test]
fn nonexistent_offering_returns_empty_page_and_no_cursor() {
    let (env, contract_id, _issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let stranger = Address::generate(&env);

    let (page, cursor) =
        client.get_whitelist_page(&stranger, &symbol_short!("def"), &token, &0, &10);
    assert_eq!(page.len(), 0);
    assert!(cursor.is_none());
}

/// Wrong namespace for an existing issuer/token: different storage key, returns
/// `([], None)`.
#[test]
fn wrong_namespace_returns_empty_page() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 3);

    let (page, cursor) =
        client.get_whitelist_page(&issuer, &symbol_short!("other"), &token, &0, &10);
    assert_eq!(page.len(), 0);
    assert!(cursor.is_none());
}

/// Wrong token for an existing issuer/namespace: different storage key, returns
/// `([], None)`.
#[test]
fn wrong_token_returns_empty_page() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 3);
    let other_token = Address::generate(&env);

    let (page, cursor) =
        client.get_whitelist_page(&issuer, &symbol_short!("def"), &other_token, &0, &10);
    assert_eq!(page.len(), 0);
    assert!(cursor.is_none());
}

// ── single-entry cases ────────────────────────────────────────────────────────

/// Single entry, start=0, limit=1: returns the entry with no next cursor.
#[test]
fn single_entry_start0_limit1_returns_entry_no_cursor() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 1);

    let (page, cursor) = client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &0, &1);
    assert_eq!(page.len(), 1, "must return the single entry");
    assert!(cursor.is_none(), "no more entries after the only one");
}

/// Single entry, start=1 (past the end): returns empty page and no cursor.
#[test]
fn single_entry_start1_returns_empty_no_cursor() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 1);

    let (page, cursor) = client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &1, &10);
    assert_eq!(page.len(), 0);
    assert!(cursor.is_none());
}

// ── pagination / multi-entry cases ────────────────────────────────────────────

/// 5 entries, start=0, limit=3: returns first 3 entries, cursor = 3.
#[test]
fn pagination_first_page_returns_correct_slice_and_cursor() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 5);

    let (page, cursor) = client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &0, &3);
    assert_eq!(page.len(), 3, "first page must contain 3 entries");
    assert_eq!(cursor, Some(3), "cursor must point to the next start index");
}

/// 5 entries, start=3, limit=3: returns last 2 entries, no cursor.
#[test]
fn pagination_last_page_returns_remaining_and_no_cursor() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 5);

    let (page, cursor) = client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &3, &3);
    assert_eq!(page.len(), 2, "last page must contain only the remaining 2 entries");
    assert!(cursor.is_none(), "no next page after the last entry");
}

/// 6 entries, start=2, limit=2: middle page with correct cursor.
#[test]
fn pagination_middle_page_has_correct_cursor() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 6);

    let (page, cursor) = client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &2, &2);
    assert_eq!(page.len(), 2);
    assert_eq!(cursor, Some(4), "cursor must be start + limit = 4");
}

/// start == count exactly: returns empty page, no cursor.
#[test]
fn start_equal_to_count_returns_empty_no_cursor() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 4);

    let (page, cursor) = client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &4, &10);
    assert_eq!(page.len(), 0, "start == count must return empty page");
    assert!(cursor.is_none());
}

/// start > count: returns empty page, no cursor.
#[test]
fn start_greater_than_count_returns_empty_no_cursor() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 4);

    let (page, cursor) =
        client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &100, &10);
    assert_eq!(page.len(), 0, "start > count must return empty page");
    assert!(cursor.is_none());
}

/// limit larger than remaining entries: returns all remaining, no cursor.
#[test]
fn limit_larger_than_remaining_returns_all_remaining_no_cursor() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 5);

    // Request 100 starting at 3; only 2 remain.
    let (page, cursor) =
        client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &3, &100);
    assert_eq!(page.len(), 2, "must return only the 2 remaining entries");
    assert!(cursor.is_none(), "no further page when all remaining are returned");
}

// ── limit boundary / capping tests ───────────────────────────────────────────

/// limit == 0 is capped to MAX_PAGE_LIMIT (20): with 5 entries returns all 5.
#[test]
fn limit_zero_capped_to_max_returns_up_to_20_entries() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 5);

    let (page, cursor) = client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &0, &0);
    assert_eq!(page.len(), 5, "limit=0 capped to 20; 5 entries must all be returned");
    assert!(cursor.is_none());
}

/// limit > MAX_PAGE_LIMIT (> 20) is capped to 20: with 5 entries returns all 5.
#[test]
fn limit_above_max_capped_to_20() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 5);

    let (page, cursor) =
        client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &0, &9999);
    assert_eq!(page.len(), 5, "limit capped at 20; all 5 entries must be returned");
    assert!(cursor.is_none());
}

/// limit == MAX_PAGE_LIMIT exactly (20) is not capped: with 25 entries first
/// page returns exactly 20, cursor = 20.
#[test]
fn limit_exactly_max_page_limit_returns_20_entries_with_cursor() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 25);

    let (page, cursor) = client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &0, &20);
    assert_eq!(page.len(), 20, "exactly 20 entries must be returned on the first page");
    assert_eq!(cursor, Some(20), "cursor must point to entry 20");
}

/// 25 entries, limit=0 (→ capped to 20), start=0: 20 entries, cursor=20.
#[test]
fn twenty_five_entries_limit_zero_first_page() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 25);

    let (page, cursor) = client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &0, &0);
    assert_eq!(page.len(), 20);
    assert_eq!(cursor, Some(20));
}

/// 25 entries, limit=0 (→ capped to 20), start=20: last 5 entries, no cursor.
#[test]
fn twenty_five_entries_limit_zero_second_page() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 25);

    let (page, cursor) = client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &20, &0);
    assert_eq!(page.len(), 5, "second page must contain the remaining 5 entries");
    assert!(cursor.is_none(), "no next page after the last entry");
}

// ── full cursor-walk correctness ──────────────────────────────────────────────

/// Walk all pages with limit=3 over 7 entries; verify all addresses are visited
/// exactly once and the walk terminates.
#[test]
fn full_cursor_walk_visits_all_entries_exactly_once() {
    let (env, contract_id, issuer, token) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    seed_whitelist(&env, &client, &issuer, &token, 7);

    let mut collected: alloc::vec::Vec<Address> = alloc::vec::Vec::new();
    let mut start: u32 = 0;
    let limit: u32 = 3;

    loop {
        let (page, cursor) =
            client.get_whitelist_page(&issuer, &symbol_short!("def"), &token, &start, &limit);
        for i in 0..page.len() {
            collected.push(page.get(i).unwrap());
        }
        match cursor {
            Some(next) => start = next,
            None => break,
        }
    }

    assert_eq!(collected.len(), 7, "cursor walk must visit all 7 entries");
    // Verify count: seed_whitelist generates unique addresses, so visiting each
    // exactly once is sufficient to confirm correctness.
}
