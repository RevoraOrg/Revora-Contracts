#![cfg(test)]
//! # Issue #1309 — `initialize` must be authorized and must not silently no-op.
//!
//! Before the fix `initialize` never called `admin.require_auth()` and returned
//! `()` when the admin was already set, so a front-runner could install their own
//! admin and the deployer's later call "succeeded" as a no-op. These tests pin:
//! - `initialize` without the admin's signature fails and writes nothing;
//! - a second `initialize` returns `AlreadyInitialized` and keeps the original admin;
//! - `set_admin` after `initialize` (and `initialize` after `set_admin`) fails, so
//!   there is a single initialization slot.

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{testutils::{Address as _, MockAuth, MockAuthInvoke}, Address, Env, IntoVal};

fn make_client(env: &Env) -> (RevoraRevenueShareClient<'static>, Address) {
    let id = env.register_contract(None, RevoraRevenueShare);
    (RevoraRevenueShareClient::new(env, &id), id)
}

#[test]
fn initialize_without_any_auth_fails() {
    let env = Env::default();
    let (client, _id) = make_client(&env);
    let admin = Address::generate(&env);

    let result = client.try_initialize(&admin, &None::<Address>, &None::<bool>);
    assert!(result.is_err(), "initialize must require the admin's authorization");
    assert_eq!(client.get_admin(), None);
}

#[test]
fn initialize_signed_by_a_different_address_fails() {
    let env = Env::default();
    let (client, id) = make_client(&env);
    let victim_admin = Address::generate(&env);
    let attacker = Address::generate(&env);

    // The attacker signs the exact `initialize(victim_admin, None, None)` invocation,
    // but only `victim_admin`'s signature may authorize installing `victim_admin`.
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &id,
            fn_name: "initialize",
            args: (victim_admin.clone(), None::<Address>, None::<bool>).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_initialize(&victim_admin, &None::<Address>, &None::<bool>);
    assert!(result.is_err(), "a signature from another address must not authorize initialize");
    assert_eq!(client.get_admin(), None);
}

#[test]
fn initialize_signed_by_the_admin_succeeds() {
    let env = Env::default();
    let (client, id) = make_client(&env);
    let admin = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &id,
            fn_name: "initialize",
            args: (admin.clone(), None::<Address>, None::<bool>).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    assert_eq!(client.get_admin(), Some(admin));
}

#[test]
fn second_initialize_errors_and_keeps_original_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _id) = make_client(&env);
    let admin = Address::generate(&env);
    let safety = Address::generate(&env);
    client.initialize(&admin, &Some(safety), &None::<bool>);

    let intruder = Address::generate(&env);
    let result = client.try_initialize(&intruder, &None::<Address>, &Some(true));
    assert_eq!(result, Err(Ok(RevoraError::AlreadyInitialized)));

    // Original configuration is untouched.
    assert_eq!(client.get_admin(), Some(admin.clone()));
    assert!(!client.is_event_only());

    // Re-initializing with the same admin is rejected too (no silent no-op).
    let result = client.try_initialize(&admin, &None::<Address>, &None::<bool>);
    assert_eq!(result, Err(Ok(RevoraError::AlreadyInitialized)));
    assert_eq!(client.get_admin(), Some(admin));
}

#[test]
fn set_admin_after_initialize_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _id) = make_client(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let intruder = Address::generate(&env);
    assert_eq!(client.try_set_admin(&intruder), Err(Ok(RevoraError::LimitReached)));
    assert_eq!(client.get_admin(), Some(admin));
}

#[test]
fn initialize_after_set_admin_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _id) = make_client(&env);
    let admin = Address::generate(&env);
    client.set_admin(&admin);

    let intruder = Address::generate(&env);
    let result = client.try_initialize(&intruder, &None::<Address>, &None::<bool>);
    assert_eq!(result, Err(Ok(RevoraError::AlreadyInitialized)));
    assert_eq!(client.get_admin(), Some(admin));
}
