#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::{Address as _, Events}, Address, Env, Vec};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Gas bounding test for `blacklist_add_many` in a worst-case mixed batch.
#[test]
fn blacklist_add_many_gas_bound_worst_case() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    // Initialize and register offering
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &ns, &token, &1000u32, &token, &0_i128);

    // Prefill blacklist to half the default cap (MAX_BLACKLIST_SIZE = 200 -> 100)
    let mut prefilled: Vec<Address> = Vec::new(&env);
    for _ in 0..25u32 {
        let a = Address::generate(&env);
        prefilled.push_back(a.clone());
        client.blacklist_add(&issuer, &issuer, &ns, &token, &a);
    }
    // Add more to reach half the cap (100 total)
    for _ in 0..75u32 {
        let a = Address::generate(&env);
        client.blacklist_add(&issuer, &issuer, &ns, &token, &a);
    }

    // Build a full batch (MAX_BATCH_SIZE = 50) with half already blacklisted
    let mut batch: Vec<Address> = Vec::new(&env);
    // Reuse first 25 prefilled entries
    for i in 0..25 {
        let addr = prefilled.get(i).unwrap();
        batch.push_back(addr.clone());
    }
    // Add 25 new addresses
    for _ in 0..25u32 {
        let a = Address::generate(&env);
        batch.push_back(a);
    }

    // Snapshot events and budget
    let events_before = env.events().all().len();
    let cpu_before = env.budget().cpu_instruction_cost();

    // Execute batch add
    client.blacklist_add_many(&issuer, &issuer, &ns, &token, &batch);

    let events_after = env.events().all().len();
    let cpu_after = env.budget().cpu_instruction_cost();

    // Only the 25 new addresses should have emitted bl_add events
    let new_events = events_after - events_before;
    assert_eq!(new_events, 25);

    // Assert CPU instruction delta is under a conservative ceiling
    // (keeps worst-case bounded; adjust if upstream budget changes)
    let consumed = cpu_after - cpu_before;
    let ceiling: u64 = 5_000_000;
    assert!(consumed <= ceiling, "Consumed CPU {} exceeded ceiling {}", consumed, ceiling);
}
