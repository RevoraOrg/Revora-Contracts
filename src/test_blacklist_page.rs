//! # `get_blacklist_page` — Adversarial Coverage (Issue #1100)
//!
//! `get_blacklist_page` is a public read entrypoint that pages through the
//! per-offering blacklist in insertion order and returns a `(page, next_cursor)`
//! pair. It backs `get_blacklist` (legacy full read) and feeds the bounded
//! blacklist snapshot embedded in distribution events.
//!
//! Adversarial surface exercised here:
//!
//! - **Pagination bounds** — `start == len` and `start > len` (empty page, `None`
//!   cursor), `limit == 0` (falls back to `MAX_PAGE_LIMIT = 20`), `limit > 20`
//!   (clamped to 20), pages smaller than the limit, exact-multiple pagination
//!   where the final page must report `None` rather than a cursor pointing at an
//!   empty follow-up page.
//! - **Determinism** — two reads of the same `(start, limit)` window return
//!   identical pages; insertion order is preserved across pages.
//! - **Keying / isolation** — results are keyed by the full
//!   `(issuer, namespace, token)` triple: an unknown issuer, unknown namespace,
//!   or unknown token must all yield an empty page.
//! - **Mutation-driven order** — `blacklist_remove` rebuilds the insertion-order
//!   vector; removing a middle entry must shift the remaining entries forward so
//!   pages stay contiguous and cursor arithmetic stays correct. Re-adding a
//!   removed address must append it at the end of the order.
//! - **Gating** — while the contract is soft-paused or globally frozen,
//!   `blacklist_add` is rejected so the paginated view cannot silently change,
//!   while the read itself stays observable for off-chain tooling.
//! - **State invariants** — rejected operations (unauthorized caller, paused,
//!   frozen) leave the paginated view unchanged (entry count and membership
//!   stable), and pagination stays consistent with the O(1)
//!   `get_blacklist_size` after every mutation.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

/// `MAX_PAGE_LIMIT` is defined in `lib.rs` (20) but not exported; pin the value
/// here so these tests fail loudly if the clamping behavior ever changes.
const MAX_PAGE_LIMIT: u32 = 20;

// ─── Fixture ──────────────────────────────────────────────────────────────────

type Fixture = (Env, RevoraRevenueShareClient<'static>, Address, Address, Symbol, Address);

/// Deploy + initialize the contract and register one offering with the given
/// issuer/namespace/token. Returns
/// `(env, client, admin, issuer, namespace, token)`.
fn setup_with(ns: Symbol, token: &Address) -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let payout = Address::generate(&env);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns,
        token,
        &2_500u32,
        &payout,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
    (env, client, admin, issuer, ns, token.clone())
}

/// Standard fixture: one offering under namespace `ns1`.
fn setup() -> Fixture {
    let env = Env::default();
    let token = Address::generate(&env);
    setup_with(symbol_short!("ns1"), &token)
}

/// Add `count` distinct investors to the blacklist and return them in
/// insertion order.
fn add_investors(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    ns: &Symbol,
    token: &Address,
    count: u32,
) -> Vec<Address> {
    let mut added = Vec::new(env);
    for _ in 0..count {
        let investor = Address::generate(env);
        client
            .try_blacklist_add(issuer, issuer, ns, token, &investor)
            .expect("blacklist_add must succeed inside the size limit");
        added.push_back(investor);
    }
    added
}

// ─── 1. Empty and unknown-key pages ───────────────────────────────────────────

/// A freshly registered offering with an empty blacklist must page as empty
/// for every starting offset, with no cursor.
#[test]
fn empty_blacklist_pages_are_empty_for_every_start() {
    let (_env, client, _admin, issuer, ns, token) = setup();

    for start in [0u32, 1, 7, u32::MAX - 1] {
        let (page, next) = client.get_blacklist_page(&issuer, &ns, &token, &start, &10u32);
        assert_eq!(page.len(), 0, "empty blacklist must page empty at start={start}");
        assert_eq!(next, None, "empty blacklist must never report a cursor");
    }
}

/// A completely unknown issuer must not observe any other offering's blacklist.
#[test]
fn unknown_issuer_yields_empty_page() {
    let (env, client, _admin, issuer, ns, token) = setup();
    add_investors(&env, &client, &issuer, &ns, &token, 3);

    let stranger = Address::generate(&env);
    let (page, next) = client.get_blacklist_page(&stranger, &ns, &token, &0, &10u32);
    assert_eq!(page.len(), 0);
    assert_eq!(next, None);
}

/// The namespace and token form the storage key; swapping either must isolate
/// offerings from each other even under the same issuer.
#[test]
fn namespace_and_token_isolation() {
    let (env, client, _admin, issuer, _ns, _token) = setup();
    let ns_a = symbol_short!("nsA");
    let ns_b = symbol_short!("nsB");
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    for (ns, token) in [(&ns_a, &token_a), (&ns_b, &token_b)] {
        let payout = Address::generate(&env);
        client.register_offering(
            &issuer,
            &Vec::new(&env),
            &1u32,
            ns,
            token,
            &2_500u32,
            &payout,
            &0i128,
            &symbol_short!(""),
            &0u32,
        );
    }

    let victim = Address::generate(&env);
    client
        .try_blacklist_add(&issuer, &issuer, &ns_a, &token_a, &victim)
        .expect("add to offering A must succeed");

    // Offering B (different namespace AND token) must not see the entry.
    let (page_b, next_b) = client.get_blacklist_page(&issuer, &ns_b, &token_b, &0, &10u32);
    assert_eq!(page_b.len(), 0, "offerings must not share blacklist entries");
    assert_eq!(next_b, None);

    // Swapping just the namespace must also isolate.
    let (page_cross, _) = client.get_blacklist_page(&issuer, &ns_b, &token_a, &0, &10u32);
    assert_eq!(page_cross.len(), 0);

    // The original key still resolves.
    let (page_a, _) = client.get_blacklist_page(&issuer, &ns_a, &token_a, &0, &10u32);
    assert_eq!(page_a.len(), 1);
    assert_eq!(page_a.get(0).unwrap(), victim);
}

// ─── 2. Pagination bounds and limit clamping ─────────────────────────────────

/// `limit == 0` falls back to the default page size (`MAX_PAGE_LIMIT`), and
/// `limit > MAX_PAGE_LIMIT` is clamped down to it. Both clamped forms must be
/// byte-identical to an explicit `limit = 20` request.
#[test]
fn limit_zero_defaults_and_oversized_limit_is_clamped() {
    let (env, client, _admin, issuer, ns, token) = setup();
    add_investors(&env, &client, &issuer, &ns, &token, 25);

    // limit = 0 → default to MAX_PAGE_LIMIT (20), not 0.
    let (page_zero, next_zero) = client.get_blacklist_page(&issuer, &ns, &token, &0, &0u32);
    assert_eq!(page_zero.len(), MAX_PAGE_LIMIT, "limit=0 must use the default page size");
    assert_eq!(next_zero, Some(MAX_PAGE_LIMIT));

    // limit = u32::MAX → clamped to MAX_PAGE_LIMIT.
    let (page_huge, next_huge) = client.get_blacklist_page(&issuer, &ns, &token, &0, &u32::MAX);
    assert_eq!(page_huge.len(), MAX_PAGE_LIMIT, "oversized limit must be clamped");
    assert_eq!(next_huge, Some(MAX_PAGE_LIMIT));

    // Both clamped pages must be identical to the canonical limit=20 page.
    let (page_20, next_20) = client.get_blacklist_page(&issuer, &ns, &token, &0, &20u32);
    assert_eq!(page_zero, page_20);
    assert_eq!(page_huge, page_20);
    assert_eq!(next_zero, next_20);
    assert_eq!(next_huge, next_20);
}

/// With `count < limit`, the whole blacklist fits on one page and no cursor is
/// reported. The cursor must point at the next unread entry, and resuming from
/// that cursor yields the exact remainder.
#[test]
fn partial_page_and_cursor_arithmetic() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let investors = add_investors(&env, &client, &issuer, &ns, &token, 7);

    // Everything fits under the default limit.
    let (page_all, next_all) = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);
    assert_eq!(page_all.len(), 7);
    assert_eq!(next_all, None, "full read must not report a cursor");

    // limit=3: page 1 is entries 0..3 with cursor 3.
    let (page1, next1) = client.get_blacklist_page(&issuer, &ns, &token, &0, &3u32);
    assert_eq!(page1.len(), 3);
    assert_eq!(next1, Some(3));
    for i in 0..3u32 {
        assert_eq!(page1.get(i).unwrap(), investors.get(i).unwrap());
    }

    // Resuming from the cursor yields the remainder.
    let (page2, next2) = client.get_blacklist_page(&issuer, &ns, &token, &next1.unwrap(), &3u32);
    assert_eq!(page2.len(), 3);
    assert_eq!(next2, Some(6));
    for i in 0..3u32 {
        assert_eq!(page2.get(i).unwrap(), investors.get(3 + i).unwrap());
    }

    // Final page holds the last entry and no cursor.
    let (page3, next3) = client.get_blacklist_page(&issuer, &ns, &token, &next2.unwrap(), &3u32);
    assert_eq!(page3.len(), 1);
    assert_eq!(next3, None);
    assert_eq!(page3.get(0).unwrap(), investors.get(6).unwrap());
}

/// With an exact multiple of the page size, the last full page must report
/// `None` — never a stale cursor pointing at an empty follow-up page.
#[test]
fn exact_multiple_reports_no_cursor_on_last_page() {
    let (env, client, _admin, issuer, ns, token) = setup();
    add_investors(&env, &client, &issuer, &ns, &token, 20);

    let (page1, next1) = client.get_blacklist_page(&issuer, &ns, &token, &0, &20u32);
    assert_eq!(page1.len(), 20);
    assert_eq!(next1, None, "last full page must not report a cursor");

    // But an explicit offset at the end is still a valid empty page.
    let (page_end, next_end) = client.get_blacklist_page(&issuer, &ns, &token, &20u32, &20u32);
    assert_eq!(page_end.len(), 0);
    assert_eq!(next_end, None);
}

/// `start == len` and `start > len` (including `u32::MAX`) must return an empty
/// page with no cursor instead of panicking.
#[test]
fn start_at_or_past_len_returns_empty_page() {
    let (env, client, _admin, issuer, ns, token) = setup();
    add_investors(&env, &client, &issuer, &ns, &token, 5);

    for start in [5u32, 6, 100, u32::MAX] {
        let (page, next) = client.get_blacklist_page(&issuer, &ns, &token, &start, &10u32);
        assert_eq!(page.len(), 0, "start={start} past the end must page empty");
        assert_eq!(next, None, "start={start} past the end must not report a cursor");
    }
}

/// A tiny limit (1) still pages correctly through the whole list and matches
/// the single-page read element-for-element.
#[test]
fn tiny_limit_walk_matches_full_read() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let investors = add_investors(&env, &client, &issuer, &ns, &token, 6);

    let (full, _) = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);

    let mut walked = 0u32;
    let mut cursor = 0u32;
    loop {
        let (page, next) = client.get_blacklist_page(&issuer, &ns, &token, &cursor, &1u32);
        for i in 0..page.len() {
            assert_eq!(page.get(i).unwrap(), full.get(walked).unwrap());
            assert_eq!(page.get(i).unwrap(), investors.get(walked).unwrap());
            walked += 1;
        }
        match next {
            Some(c) => cursor = c,
            None => break,
        }
    }
    assert_eq!(walked, 6, "walking with limit=1 must visit every entry exactly once");
}

/// Repeated reads of the same window are deterministic.
#[test]
fn repeated_reads_are_deterministic() {
    let (env, client, _admin, issuer, ns, token) = setup();
    add_investors(&env, &client, &issuer, &ns, &token, 9);

    let (p1, c1) = client.get_blacklist_page(&issuer, &ns, &token, &2, &4u32);
    let (p2, c2) = client.get_blacklist_page(&issuer, &ns, &token, &2, &4u32);
    assert_eq!(p1, p2);
    assert_eq!(c1, c2);
}

// ─── 3. Removal shifts order; pages stay contiguous ──────────────────────────

/// `blacklist_remove` rebuilds the insertion-order vector. Removing a middle
/// entry must compact the list so pages remain contiguous and the entries
/// beyond the removal shift forward.
#[test]
fn removal_compacts_order_and_pages_stay_contiguous() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let investors = add_investors(&env, &client, &issuer, &ns, &token, 6);

    // Remove a middle entry (index 2).
    client
        .try_blacklist_remove(&issuer, &issuer, &ns, &token, &investors.get(2).unwrap())
        .expect("issuer may remove a blacklisted investor");

    // The ordered view now holds 5 entries: 0,1,3,4,5.
    let (page, next) = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);
    assert_eq!(page.len(), 5);
    assert_eq!(next, None);
    let expected = [0u32, 1, 3, 4, 5];
    for (i, e) in expected.iter().enumerate() {
        assert_eq!(page.get(i as u32).unwrap(), investors.get(*e).unwrap());
    }

    // get_blacklist_size agrees with the paged view.
    assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), 5);

    // Cursor arithmetic across the smaller list: limit=2 → pages of 2,2,1.
    let (p1, c1) = client.get_blacklist_page(&issuer, &ns, &token, &0, &2u32);
    assert_eq!(p1.len(), 2);
    assert_eq!(c1, Some(2));
    let (p2, c2) = client.get_blacklist_page(&issuer, &ns, &token, &c1.unwrap(), &2u32);
    assert_eq!(p2.len(), 2);
    assert_eq!(c2, Some(4));
    let (p3, c3) = client.get_blacklist_page(&issuer, &ns, &token, &c2.unwrap(), &2u32);
    assert_eq!(p3.len(), 1);
    assert_eq!(c3, None);
}

/// Re-adding a removed address appends it at the end of the insertion order
/// (not its original position).
#[test]
fn readd_goes_to_end_of_order() {
    let (env, client, _admin, issuer, ns, token) = setup();
    let investors = add_investors(&env, &client, &issuer, &ns, &token, 4);

    client
        .try_blacklist_remove(&issuer, &issuer, &ns, &token, &investors.get(0).unwrap())
        .expect("remove must succeed");
    client
        .try_blacklist_add(&issuer, &issuer, &ns, &token, &investors.get(0).unwrap())
        .expect("re-add must succeed");

    let (page, next) = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);
    assert_eq!(page.len(), 4);
    assert_eq!(next, None);
    assert_eq!(page.get(0).unwrap(), investors.get(1).unwrap());
    assert_eq!(page.get(3).unwrap(), investors.get(0).unwrap(), "re-added entry must be last");
}

// ─── 4. Gating: pause / freeze ────────────────────────────────────────────────

/// While soft-paused, `blacklist_add` is rejected, so the paged view cannot
/// change. The read itself stays available (it is a read-only view).
#[test]
fn soft_pause_blocks_adds_and_keeps_page_stable() {
    let (env, client, admin, issuer, ns, token) = setup();
    add_investors(&env, &client, &issuer, &ns, &token, 2);
    let before = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);

    client.pause_admin(&admin);

    // Reads keep working while paused.
    let during = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);
    assert_eq!(during, before, "pause must not change the paginated view");

    // Mutating the blacklist is blocked while paused.
    let intruder = Address::generate(&env);
    let res = client.try_blacklist_add(&issuer, &issuer, &ns, &token, &intruder);
    match res {
        Err(Ok(RevoraError::ContractPaused)) => {}
        other => panic!("expected ContractPaused, got: {:?}", other),
    }

    let after = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);
    assert_eq!(after, before, "rejected add must leave the paginated view unchanged");
}

/// While globally frozen, `blacklist_add` fails closed; the read path stays
/// observable so off-chain tooling can still page through the frozen state.
///
/// The global freeze flag is set directly through contract storage: the
/// freeze/unfreeze entrypoints live in an impl block that is not exported on
/// the generated client, so tests flip the same `DataKey::Frozen` flag the
/// contract's own `set_freeze` writes.
#[test]
fn freeze_blocks_adds_and_keeps_page_stable() {
    let (env, client, _admin, issuer, ns, token) = setup();
    add_investors(&env, &client, &issuer, &ns, &token, 2);
    let before = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);

    // Freeze the contract exactly as `set_freeze` would.
    let contract_address = client.address.clone();
    env.as_contract(&contract_address, || {
        env.storage().persistent().set(&crate::DataKey::Frozen, &true);
    });

    let intruder = Address::generate(&env);
    let res = client.try_blacklist_add(&issuer, &issuer, &ns, &token, &intruder);
    match res {
        Err(Ok(RevoraError::ContractFrozen)) => {}
        other => panic!("expected ContractFrozen, got: {:?}", other),
    }

    let during = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);
    assert_eq!(during, before, "freeze must not change the paginated view");
}

// ─── 5. Auth surface and consistency with the rest of the read API ───────────

/// The contract admin is an authorized caller for `blacklist_add`; entries the
/// admin adds are visible through pagination just like issuer-added entries.
#[test]
fn admin_caller_add_is_visible_in_page() {
    let (env, client, admin, issuer, ns, token) = setup();

    let via_issuer = Address::generate(&env);
    client.try_blacklist_add(&issuer, &issuer, &ns, &token, &via_issuer).expect("issuer may add");

    let via_admin = Address::generate(&env);
    client.try_blacklist_add(&admin, &issuer, &ns, &token, &via_admin).expect("admin may add");

    let (page, next) = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);
    assert_eq!(page.len(), 2);
    assert_eq!(next, None);
    assert_eq!(page.get(0).unwrap(), via_issuer);
    assert_eq!(page.get(1).unwrap(), via_admin);
}

/// A rejected unauthorized add must leave the paginated view byte-identical.
#[test]
fn rejected_unauthorized_add_leaves_page_unchanged() {
    let (env, client, _admin, issuer, ns, token) = setup();
    add_investors(&env, &client, &issuer, &ns, &token, 3);
    let before = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);

    let attacker = Address::generate(&env);
    let intruder = Address::generate(&env);
    let res = client.try_blacklist_add(&attacker, &issuer, &ns, &token, &intruder);
    match res {
        Err(Ok(RevoraError::NotAuthorized)) => {}
        other => panic!("expected NotAuthorized, got: {:?}", other),
    }

    let after = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);
    assert_eq!(after, before, "rejected add must leave the paginated view unchanged");
}

/// After every incremental add, the paged view and `get_blacklist_size` must
/// stay consistent, and the cursor must flip exactly when the list exceeds one
/// default page.
#[test]
fn page_stays_consistent_with_size_across_growth() {
    let (env, client, _admin, issuer, ns, token) = setup();

    for i in 0..21u32 {
        let investor = Address::generate(&env);
        client
            .try_blacklist_add(&issuer, &issuer, &ns, &token, &investor)
            .expect("add must succeed inside the size limit");

        let count = i + 1;
        assert_eq!(client.get_blacklist_size(&issuer, &ns, &token), count);

        let (page, next) = client.get_blacklist_page(&issuer, &ns, &token, &0, &MAX_PAGE_LIMIT);
        assert_eq!(page.len(), count.min(MAX_PAGE_LIMIT));
        if count <= MAX_PAGE_LIMIT {
            assert_eq!(next, None);
        } else {
            assert_eq!(next, Some(MAX_PAGE_LIMIT));
        }
    }
}
