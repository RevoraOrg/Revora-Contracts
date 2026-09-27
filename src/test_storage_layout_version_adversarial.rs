//! Adversarial coverage for [`RevoraRevenueShare::set_storage_layout_version`].
//!
//! The setter is the admin-only migration hook that rewrites the on-chain
//! storage layout stamp and emits `layout_v`. These tests pin the write-side
//! contract beyond the happy path:
//!
//! * an uninitialized contract is rejected with `NotInitialized`;
//! * only the stored admin may write — any other caller gets `NotAuthorized`;
//! * a rejected call leaves the previously stored stamp untouched and emits no
//!   new layout event;
//! * the full `u32` range (`0` and `u32::MAX`) round-trips;
//! * re-stamping the same value is idempotent and emits per accepted write.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol,
};

fn layout_topic() -> Symbol {
    symbol_short!("layout_v")
}

/// Count the `layout_v` events recorded so far.
fn count_layout_events(env: &Env) -> u32 {
    let all = env.events().all();
    let mut count = 0u32;
    for i in 0..all.len() {
        let (_, topics, _) = all.get(i).unwrap();
        if !topics.is_empty() {
            let t0: Symbol = topics.get(0).unwrap().into_val(env);
            if t0 == layout_topic() {
                count += 1;
            }
        }
    }
    count
}

/// Value carried by the most recent `layout_v` event.
fn last_layout_event_value(env: &Env) -> Option<u32> {
    let all = env.events().all();
    let mut value: Option<u32> = None;
    for i in 0..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        if !topics.is_empty() {
            let t0: Symbol = topics.get(0).unwrap().into_val(env);
            if t0 == layout_topic() {
                value = Some(data.into_val(env));
            }
        }
    }
    value
}

/// Initialized contract with the acting admin.
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
fn admin_write_persists_value_and_emits_layout_event() {
    let (env, client, admin) = setup();
    let baseline = count_layout_events(&env);

    client.set_storage_layout_version(&admin, &7).unwrap();

    assert_eq!(client.storage_layout_version(), Some(7));
    assert_eq!(count_layout_events(&env), baseline + 1);
    assert_eq!(last_layout_event_value(&env), Some(7));
}

#[test]
fn rejects_uninitialized_contract_without_writing_a_stamp() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let caller = Address::generate(&env);

    let result = client.try_set_storage_layout_version(&caller, &3);

    assert_eq!(result, Err(Ok(RevoraError::NotInitialized)));
    assert_eq!(client.storage_layout_version(), None);
}

#[test]
fn rejects_non_admin_caller_and_preserves_previous_stamp() {
    let (env, client, admin) = setup();

    client.set_storage_layout_version(&admin, &5).unwrap();
    let events_before = count_layout_events(&env);

    let intruder = Address::generate(&env);
    let result = client.try_set_storage_layout_version(&intruder, &9);

    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));
    // Rejected write must not move the stamp nor emit a layout event.
    assert_eq!(client.storage_layout_version(), Some(5));
    assert_eq!(count_layout_events(&env), events_before);
}

#[test]
fn accepts_the_full_u32_boundary_range() {
    let (_env, client, admin) = setup();

    client.set_storage_layout_version(&admin, &0).unwrap();
    assert_eq!(client.storage_layout_version(), Some(0));

    client.set_storage_layout_version(&admin, &u32::MAX).unwrap();
    assert_eq!(client.storage_layout_version(), Some(u32::MAX));
    assert_eq!(client.storage_layout_version(), Some(4_294_967_295));
}

#[test]
fn re_stamping_the_same_value_is_idempotent_and_emits_per_write() {
    let (env, client, admin) = setup();
    let baseline = count_layout_events(&env);

    client.set_storage_layout_version(&admin, &3).unwrap();
    client.set_storage_layout_version(&admin, &3).unwrap();

    assert_eq!(client.storage_layout_version(), Some(3));
    assert_eq!(count_layout_events(&env), baseline + 2);
}

#[test]
fn rejects_unknown_caller_after_an_admin_write_without_side_effects() {
    let (env, client, admin) = setup();

    // Two distinct rejected callers in a row must both be inert.
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let baseline = count_layout_events(&env);

    assert_eq!(client.try_set_storage_layout_version(&a, &11), Err(Ok(RevoraError::NotAuthorized)));
    assert_eq!(client.try_set_storage_layout_version(&b, &22), Err(Ok(RevoraError::NotAuthorized)));

    assert_eq!(client.storage_layout_version(), None);
    assert_eq!(count_layout_events(&env), baseline);
    // Admin can still perform the write afterwards.
    client.set_storage_layout_version(&admin, &1).unwrap();
    assert_eq!(client.storage_layout_version(), Some(1));
}
