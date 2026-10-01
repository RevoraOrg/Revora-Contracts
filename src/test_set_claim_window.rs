//! # `set_claim_window` — configuration and boundary coverage
//!
//! `set_claim_window` writes the `AccessWindow { start_timestamp, end_timestamp }`
//! that gates the claim flow. These tests pin the observable contract of the
//! entrypoint from the outside (via the generated client) so that a regression in
//! authorization, boundary validation, offering resolution, freeze handling or
//! storage isolation is caught immediately.
//!
//! Covered here:
//! - the happy path and the value that `get_claim_window` reads back (default `None`)
//! - inclusive boundaries: `start == end`, `start == 0`, `u64::MAX` edges
//! - rejected reconfigurations (inverted range) leave the previous window intact
//! - unknown offering / namespace / issuer are rejected and never create state
//! - overwrite and idempotent re-set semantics
//! - per-namespace, per-issuer and per-token isolation of the stored window
//! - a globally frozen contract rejects the write without touching storage
//! - the claim window is independent of the report window
//! - an event is emitted on success and no event is emitted on rejection

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, Vec,
};

/// Register a fresh contract with one offering owned by `issuer` under `ns`/`token`.
fn setup() -> (Env, Address, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let _ = client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    (env, contract_id, client, issuer, token)
}

// ─── 1. Happy path ────────────────────────────────────────────────────────────

#[test]
fn claim_window_is_unset_by_default() {
    let (_env, _id, client, issuer, token) = setup();

    assert_eq!(
        client.get_claim_window(&issuer, &symbol_short!("ns"), &token),
        None,
        "a freshly registered offering must not have a claim window"
    );
}

#[test]
fn set_claim_window_accepts_valid_range_and_is_readable() {
    let (_env, _id, client, issuer, token) = setup();

    let result = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    assert!(result.is_ok(), "valid range must be accepted, got {result:?}");

    let window = client
        .get_claim_window(&issuer, &symbol_short!("ns"), &token)
        .expect("window must be readable after set_claim_window");
    assert_eq!(window.start_timestamp, 1_000);
    assert_eq!(window.end_timestamp, 2_000);
}

#[test]
fn set_claim_window_accepts_zero_width_window() {
    let (_env, _id, client, issuer, token) = setup();

    // `start == end` is a legal single-instant window: the validation rule is
    // `start <= end`, not `start < end`.
    let result = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &7, &7);
    assert!(result.is_ok(), "start == end must be accepted, got {result:?}");

    let window = client
        .get_claim_window(&issuer, &symbol_short!("ns"), &token)
        .expect("window must be readable");
    assert_eq!(window.start_timestamp, 7);
    assert_eq!(window.end_timestamp, 7);
}

#[test]
fn set_claim_window_accepts_zero_start() {
    let (_env, _id, client, issuer, token) = setup();

    let result = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &0, &500);
    assert!(result.is_ok(), "start == 0 must be accepted, got {result:?}");

    let window = client
        .get_claim_window(&issuer, &symbol_short!("ns"), &token)
        .expect("window must be readable");
    assert_eq!(window.start_timestamp, 0);
    assert_eq!(window.end_timestamp, 500);
}

#[test]
fn set_claim_window_accepts_u64_max_boundaries() {
    let (_env, _id, client, issuer, token) = setup();

    // Upper edge: the widest legal window that does not overflow.
    let result = client.try_set_claim_window(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &(u64::MAX - 1),
        &u64::MAX,
    );
    assert!(result.is_ok(), "u64::MAX edges must be accepted, got {result:?}");

    let window = client
        .get_claim_window(&issuer, &symbol_short!("ns"), &token)
        .expect("window must be readable");
    assert_eq!(window.start_timestamp, u64::MAX - 1);
    assert_eq!(window.end_timestamp, u64::MAX);

    // Degenerate upper edge: both ends pinned at u64::MAX is still `start == end`.
    let result = client.try_set_claim_window(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &u64::MAX,
        &u64::MAX,
    );
    assert!(result.is_ok(), "u64::MAX == u64::MAX must be accepted, got {result:?}");
}

#[test]
fn set_claim_window_rejects_u64_max_overflow_pair() {
    let (_env, _id, client, issuer, token) = setup();

    let result = client.try_set_claim_window(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &u64::MAX,
        &(u64::MAX - 1),
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::LimitReached)),
        "an inverted range at the u64 edge must be rejected"
    );
}

// ─── 2. Inverted range ────────────────────────────────────────────────────────

#[test]
fn set_claim_window_rejects_inverted_range() {
    let (_env, _id, client, issuer, token) = setup();

    let result = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &2_000, &1_000);
    assert_eq!(
        result,
        Err(Ok(RevoraError::LimitReached)),
        "start > end must be rejected"
    );

    assert_eq!(
        client.get_claim_window(&issuer, &symbol_short!("ns"), &token),
        None,
        "a rejected configuration must not create a window"
    );
}

#[test]
fn rejected_reconfiguration_keeps_the_previous_window() {
    let (_env, _id, client, issuer, token) = setup();

    assert!(client
        .try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &100, &200)
        .is_ok());

    let result = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &900, &300);
    assert_eq!(result, Err(Ok(RevoraError::LimitReached)));

    let window = client
        .get_claim_window(&issuer, &symbol_short!("ns"), &token)
        .expect("the previously stored window must survive a rejected overwrite");
    assert_eq!(window.start_timestamp, 100);
    assert_eq!(window.end_timestamp, 200);
}

// ─── 3. Offering resolution / authorization ──────────────────────────────────

#[test]
fn set_claim_window_rejects_unregistered_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let result = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &10, &20);
    assert_eq!(
        result,
        Err(Ok(RevoraError::OfferingNotFound)),
        "an unregistered offering must be rejected"
    );
    assert_eq!(client.get_claim_window(&issuer, &symbol_short!("ns"), &token), None);
}

#[test]
fn set_claim_window_rejects_wrong_issuer_and_leaves_state_untouched() {
    let (env, _id, client, issuer, token) = setup();

    let stranger = Address::generate(&env);
    let result = client.try_set_claim_window(&stranger, &symbol_short!("ns"), &token, &10, &20);
    assert_eq!(
        result,
        Err(Ok(RevoraError::OfferingNotFound)),
        "only the offering issuer may configure the window"
    );

    // The real issuer's offering is still unconfigured.
    assert_eq!(client.get_claim_window(&issuer, &symbol_short!("ns"), &token), None);
}

#[test]
fn set_claim_window_rejects_unknown_namespace() {
    let (_env, _id, client, issuer, token) = setup();

    let result = client.try_set_claim_window(&issuer, &symbol_short!("other"), &token, &10, &20);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(
        client.get_claim_window(&issuer, &symbol_short!("other"), &token),
        None
    );
}

#[test]
fn set_claim_window_rejects_unknown_token() {
    let (env, _id, client, issuer, _token) = setup();
    let other_token = Address::generate(&env);

    let result = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &other_token, &10, &20);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

// ─── 4. Overwrite / idempotency ──────────────────────────────────────────────

#[test]
fn set_claim_window_overwrites_previous_value() {
    let (_env, _id, client, issuer, token) = setup();

    assert!(client
        .try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &100, &200)
        .is_ok());
    assert!(client
        .try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &300, &400)
        .is_ok());

    let window = client
        .get_claim_window(&issuer, &symbol_short!("ns"), &token)
        .expect("window must exist");
    assert_eq!(window.start_timestamp, 300);
    assert_eq!(window.end_timestamp, 400);
}

#[test]
fn set_claim_window_is_idempotent_for_identical_values() {
    let (_env, _id, client, issuer, token) = setup();

    let first = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &50, &75);
    let second = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &50, &75);

    assert_eq!(first, Ok(()));
    assert_eq!(second, Ok(()), "re-setting the same window must succeed");

    let window = client
        .get_claim_window(&issuer, &symbol_short!("ns"), &token)
        .expect("window must exist");
    assert_eq!(window.start_timestamp, 50);
    assert_eq!(window.end_timestamp, 75);
}

// ─── 5. Isolation between offerings ─────────────────────────────────────────

#[test]
fn claim_window_is_scoped_per_namespace() {
    let (env, _id, client, issuer, token) = setup();

    let _ = client.register_offering(
        &issuer,
        &Vec::new(&env),
        &2u32,
        &symbol_short!("ns2"),
        &token,
        &1_000,
        &token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    assert!(client
        .try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &10, &20)
        .is_ok());

    assert!(client
        .get_claim_window(&issuer, &symbol_short!("ns"), &token)
        .is_some());
    assert_eq!(
        client.get_claim_window(&issuer, &symbol_short!("ns2"), &token),
        None,
        "a sibling namespace must not inherit the window"
    );
}

#[test]
fn claim_window_is_scoped_per_token() {
    let (env, _id, client, issuer, token) = setup();
    let second_token = Address::generate(&env);

    let _ = client.register_offering(
        &issuer,
        &Vec::new(&env),
        &2u32,
        &symbol_short!("ns"),
        &second_token,
        &1_000,
        &second_token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    assert!(client
        .try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &10, &20)
        .is_ok());

    assert_eq!(
        client.get_claim_window(&issuer, &symbol_short!("ns"), &second_token),
        None,
        "a sibling token must not inherit the window"
    );
}

#[test]
fn claim_window_is_scoped_per_issuer() {
    let (env, _id, client, issuer, token) = setup();
    let second_issuer = Address::generate(&env);

    let _ = client.register_offering(
        &second_issuer,
        &Vec::new(&env),
        &2u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &token,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    assert!(client
        .try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &10, &20)
        .is_ok());

    assert_eq!(
        client.get_claim_window(&second_issuer, &symbol_short!("ns"), &token),
        None,
        "a different issuer's offering must not inherit the window"
    );
}

// ─── 6. Freeze handling ──────────────────────────────────────────────────────

#[test]
fn frozen_contract_rejects_set_claim_window() {
    let (env, _id, client, issuer, token) = setup();

    // Configure a window first so we can prove the freeze does not erase it.
    assert!(client
        .try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &10, &20)
        .is_ok());

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.freeze();

    let result = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &30, &40);
    assert_eq!(
        result,
        Err(Ok(RevoraError::ContractFrozen)),
        "a frozen contract must reject claim window changes"
    );

    let window = client
        .get_claim_window(&issuer, &symbol_short!("ns"), &token)
        .expect("the pre-freeze window must remain readable");
    assert_eq!(window.start_timestamp, 10);
    assert_eq!(window.end_timestamp, 20);
}

// ─── 7. Independence from the report window ─────────────────────────────────

#[test]
fn claim_window_is_independent_from_report_window() {
    let (_env, _id, client, issuer, token) = setup();

    assert!(client
        .try_set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000)
        .is_ok());
    assert!(client
        .try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &5_000, &6_000)
        .is_ok());

    let report = client
        .get_report_window(&issuer, &symbol_short!("ns"), &token)
        .expect("report window must exist");
    assert_eq!(report.start_timestamp, 1_000);
    assert_eq!(report.end_timestamp, 2_000);

    let claim = client
        .get_claim_window(&issuer, &symbol_short!("ns"), &token)
        .expect("claim window must exist");
    assert_eq!(claim.start_timestamp, 5_000);
    assert_eq!(claim.end_timestamp, 6_000);
}

// ─── 8. Event emission ───────────────────────────────────────────────────────

#[test]
fn set_claim_window_emits_an_event_on_success() {
    let (env, _id, client, issuer, token) = setup();

    let before = env.events().all().len();
    assert!(client
        .try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &11, &22)
        .is_ok());
    let after = env.events().all().len();

    assert!(
        after > before,
        "a successful configuration must be observable (expected an event)"
    );
}

#[test]
fn set_claim_window_emits_no_event_on_rejection() {
    let (env, _id, client, issuer, token) = setup();

    let before = env.events().all().len();
    let result = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &99, &1);
    assert_eq!(result, Err(Ok(RevoraError::LimitReached)));

    assert_eq!(
        env.events().all().len(),
        before,
        "a rejected configuration must not emit an event"
    );
}

// ─── 9. Ledger independence ──────────────────────────────────────────────────

#[test]
fn claim_window_persists_across_ledger_advances() {
    let (env, _id, client, issuer, token) = setup();

    assert!(client
        .try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000)
        .is_ok());

    env.ledger().with_mut(|l| {
        l.timestamp = 1_500;
        l.sequence_number += 25;
    });

    let window = client
        .get_claim_window(&issuer, &symbol_short!("ns"), &token)
        .expect("window must survive a ledger advance");
    assert_eq!(window.start_timestamp, 1_000);
    assert_eq!(window.end_timestamp, 2_000);
}
