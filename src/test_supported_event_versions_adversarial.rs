//! Adversarial coverage for [`RevoraRevenueShare::supported_event_versions`]
//! and the admin setter that drives it,
//! [`RevoraRevenueShare::set_emit_v2_compat`].
//!
//! `supported_event_versions` is a read-only capability query used by indexers
//! to negotiate which topic schemas they may subscribe to.  These tests pin the
//! advertised contract:
//!
//! * the canonical V3 topic (`ev_idx3`, version 3) is always advertised;
//! * the V2 compatibility topic (`ev_idx2`, version 2) is advertised only while
//!   the V2 compat shim is enabled — and it defaults to enabled;
//! * the `ev_idx2` entry disappears when an admin disables the shim and comes
//!   back when it is re-enabled;
//! * toggling twice with the same value is idempotent;
//! * a non-admin caller is rejected with `NotAuthorized` and the advertised list
//!   is unchanged;
//! * the setter is rejected with `NotInitialized` before initialization;
//! * repeated queries are read-only and do not change the advertised list.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    (env, client, admin)
}

#[test]
fn advertises_canonical_v3_and_v2_compat_by_default() {
    let (_env, client, _admin) = setup();

    let list = client.supported_event_versions();
    assert_eq!(list.len(), 2);

    // V3 is canonical and first; V2 follows while the compat shim defaults on.
    assert_eq!(list.get(0).unwrap().topic, symbol_short!("ev_idx3"));
    assert_eq!(list.get(0).unwrap().version, 3u32);
    assert_eq!(list.get(1).unwrap().topic, symbol_short!("ev_idx2"));
    assert_eq!(list.get(1).unwrap().version, 2u32);
}

#[test]
fn disabling_v2_compat_keeps_only_the_canonical_v3_topic() {
    let (_env, client, admin) = setup();

    client.set_emit_v2_compat(&admin, &false);

    let list = client.supported_event_versions();
    assert_eq!(list.len(), 1);
    assert_eq!(list.get(0).unwrap().topic, symbol_short!("ev_idx3"));
    assert_eq!(list.get(0).unwrap().version, 3u32);
}

#[test]
fn re_enabling_v2_compat_restores_the_v2_topic() {
    let (_env, client, admin) = setup();

    client.set_emit_v2_compat(&admin, &false);
    assert_eq!(client.supported_event_versions().len(), 1);

    client.set_emit_v2_compat(&admin, &true);
    let list = client.supported_event_versions();
    assert_eq!(list.len(), 2);
    assert_eq!(list.get(1).unwrap().topic, symbol_short!("ev_idx2"));
    assert_eq!(list.get(1).unwrap().version, 2u32);
}

#[test]
fn toggling_the_same_value_twice_is_idempotent() {
    let (_env, client, admin) = setup();

    client.set_emit_v2_compat(&admin, &false);
    client.set_emit_v2_compat(&admin, &false);
    assert_eq!(client.supported_event_versions().len(), 1);

    client.set_emit_v2_compat(&admin, &true);
    client.set_emit_v2_compat(&admin, &true);
    assert_eq!(client.supported_event_versions().len(), 2);
}

#[test]
fn non_admin_caller_is_rejected_and_the_advertised_list_is_unchanged() {
    let (env, client, _admin) = setup();
    let attacker = Address::generate(&env);

    let result = client.try_set_emit_v2_compat(&attacker, &false);
    assert!(matches!(result.err(), Some(Ok(RevoraError::NotAuthorized))));

    // Still the default two-entry list: the rejected write was a no-op.
    assert_eq!(client.supported_event_versions().len(), 2);
}

#[test]
fn setter_is_rejected_before_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let caller = Address::generate(&env);

    let result = client.try_set_emit_v2_compat(&caller, &false);
    assert!(matches!(result.err(), Some(Ok(RevoraError::NotInitialized))));

    // The read-only query keeps advertising the default capabilities.
    assert_eq!(client.supported_event_versions().len(), 2);
}

#[test]
fn repeated_queries_are_read_only() {
    let (_env, client, admin) = setup();

    client.set_emit_v2_compat(&admin, &false);

    let first = client.supported_event_versions();
    let second = client.supported_event_versions();

    assert_eq!(first.len(), second.len());
    assert_eq!(first.len(), 1);

    // The queries must not have flipped the shim back on.
    assert_eq!(client.supported_event_versions().len(), 1);
}
