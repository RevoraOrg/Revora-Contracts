//! Adversarial / boundary coverage for `RevoraRevenueShare::get_version` (#1163).
//!
//! `get_version` is a pure, unauthenticated view that returns the *compiled*
//! semver triple. The happy path already lives in `test_storage_layout_version.rs`;
//! this module attacks the surrounding semantics instead:
//!
//! - the tuple is genuinely `(MAJOR, MINOR, PATCH)` and mirrors `CONTRACT_VERSION`,
//! - repeated calls (and separate deployments) are deterministic,
//! - the view is side-effect free, needs no authorization and is callable before
//!   `initialize`,
//! - the compiled version does **not** drift when `migrate_storage` moves the
//!   persisted `DeployedVersion`.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, CONTRACT_VERSION};
use soroban_sdk::{testutils::Address as _, testutils::Events as _, Address, Env};

fn new_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &contract_id)
}

/// The returned triple must equal `CONTRACT_VERSION` component by component, with
/// the documented `(major, minor, patch)` ordering, and a released contract must
/// not advertise a zero major version.
#[test]
fn get_version_matches_compiled_constant_components() {
    let env = Env::default();
    let client = new_client(&env);

    let v = client.get_version();

    assert_eq!(v, CONTRACT_VERSION, "get_version must mirror CONTRACT_VERSION");
    assert_eq!(v.0, CONTRACT_VERSION.0, "component 0 must be MAJOR");
    assert_eq!(v.1, CONTRACT_VERSION.1, "component 1 must be MINOR");
    assert_eq!(v.2, CONTRACT_VERSION.2, "component 2 must be PATCH");
    assert!(v.0 >= 1, "a released contract must not report MAJOR == 0");
}

/// Determinism: many repeated calls, and two independently deployed instances,
/// must all agree on the same triple.
#[test]
fn get_version_is_deterministic_across_calls_and_instances() {
    let env = Env::default();
    let a = new_client(&env);
    let b = new_client(&env);

    let expected = a.get_version();
    for _ in 0..16 {
        assert_eq!(a.get_version(), expected, "repeated calls must not drift");
    }
    assert_eq!(b.get_version(), expected, "separate deployments must agree");
}

/// Read-only view: no authorization is required and no events are emitted.
#[test]
fn get_version_is_side_effect_free_and_unauthenticated() {
    let env = Env::default();
    let client = new_client(&env);

    // No `initialize`, no `mock_all_auths`, and explicitly empty auth entries:
    // a pure view must still succeed.
    env.set_auths(&[]);
    let before = env.events().all().len();
    let v = client.get_version();
    assert_eq!(v, CONTRACT_VERSION);
    assert_eq!(env.events().all().len(), before, "get_version must not emit events");
}

/// Callable before initialization and unaffected by a later `initialize`.
#[test]
fn get_version_available_before_and_after_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let client = new_client(&env);

    let pre = client.get_version();
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    assert_eq!(client.get_version(), pre, "initialize must not change get_version");
    assert_eq!(pre, CONTRACT_VERSION);
}

/// `get_version` reports the *compiled* version, not the persisted
/// `DeployedVersion`: advancing storage through `migrate_storage` must not move it.
#[test]
fn get_version_tracks_compiled_not_persisted_version() {
    let env = Env::default();
    env.mock_all_auths();
    let client = new_client(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let compiled = client.get_version();
    let next_major = compiled.0.checked_add(1).unwrap();

    // Move the persisted version strictly forward (major bump).
    client.migrate_storage(&admin, &next_major, &0, &0);
    assert_eq!(
        client.get_version(),
        compiled,
        "get_version must not follow the persisted DeployedVersion"
    );

    // Re-targeting the same version is rejected, and the view stays put.
    let res = client.try_migrate_storage(&admin, &next_major, &0, &0);
    assert_eq!(res, Err(Ok(RevoraError::AlreadyAtTargetVersion)));
    assert_eq!(client.get_version(), compiled);
}
