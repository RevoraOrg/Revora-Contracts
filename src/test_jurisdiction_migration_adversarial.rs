//! Adversarial coverage for [`RevoraRevenueShare::get_jurisdiction_migration`].
//!
//! `get_jurisdiction_migration` is an unauthenticated, read-only getter for a
//! holder's *pending* jurisdiction migration entry. It is the read half of the
//! grace-period workflow: `require_jurisdiction_migration_not_expired` consumes
//! the same `DataKey2::JurisdictionMigration(offering, holder)` slot when it
//! finalises an expired migration.
//!
//! ## Coverage boundary (read this before extending the suite)
//!
//! On the revision this suite targets, **no code path writes a
//! `DataKey2::JurisdictionMigration` entry** and the
//! `JurisdictionMigrationState` type is not declared in `src/lib.rs` (the only
//! other reference lives in the orphaned, uncompiled `src/test_jurisdiction.rs`).
//! Consequently the `Some(..)` success path is **unreachable**: every call
//! returns `None`. `set_holder_jurisdiction` documents that a future
//! `effective_ts` "emits a migration event and schedules a compliance deadline",
//! but its implementation ignores `effective_ts` and writes only
//! `HolderJurisdiction`.
//!
//! These tests therefore pin the *observable* contract and act as change
//! detectors around that gap:
//!
//! * the getter returns `None` for unconfigured, unknown, and configured
//!   offerings alike, for every coordinate combination;
//! * an immediate (`effective_ts == 0`) and a future `effective_ts` both leave
//!   no pending-migration entry behind — so if a pending-migration write is ever
//!   implemented, `future_effective_timestamp_does_not_create_a_pending_migration`
//!   fails loudly and the `Some(..)` cases must be added here;
//! * the getter is non-mutating and stable across repeated/interleaved reads;
//! * reads are coordinate-scoped and unaffected by a global freeze.

#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

/// Documented default for the jurisdiction grace period (7 days).
const DEFAULT_GRACE_SECS: u64 = 7 * 24 * 60 * 60;

fn register_offering(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
) {
    let payout_asset = Address::generate(env);
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        &symbol_short!("def"),
        token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
}

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &token);
    (env, client, issuer, token)
}

#[test]
fn returns_none_for_an_unconfigured_offering() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    assert!(client.get_jurisdiction_migration(&issuer, &ns, &token, &holder).is_none());
}

#[test]
fn returns_none_for_an_unknown_offering() {
    let (env, client, issuer, _token) = setup();
    let ns = symbol_short!("def");
    let unknown_token = Address::generate(&env);
    let holder = Address::generate(&env);

    // The getter does not validate coordinates; it must not fail for an offering
    // that was never registered.
    assert!(client.get_jurisdiction_migration(&issuer, &ns, &unknown_token, &holder).is_none());
}

#[test]
fn returns_none_after_an_immediate_jurisdiction_change() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    // effective_ts == 0 applies the jurisdiction immediately: no grace period,
    // hence no pending migration entry.
    client.set_holder_jurisdiction(&issuer, &ns, &token, &holder, &symbol_short!("NG"), &0);

    assert!(client.get_jurisdiction_migration(&issuer, &ns, &token, &holder).is_none());
    // The neighbouring read confirms the direct write did land.
    assert_eq!(
        client.get_holder_jurisdiction(&issuer, &ns, &token, &holder),
        Some(symbol_short!("NG"))
    );
}

#[test]
fn future_effective_timestamp_does_not_create_a_pending_migration() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);
    let future = env.ledger().timestamp() + 86_400;

    client.set_holder_jurisdiction(&issuer, &ns, &token, &holder, &symbol_short!("NG"), &future);

    // CHANGE DETECTOR. Today `set_holder_jurisdiction` ignores `effective_ts`
    // and never writes a pending-migration entry, even though its doc comment
    // says a compliance deadline is scheduled. When that write is implemented,
    // this assertion fails and the `Some(..)` cases belong here.
    assert!(client.get_jurisdiction_migration(&issuer, &ns, &token, &holder).is_none());

    // The documented default grace period is still readable and unrelated to the
    // (absent) migration entry.
    assert_eq!(client.get_jurisdiction_grace_period(&issuer, &ns, &token), DEFAULT_GRACE_SECS);
}

#[test]
fn queries_are_read_only_and_stable_across_repeats() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    client.set_holder_jurisdiction(&issuer, &ns, &token, &holder, &symbol_short!("NG"), &0);

    for _ in 0..3 {
        assert!(client.get_jurisdiction_migration(&issuer, &ns, &token, &holder).is_none());
    }

    // The read must not have disturbed the neighbouring jurisdiction state.
    assert_eq!(
        client.get_holder_jurisdiction(&issuer, &ns, &token, &holder),
        Some(symbol_short!("NG"))
    );
}

#[test]
fn queries_are_scoped_per_holder_and_per_offering() {
    let (env, client, issuer_a, token_a) = setup();
    let ns = symbol_short!("def");

    let holder_listed = Address::generate(&env);
    let holder_unlisted = Address::generate(&env);
    client.set_holder_jurisdiction(
        &issuer_a,
        &ns,
        &token_a,
        &holder_listed,
        &symbol_short!("NG"),
        &0,
    );

    // A configured holder and an untouched holder are indistinguishable through
    // this getter: both report no pending migration.
    assert!(client.get_jurisdiction_migration(&issuer_a, &ns, &token_a, &holder_listed).is_none());
    assert!(client
        .get_jurisdiction_migration(&issuer_a, &ns, &token_a, &holder_unlisted)
        .is_none());

    // A sibling offering reports no pending migration either.
    let issuer_b = Address::generate(&env);
    let token_b = Address::generate(&env);
    register_offering(&env, &client, &issuer_b, &token_b);
    client.set_holder_jurisdiction(
        &issuer_b,
        &ns,
        &token_b,
        &holder_listed,
        &symbol_short!("NG"),
        &0,
    );

    assert!(client.get_jurisdiction_migration(&issuer_b, &ns, &token_b, &holder_listed).is_none());
}

#[test]
fn reads_are_unaffected_by_a_global_freeze() {
    let (env, client, issuer, token) = setup();
    let ns = symbol_short!("def");
    let holder = Address::generate(&env);

    client.set_holder_jurisdiction(&issuer, &ns, &token, &holder, &symbol_short!("NG"), &0);
    client.freeze();

    // A read-only getter must keep answering while the contract is frozen.
    assert!(client.get_jurisdiction_migration(&issuer, &ns, &token, &holder).is_none());
}
