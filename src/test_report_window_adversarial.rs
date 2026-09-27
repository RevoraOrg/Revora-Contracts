//! # Adversarial coverage for `set_report_window`
//!
//! `set_report_window` is a state-mutating, issuer-only entrypoint. Beyond the
//! happy-path boundary matrix already exercised elsewhere, this suite pins the
//! *rejection* contract and, critically, that a rejected call never mutates
//! stored window state.
//!
//! ## Coverage matrix
//!
//! | Scenario | Expected |
//! |---|---|
//! | Valid range on a registered offering | accepted, readable via `get_report_window` |
//! | `start == end` (zero-width) | accepted |
//! | `start == 0 && end == 0` | accepted |
//! | `start == 0 && end == u64::MAX` | accepted |
//! | `start > end` | `LimitReached`, no window stored |
//! | Unknown offering | `OfferingNotFound`, no window stored |
//! | Wrong issuer for an existing (namespace, token) | `OfferingNotFound` |
//! | Wrong namespace for an existing token | `OfferingNotFound` |
//! | Rejected reconfigure of an existing window | previous window preserved |
//! | Rejected call | no event emitted |
//!
//! ## Notes / intentional gaps
//!
//! - Host-level `require_auth` failures panic and cannot be caught with `try_*`
//!   in this `no_std` test harness; they are covered by `test_auth.rs` and are
//!   therefore intentionally out of scope here.
//! - Report and claim windows are independent storage slots; this suite asserts
//!   that setting a report window does not create a claim window.

#![cfg(test)]
#![allow(unused_imports)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::testutils::Events as _;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Register a minimal offering owned by `issuer` under (`namespace`, `token`).
fn register(
    client: &RevoraRevenueShareClient,
    env: &Env,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) {
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        namespace,
        token,
        &1_000,
        token,
        &0,
        &symbol_short!(""),
        &0,
    );
}

#[test]
fn valid_window_is_accepted_and_readable() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);

    let r = client.try_set_report_window(&issuer, &ns, &token, &1_234, &5_678);
    assert!(r.is_ok(), "valid window must be accepted");

    let window = client.get_report_window(&issuer, &ns, &token).unwrap();
    assert_eq!(window.start_timestamp, 1_234);
    assert_eq!(window.end_timestamp, 5_678);
}

#[test]
fn zero_width_window_is_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);

    let r = client.try_set_report_window(&issuer, &ns, &token, &5_000, &5_000);
    assert!(r.is_ok(), "zero-width window is a valid single-second slot");

    let window = client.get_report_window(&issuer, &ns, &token).unwrap();
    assert_eq!(window.start_timestamp, 5_000);
    assert_eq!(window.end_timestamp, 5_000);
}

#[test]
fn boundary_timestamps_are_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);

    assert!(
        client.try_set_report_window(&issuer, &ns, &token, &0, &0).is_ok(),
        "epoch-zero window must be accepted"
    );
    assert!(
        client.try_set_report_window(&issuer, &ns, &token, &0, &u64::MAX).is_ok(),
        "full-range window must be accepted"
    );

    let window = client.get_report_window(&issuer, &ns, &token).unwrap();
    assert_eq!(window.start_timestamp, 0);
    assert_eq!(window.end_timestamp, u64::MAX);
}

#[test]
fn inverted_range_is_rejected_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);

    let r = client.try_set_report_window(&issuer, &ns, &token, &2_000, &1_000);
    assert_eq!(r, Err(Ok(RevoraError::LimitReached)));

    assert!(
        client.get_report_window(&issuer, &ns, &token).is_none(),
        "rejected call must not persist a window"
    );
}

#[test]
fn unknown_offering_is_rejected_and_state_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");

    let r = client.try_set_report_window(&issuer, &ns, &token, &1_000, &2_000);
    assert_eq!(r, Err(Ok(RevoraError::OfferingNotFound)));

    assert!(client.get_report_window(&issuer, &ns, &token).is_none());
}

#[test]
fn wrong_issuer_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);

    // A different, unrelated issuer cannot configure this offering's window.
    let attacker = Address::generate(&env);
    let r = client.try_set_report_window(&attacker, &ns, &token, &1_000, &2_000);
    assert_eq!(r, Err(Ok(RevoraError::OfferingNotFound)));

    assert!(client.get_report_window(&issuer, &ns, &token).is_none());
}

#[test]
fn wrong_namespace_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    register(&client, &env, &issuer, &symbol_short!("aa"), &token);

    let r = client.try_set_report_window(&issuer, &symbol_short!("bb"), &token, &1_000, &2_000);
    assert_eq!(r, Err(Ok(RevoraError::OfferingNotFound)));

    assert!(client.get_report_window(&issuer, &symbol_short!("aa"), &token).is_none());
}

#[test]
fn rejected_reconfigure_preserves_previous_window() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);

    assert!(client.try_set_report_window(&issuer, &ns, &token, &1_000, &2_000).is_ok());

    let r = client.try_set_report_window(&issuer, &ns, &token, &3_000, &2_500);
    assert_eq!(r, Err(Ok(RevoraError::LimitReached)));

    let window = client.get_report_window(&issuer, &ns, &token).unwrap();
    assert_eq!(window.start_timestamp, 1_000);
    assert_eq!(window.end_timestamp, 2_000);
}

#[test]
fn window_is_scoped_to_a_single_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token_a);
    register(&client, &env, &issuer, &ns, &token_b);

    assert!(client.try_set_report_window(&issuer, &ns, &token_a, &1_000, &2_000).is_ok());

    assert!(client.get_report_window(&issuer, &ns, &token_a).is_some());
    assert!(
        client.get_report_window(&issuer, &ns, &token_b).is_none(),
        "setting a window on one offering must not leak to a sibling offering"
    );
}

#[test]
fn report_window_does_not_create_a_claim_window() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);

    assert!(client.try_set_report_window(&issuer, &ns, &token, &1_000, &2_000).is_ok());

    assert!(client.get_report_window(&issuer, &ns, &token).is_some());
    assert!(
        client.get_claim_window(&issuer, &ns, &token).is_none(),
        "report and claim windows are independent slots"
    );
}

#[test]
fn accepted_call_emits_an_event_but_rejected_call_does_not() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns");
    register(&client, &env, &issuer, &ns, &token);

    let before = env.events().all().len();
    assert!(client.try_set_report_window(&issuer, &ns, &token, &1_000, &2_000).is_ok());
    assert!(env.events().all().len() > before, "accepted set_report_window must emit an event");

    let after_accept = env.events().all().len();
    let r = client.try_set_report_window(&issuer, &ns, &token, &3_000, &2_500);
    assert_eq!(r, Err(Ok(RevoraError::LimitReached)));
    assert_eq!(
        env.events().all().len(),
        after_accept,
        "rejected set_report_window must not emit an event"
    );
}
