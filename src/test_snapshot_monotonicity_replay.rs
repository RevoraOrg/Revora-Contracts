//! # Snapshot Monotonicity Replay Stress Tests
//!
//! **Issue:** `commit_snapshot` rejects with `OutdatedSnapshot` when
//! `snapshot_ref <= last_ref`, but the test suite did not exercise rapid
//! out-of-order replay attempts that emulate a buggy off-chain coordinator.
//!
//! **This module** adds a stress test that drives the ref sequence
//! `[5, 3, 5, 4, 6]` and asserts:
//!
//! - Ref **5** is accepted (first commit; `last_ref` advances to 5).
//! - Ref **3** is rejected with `OutdatedSnapshot` (`3 <= 5`).
//! - Ref **5** is rejected with `OutdatedSnapshot` (equal, not strictly greater).
//! - Ref **4** is rejected with `OutdatedSnapshot` (`4 <= 5`).
//! - Ref **6** is accepted (first strictly-greater ref; `last_ref` advances to 6).
//!
//! After the sequence `get_last_snapshot_ref` must return **6**.
//!
//! ## Security Assumptions Validated
//!
//! 1. **Strict monotonicity** — only `snapshot_ref > last_ref` is accepted.
//!    Equal refs are treated as replay attempts and rejected.
//! 2. **No partial state mutation** — rejected calls leave `last_ref` unchanged.
//! 3. **Forward-only advancement** — `last_ref` never decreases.
//! 4. **Typed errors** — every rejection surfaces as `RevoraError::OutdatedSnapshot`
//!    via `try_commit_snapshot`, enabling callers to distinguish this condition
//!    from other failures without string matching.
//!
//! ## Edge Cases Covered
//!
//! | Scenario                        | Expected outcome              |
//! |---------------------------------|-------------------------------|
//! | `snapshot_ref == 0`             | `OutdatedSnapshot` (0 ≤ 0)   |
//! | `snapshot_ref == u64::MAX`      | Accepted; `last_ref` = MAX    |
//! | Equal-ref retry after MAX       | `OutdatedSnapshot`            |
//! | Out-of-order replay `[5,3,5,4,6]` | Only 5 and 6 accepted       |
//!
//! ## Test Coverage
//!
//! All tests use `try_commit_snapshot` to assert typed `RevoraError` variants.
//! No `unwrap()` or `expect()` calls are used in assertion paths.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, BytesN as _},
    Address, BytesN, Env,
};

// ══════════════════════════════════════════════════════════════════════════════
// Shared helpers
// ══════════════════════════════════════════════════════════════════════════════

/// Spin up a fresh environment, register an offering, enable snapshots, and
/// return everything the tests need.
///
/// The offering is registered with:
/// - namespace: `"def"`
/// - revenue_share_bps: 5 000 (50 %)
/// - snapshot distribution: **enabled**
fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &5_000,
        &payout_asset,
        &0,
    );

    // Enable snapshot distribution for this offering.
    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);

    (env, client, issuer, token)
}

/// Generate a deterministic-looking 32-byte content hash.
/// Using `BytesN::random` is fine in tests; the contract stores it verbatim.
fn hash(env: &Env) -> BytesN<32> {
    BytesN::random(env)
}

// ══════════════════════════════════════════════════════════════════════════════
// Primary stress test — out-of-order replay sequence [5, 3, 5, 4, 6]
// ══════════════════════════════════════════════════════════════════════════════

/// Drive the ref sequence `[5, 3, 5, 4, 6]` and assert that only refs 5 and 6
/// are accepted while every other attempt returns `OutdatedSnapshot`.
///
/// This emulates a buggy off-chain coordinator that re-sends stale or duplicate
/// snapshot references in rapid succession.
#[test]
fn snapshot_replay_stress_out_of_order_sequence() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    // ── Step 1: ref 5 — first commit, must succeed ────────────────────────
    let result_5a = client.try_commit_snapshot(&issuer, &ns, &token, &5, &hash(&env));
    assert!(
        result_5a.is_ok(),
        "ref 5 (first commit) must be accepted; got: {:?}",
        result_5a.err()
    );
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token),
        5,
        "last_ref must advance to 5 after first commit"
    );

    // ── Step 2: ref 3 — below last_ref (3 < 5), must be rejected ─────────
    let result_3 = client.try_commit_snapshot(&issuer, &ns, &token, &3, &hash(&env));
    assert!(
        matches!(result_3, Err(Ok(RevoraError::OutdatedSnapshot))),
        "ref 3 (< last_ref 5) must return OutdatedSnapshot; got: {:?}",
        result_3
    );
    // last_ref must not have changed
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token),
        5,
        "last_ref must remain 5 after rejected ref 3"
    );

    // ── Step 3: ref 5 — equal to last_ref (5 == 5), must be rejected ─────
    let result_5b = client.try_commit_snapshot(&issuer, &ns, &token, &5, &hash(&env));
    assert!(
        matches!(result_5b, Err(Ok(RevoraError::OutdatedSnapshot))),
        "ref 5 (== last_ref 5) must return OutdatedSnapshot; got: {:?}",
        result_5b
    );
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token),
        5,
        "last_ref must remain 5 after duplicate ref 5"
    );

    // ── Step 4: ref 4 — below last_ref (4 < 5), must be rejected ─────────
    let result_4 = client.try_commit_snapshot(&issuer, &ns, &token, &4, &hash(&env));
    assert!(
        matches!(result_4, Err(Ok(RevoraError::OutdatedSnapshot))),
        "ref 4 (< last_ref 5) must return OutdatedSnapshot; got: {:?}",
        result_4
    );
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token),
        5,
        "last_ref must remain 5 after rejected ref 4"
    );

    // ── Step 5: ref 6 — strictly greater (6 > 5), must succeed ───────────
    let result_6 = client.try_commit_snapshot(&issuer, &ns, &token, &6, &hash(&env));
    assert!(
        result_6.is_ok(),
        "ref 6 (> last_ref 5) must be accepted; got: {:?}",
        result_6.err()
    );

    // ── Final invariant: last_ref == 6 ────────────────────────────────────
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token),
        6,
        "last_ref must be 6 after the full replay sequence"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Edge case: snapshot_ref == 0
// ══════════════════════════════════════════════════════════════════════════════

/// `snapshot_ref == 0` must be rejected because `last_ref` starts at 0 and the
/// invariant requires `snapshot_ref > last_ref` (strictly greater).
///
/// This prevents a coordinator from accidentally committing a "null" snapshot.
#[test]
fn snapshot_ref_zero_is_rejected() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    let result = client.try_commit_snapshot(&issuer, &ns, &token, &0, &hash(&env));
    assert!(
        matches!(result, Err(Ok(RevoraError::OutdatedSnapshot))),
        "ref 0 must return OutdatedSnapshot (0 <= initial last_ref 0); got: {:?}",
        result
    );

    // State must be pristine.
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token),
        0,
        "last_ref must remain 0 after rejected ref 0"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Edge case: snapshot_ref == u64::MAX
// ══════════════════════════════════════════════════════════════════════════════

/// `u64::MAX` is a valid snapshot reference and must be accepted when it is
/// strictly greater than the current `last_ref`.
///
/// After acceptance, any subsequent commit (including another `u64::MAX`) must
/// be rejected because no value can exceed `u64::MAX`.
#[test]
fn snapshot_ref_u64_max_is_accepted_then_blocks_further_commits() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    // Commit at u64::MAX — must succeed.
    let result_max = client.try_commit_snapshot(&issuer, &ns, &token, &u64::MAX, &hash(&env));
    assert!(
        result_max.is_ok(),
        "ref u64::MAX must be accepted; got: {:?}",
        result_max.err()
    );
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token),
        u64::MAX,
        "last_ref must be u64::MAX after commit"
    );

    // Any subsequent commit (equal or lower) must be rejected.
    let result_retry = client.try_commit_snapshot(&issuer, &ns, &token, &u64::MAX, &hash(&env));
    assert!(
        matches!(result_retry, Err(Ok(RevoraError::OutdatedSnapshot))),
        "retry at u64::MAX must return OutdatedSnapshot; got: {:?}",
        result_retry
    );

    // last_ref must not have changed.
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token),
        u64::MAX,
        "last_ref must remain u64::MAX after rejected retry"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Edge case: equal-ref retry after a normal commit
// ══════════════════════════════════════════════════════════════════════════════

/// Committing the same `snapshot_ref` twice in a row must always return
/// `OutdatedSnapshot` on the second attempt, regardless of the ref value.
///
/// This is the canonical idempotency-rejection test: the contract is
/// write-once per ref, not idempotent.
#[test]
fn equal_ref_retry_always_returns_outdated_snapshot() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    // First commit at ref 42 — must succeed.
    let first = client.try_commit_snapshot(&issuer, &ns, &token, &42, &hash(&env));
    assert!(first.is_ok(), "first commit at ref 42 must succeed; got: {:?}", first.err());

    // Immediate retry at the same ref — must fail.
    let retry = client.try_commit_snapshot(&issuer, &ns, &token, &42, &hash(&env));
    assert!(
        matches!(retry, Err(Ok(RevoraError::OutdatedSnapshot))),
        "retry at ref 42 must return OutdatedSnapshot; got: {:?}",
        retry
    );

    // last_ref must still be 42.
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token),
        42,
        "last_ref must remain 42 after rejected retry"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Monotonicity invariant: last_ref never decreases
// ══════════════════════════════════════════════════════════════════════════════

/// After a sequence of mixed accepted and rejected commits, `last_ref` must
/// equal the highest accepted ref and must never have decreased at any point.
///
/// Sequence: accept 10 → reject 7 → reject 10 → accept 20 → reject 15
/// Expected final `last_ref`: 20.
#[test]
fn last_ref_never_decreases_across_mixed_sequence() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");

    // Accept ref 10.
    assert!(client.try_commit_snapshot(&issuer, &ns, &token, &10, &hash(&env)).is_ok());
    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 10);

    // Reject ref 7 (< 10).
    assert!(matches!(
        client.try_commit_snapshot(&issuer, &ns, &token, &7, &hash(&env)),
        Err(Ok(RevoraError::OutdatedSnapshot))
    ));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 10);

    // Reject ref 10 (== 10).
    assert!(matches!(
        client.try_commit_snapshot(&issuer, &ns, &token, &10, &hash(&env)),
        Err(Ok(RevoraError::OutdatedSnapshot))
    ));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 10);

    // Accept ref 20 (> 10).
    assert!(client.try_commit_snapshot(&issuer, &ns, &token, &20, &hash(&env)).is_ok());
    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token), 20);

    // Reject ref 15 (< 20).
    assert!(matches!(
        client.try_commit_snapshot(&issuer, &ns, &token, &15, &hash(&env)),
        Err(Ok(RevoraError::OutdatedSnapshot))
    ));

    // Final invariant.
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token),
        20,
        "last_ref must be 20 (highest accepted ref) after mixed sequence"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Isolation: replay on one offering does not affect another
// ══════════════════════════════════════════════════════════════════════════════

/// Snapshot state is scoped per `(issuer, namespace, token)`. Replaying stale
/// refs on offering A must not corrupt the `last_ref` of offering B.
#[test]
fn snapshot_replay_is_isolated_per_offering() {
    let (env, client, issuer, token_a) = setup();
    let ns = symbol_short!("def");

    // Register a second offering (different token, same issuer/namespace).
    let token_b = Address::generate(&env);
    let payout_b = Address::generate(&env);
    client.register_offering(&issuer, &ns, &token_b, &5_000, &payout_b, &0);
    client.set_snapshot_config(&issuer, &ns, &token_b, &true);

    // Advance offering A to ref 100.
    assert!(client.try_commit_snapshot(&issuer, &ns, &token_a, &100, &hash(&env)).is_ok());

    // Replay stale refs on offering A.
    let _ = client.try_commit_snapshot(&issuer, &ns, &token_a, &50, &hash(&env));
    let _ = client.try_commit_snapshot(&issuer, &ns, &token_a, &100, &hash(&env));

    // Offering B is still at 0 — untouched.
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token_b),
        0,
        "offering B last_ref must remain 0; replay on A must not bleed over"
    );

    // Offering A is still at 100.
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &ns, &token_a),
        100,
        "offering A last_ref must remain 100 after replay attempts"
    );

    // Offering B can still accept its own first commit.
    assert!(client.try_commit_snapshot(&issuer, &ns, &token_b, &1, &hash(&env)).is_ok());
    assert_eq!(client.get_last_snapshot_ref(&issuer, &ns, &token_b), 1);
}
