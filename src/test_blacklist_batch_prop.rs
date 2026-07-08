#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use proptest::prelude::*;
use soroban_sdk::{
    symbol_short,
    Address, Env, Vec,
};


fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn setup_offering(env: &Env, issuer: &Address, token: &Address, ns: &soroban_sdk::Symbol) {
    let client = make_client(env);
    client.initialize(issuer, &None::<Address>, &None::<bool>);
    client.register_offering(issuer, ns, token, &1000u32, token, &0_i128);
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum BlEventKind {
    Add,
    Rem,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct NormalizedBlEvent {
    kind: BlEventKind,
    caller: Address,
    investor: Address,
}

fn collect_blacklist_events(env: &Env) -> Vec<NormalizedBlEvent> {
    // Event filtering: only consider bl_add/bl_rem.
    // We normalize away ordering and compare as a sorted multiset.
    let mut out: Vec<NormalizedBlEvent> = Vec::new(&env);

    // Soroban SDK testutils: env.events().all() gives all published events
    // as typed tuples we can match on.
    for i in 0..env.events().all().len() {
        // Each event is internally a tuple like: (topics..., data...)
        // We rely on the topic symbol being present in the first position.
        // Since this is a test harness and we only need deterministic comparisons,
        // we use the Events decoding to get (symbol, data) shape.
        let ev = env.events().all().get(i).unwrap();

        // ev is (topic tuple, data tuple) under testutils.
        // We only need the event symbol and (caller, investor).
        // The event topic published by the contract is:
        //   (EVENT_BL_ADD, issuer, namespace, token)
        //   (EVENT_BL_REM, issuer, namespace, token)
        // with data (caller, investor)
        let topics = ev.topics();
        if topics.is_empty() {
            continue;
        }
        // topics[0] is the event symbol.
        let sym: soroban_sdk::Symbol = topics.get(0).unwrap();

        if sym == symbol_short!("bl_add") {
            let data = ev.data();
            let caller: Address = data.get(0).unwrap();
            let investor: Address = data.get(1).unwrap();
            out.push_back(NormalizedBlEvent { kind: BlEventKind::Add, caller, investor });
        } else if sym == symbol_short!("bl_rem") {
            let data = ev.data();
            let caller: Address = data.get(0).unwrap();
            let investor: Address = data.get(1).unwrap();
            out.push_back(NormalizedBlEvent { kind: BlEventKind::Rem, caller, investor });
        }
    }

    out
}

fn sort_std_normalized(mut events: std::vec::Vec<NormalizedBlEvent>) -> std::vec::Vec<NormalizedBlEvent> {
    events.sort();
    events
}


proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        ..ProptestConfig::default()
    })]

    #[test]
    fn blacklist_batch_add_remove_order_independent(
        addrs in proptest::collection::vec(any::<u8>(), 0..50),
        rem_addrs in proptest::collection::vec(any::<u8>(), 0..50),
        shuffle_seed in any::<u64>(),
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let issuer = Address::generate(&env);
        let token = Address::generate(&env);
        let ns = symbol_short!("def");

        setup_offering(&env, &issuer, &token, &ns);
        let client = make_client(&env);

        // Prebuild a stable pool of 256 addresses.
        // Duplicates/overlaps are intentionally produced because u8 values are mapped by index.
        let mut pool: std::vec::Vec<Address> = std::vec::Vec::with_capacity(256);

        for i in 0u16..256u16 {
            pool.push(Address::generate(&env));
        }
        let mut to_addr_vec = |bytes: &std::vec::Vec<u8>| -> Vec<Address> {
            let mut out: Vec<Address> = Vec::new(&env);
            for &b in bytes.iter() {
                out.push_back(pool[b as usize].clone());
            }
            out
        };

        let add_vec_a: Vec<Address> = to_addr_vec(&addrs);
        let rem_vec_a: Vec<Address> = to_addr_vec(&rem_addrs);

        // Execution A: original order.
        let mut env_a = env.clone();
        env_a.mock_all_auths();
        setup_offering(&env_a, &issuer, &token, &ns);
        let client_a = make_client(&env_a);
        client_a.blacklist_add_many(&issuer, &issuer, &ns, &token, &add_vec_a);
        client_a.blacklist_remove_many(&issuer, &issuer, &ns, &token, &rem_vec_a);
        let final_state_a = client_a.get_blacklist(&issuer, &ns, &token);
        let events_a = collect_blacklist_events(&env_a);


        // Execution B: shuffled order, but identical multiset of input addresses.
        let mut rng = proptest::test_runner::TestRng::deterministic(shuffle_seed);
        let mut add_vec_b: Vec<Address> = add_vec_a.clone();
        let mut rem_vec_b: Vec<Address> = rem_vec_a.clone();

        // Convert Soroban Vec to std for shuffling, then back.
        let mut add_std: std::vec::Vec<Address> = Vec::into_iter(add_vec_b).collect();
        let mut rem_std: std::vec::Vec<Address> = Vec::into_iter(rem_vec_b).collect();

        add_std.shuffle(&mut rng);
        rem_std.shuffle(&mut rng);

        let add_vec_b2: Vec<Address> = {
            let mut out: Vec<Address> = Vec::new(&env);
            for a in add_std { out.push_back(a); }
            out
        };
        let rem_vec_b2: Vec<Address> = {
            let mut out: Vec<Address> = Vec::new(&env);
            for a in rem_std { out.push_back(a); }
            out
        };

        let mut env_b = env.clone();
        env_b.mock_all_auths();
        setup_offering(&env_b, &issuer, &token, &ns);
        let client_b = make_client(&env_b);
        client_b.blacklist_add_many(&issuer, &issuer, &ns, &token, &add_vec_b2);
        client_b.blacklist_remove_many(&issuer, &issuer, &ns, &token, &rem_vec_b2);
        let final_state_b = client_b.get_blacklist(&issuer, &ns, &token);
        let events_b = collect_blacklist_events(&env_b);

        // Assert final blacklist state identical.
        // Contract uses deterministic insertion-order, but because batch dedups based on first
        // occurrence, shuffling should still converge to same *set*; however order may differ.
        // The requirement says identical final state, so compare as set by sorting.
        let mut a_state_std: std::vec::Vec<Address> = Vec::into_iter(final_state_a).collect();
        let mut b_state_std: std::vec::Vec<Address> = Vec::into_iter(final_state_b).collect();
        a_state_std.sort();
        b_state_std.sort();
        prop_assert_eq!(a_state_std, b_state_std);

        // Assert events form an order-equivalent multiset.
        let mut a_events_std: std::vec::Vec<NormalizedBlEvent> = Vec::into_iter(events_a).collect();
        let mut b_events_std: std::vec::Vec<NormalizedBlEvent> = Vec::into_iter(events_b).collect();
        a_events_std.sort();
        b_events_std.sort();
        prop_assert_eq!(a_events_std, b_events_std);

        // Empty input no-op with no events.
        if addrs.is_empty() && rem_addrs.is_empty() {
            // After setup only, no blacklist ops were called with empty vectors (contract returns Ok()).
            // However other setup events exist; we only care bl_* events.
            prop_assert!(a_events_std.is_empty());
            prop_assert!(b_events_std.is_empty());
        }
    }
}

