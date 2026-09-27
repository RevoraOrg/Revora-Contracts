//! Adversarial coverage for `storage_layout_version` / `set_storage_layout_version` (#1052).
//!
//! `src/lib.rs` exposes the on-chain storage layout stamp through a read-only
//! accessor (`storage_layout_version`) and an admin-only setter
//! (`set_storage_layout_version`).  The accessor feeds
//! `assert_storage_layout_compatible`, which is the guard that prevents a newer
//! binary from silently operating on an older layout — or a downgraded binary
//! from corrupting a newer one — so both the happy path and the rejection paths
//! are pinned here.
//!
//! Coverage matrix
//!
//! | Scenario                                             | Expected                                              |
//! |------------------------------------------------------|-------------------------------------------------------|
//! | Accessor before `initialize`                          | `None` (no stamp exists yet)                          |
//! | Setter before `initialize`                            | `NotInitialized`, no stamp written                    |
//! | Accessor right after `initialize`                     | `Some(STORAGE_LAYOUT_VERSION)`                        |
//! | Setter with a non-admin caller                        | `NotAuthorized`, stamp unchanged, no event            |
//! | Setter with the admin caller at `0`                   | `Some(0)` (lower boundary)                            |
//! | Setter with the admin caller at `u32::MAX`            | `Some(u32::MAX)` (upper boundary)                     |
//! | Accepted write                                        | exactly one `layout_v` event carrying the new value    |
//! | Rejected write                                        | no `layout_v` event emitted                           |

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, STORAGE_LAYOUT_VERSION};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events},
    Address, Env, IntoVal, Symbol,
};

/// Event topic emitted by every accepted layout-version write
/// (`EVENT_LAYOUT_VERSION` in `src/lib.rs`).
fn layout_event_topic() -> Symbol {
    symbol_short!("layout_v")
}

fn fresh_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &contract_id)
}

/// Count the `layout_v` events currently recorded on the ledger.
fn count_layout_events(env: &Env) -> u32 {
    let mut count = 0u32;
    for event in env.events().all().iter() {
        let topic0: Symbol = event.1.get(0).unwrap().into_val(env);
        if topic0 == layout_event_topic() {
            count += 1;
        }
    }
    count
}

/// Read the payload of the most recent `layout_v` event.
fn last_layout_event_value(env: &Env) -> u32 {
    let events = env.events().all();
    let mut found: Option<u32> = None;
    for event in events.iter() {
        let topic0: Symbol = event.1.get(0).unwrap().into_val(env);
        if topic0 == layout_event_topic() {
            found = Some(event.2.clone().into_val(env));
        }
    }
    found.expect("a layout_v event must have been emitted")
}

#[test]
fn accessor_is_none_before_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let client = fresh_client(&env);

    assert_eq!(
        client.storage_layout_version(),
        None,
        "an uninitialized contract must not report a layout stamp"
    );
}

#[test]
fn setter_before_initialize_is_rejected_and_writes_nothing() {
    let env = Env::default();
    env.mock_all_auths();
    let client = fresh_client(&env);
    let caller = Address::generate(&env);
    let events_before = env.events().all().len();

    assert_eq!(
        client.try_set_storage_layout_version(&caller, &7u32),
        Err(Ok(RevoraError::NotInitialized)),
        "the setter must require an initialized contract"
    );

    // Rejected operations must leave the observable state untouched.
    assert_eq!(client.storage_layout_version(), None);
    assert_eq!(env.events().all().len(), events_before, "a rejected write must not emit any event");
}

#[test]
fn initialize_stamps_the_compiled_layout_version() {
    let env = Env::default();
    env.mock_all_auths();
    let client = fresh_client(&env);
    let admin = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);

    assert_eq!(client.storage_layout_version(), Some(STORAGE_LAYOUT_VERSION));
}

#[test]
fn non_admin_setter_is_rejected_and_stamp_is_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let client = fresh_client(&env);
    let admin = Address::generate(&env);
    let stranger = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let events_before = env.events().all().len();

    assert_eq!(
        client.try_set_storage_layout_version(&stranger, &(STORAGE_LAYOUT_VERSION + 1)),
        Err(Ok(RevoraError::NotAuthorized)),
        "only the stored admin may rewrite the layout stamp"
    );

    assert_eq!(
        client.storage_layout_version(),
        Some(STORAGE_LAYOUT_VERSION),
        "a rejected write must not move the stamp"
    );
    assert_eq!(
        env.events().all().len(),
        events_before,
        "a rejected write must not emit a layout_v event"
    );
}

#[test]
fn admin_setter_round_trips_the_lower_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let client = fresh_client(&env);
    let admin = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.set_storage_layout_version(&admin, &0u32).unwrap();

    assert_eq!(client.storage_layout_version(), Some(0));
}

#[test]
fn admin_setter_round_trips_the_upper_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let client = fresh_client(&env);
    let admin = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.set_storage_layout_version(&admin, &u32::MAX).unwrap();

    assert_eq!(client.storage_layout_version(), Some(u32::MAX));
}

#[test]
fn successive_writes_keep_the_latest_value_and_emit_one_event_each() {
    let env = Env::default();
    env.mock_all_auths();
    let client = fresh_client(&env);
    let admin = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let events_after_init = count_layout_events(&env);

    client.set_storage_layout_version(&admin, &3u32).unwrap();
    client.set_storage_layout_version(&admin, &9u32).unwrap();

    assert_eq!(client.storage_layout_version(), Some(9), "the last accepted write wins");
    assert_eq!(
        count_layout_events(&env),
        events_after_init + 2,
        "each accepted write emits exactly one layout_v event"
    );
    assert_eq!(
        last_layout_event_value(&env),
        9u32,
        "the event payload must carry the value that was written"
    );
}
