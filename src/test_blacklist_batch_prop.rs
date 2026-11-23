#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use proptest::prelude::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, TryIntoVal, Vec,
};

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn setup_offering<'a>(
    env: &'a Env,
    issuer: &Address,
    token: &Address,
    ns: &soroban_sdk::Symbol,
) -> RevoraRevenueShareClient<'a> {
    let client = make_client(env);
    client.initialize(issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        issuer,
        &Vec::new(&env),
        &1u32,
        ns,
        token,
        &1000u32,
        token,
        &0_i128,
        &symbol_short!(""),
        &0u32,
    );
    client
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

fn collect_blacklist_events(env: &Env, start: u32, end: u32) -> std::vec::Vec<NormalizedBlEvent> {
    let all_events = env.events().all();
    let mut out = std::vec::Vec::new();

    for i in start..end {
        let (_, topics, data) = all_events.get(i).unwrap();

        if topics.is_empty() {
            continue;
        }
        let sym: soroban_sdk::Symbol = topics.get(0).unwrap().into_val(env);
        let data: Vec<soroban_sdk::Val> = data.into_val(env);

        if sym == symbol_short!("bl_add") {
            let caller: Address = data.get(0).unwrap().try_into_val(env).unwrap();
            let investor: Address = data.get(1).unwrap().try_into_val(env).unwrap();
            out.push(NormalizedBlEvent { kind: BlEventKind::Add, caller, investor });
        } else if sym == symbol_short!("bl_rem") {
            let caller: Address = data.get(0).unwrap().try_into_val(env).unwrap();
            let investor: Address = data.get(1).unwrap().try_into_val(env).unwrap();
            out.push(NormalizedBlEvent { kind: BlEventKind::Rem, caller, investor });
        }
    }

    out
}

fn shuffle<T>(items: &mut [T], mut seed: u64) {
    for i in (1..items.len()).rev() {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        items.swap(i, (seed % (i as u64 + 1)) as usize);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        ..ProptestConfig::default()
    })]

    #[test]
    fn blacklist_batch_add_remove_order_independent(
        addrs in proptest::collection::vec(any::<u8>(), 0..48),
        rem_addrs in proptest::collection::vec(any::<u8>(), 0..48),
        shuffle_seed in any::<u64>(),
    ) {
        let mut addrs = addrs;
        let mut rem_addrs = rem_addrs;
        if let Some(&first) = addrs.first() {
            addrs.push(first);
            rem_addrs.push(first);
        }
        if let Some(&first) = rem_addrs.first() {
            rem_addrs.push(first);
        }

        let env = Env::default();
        env.mock_all_auths();

        let issuer = Address::generate(&env);
        let token = Address::generate(&env);
        let ns = symbol_short!("def");

        // Prebuild a stable pool of 256 addresses.
        let mut pool: std::vec::Vec<Address> = std::vec::Vec::with_capacity(256);

        for _ in 0u16..256u16 {
            pool.push(Address::generate(&env));
        }
        let to_addr_vec = |bytes: &std::vec::Vec<u8>| -> Vec<Address> {
            let mut out: Vec<Address> = Vec::new(&env);
            for &b in bytes.iter() {
                out.push_back(pool[b as usize].clone());
            }
            out
        };

        let add_vec_a: Vec<Address> = to_addr_vec(&addrs);
        let rem_vec_a: Vec<Address> = to_addr_vec(&rem_addrs);

        // Each execution gets an independent contract instance in the same environment.
        let client_a = setup_offering(&env, &issuer, &token, &ns);
        let add_events_start_a = env.events().all().len();
        client_a.blacklist_add_many(&issuer, &issuer, &ns, &token, &add_vec_a);
        let add_events_end_a = env.events().all().len();
        let add_events_a = collect_blacklist_events(&env, add_events_start_a, add_events_end_a);
        let rem_events_start_a = env.events().all().len();
        client_a.blacklist_remove_many(&issuer, &issuer, &ns, &token, &rem_vec_a);
        let rem_events_end_a = env.events().all().len();
        let rem_events_a = collect_blacklist_events(&env, rem_events_start_a, rem_events_end_a);
        let final_state_a = client_a.get_blacklist(&issuer, &ns, &token);

        // Execution B: shuffled order, but identical multiset of input addresses.
        let add_vec_b: Vec<Address> = add_vec_a.clone();
        let rem_vec_b: Vec<Address> = rem_vec_a.clone();

        // Convert Soroban Vec to std for shuffling, then back.
        let mut add_std: std::vec::Vec<Address> = Vec::into_iter(add_vec_b).collect();
        let mut rem_std: std::vec::Vec<Address> = Vec::into_iter(rem_vec_b).collect();

        shuffle(&mut add_std, shuffle_seed);
        shuffle(&mut rem_std, shuffle_seed);

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

        let client_b = setup_offering(&env, &issuer, &token, &ns);
        let add_events_start_b = env.events().all().len();
        client_b.blacklist_add_many(&issuer, &issuer, &ns, &token, &add_vec_b2);
        let add_events_end_b = env.events().all().len();
        let add_events_b = collect_blacklist_events(&env, add_events_start_b, add_events_end_b);
        let rem_events_start_b = env.events().all().len();
        client_b.blacklist_remove_many(&issuer, &issuer, &ns, &token, &rem_vec_b2);
        let rem_events_end_b = env.events().all().len();
        let rem_events_b = collect_blacklist_events(&env, rem_events_start_b, rem_events_end_b);
        let final_state_b = client_b.get_blacklist(&issuer, &ns, &token);

        // `get_blacklist` preserves insertion order; compare its deduplicated set.
        let mut a_state_std: std::vec::Vec<Address> = Vec::into_iter(final_state_a).collect();
        let mut b_state_std: std::vec::Vec<Address> = Vec::into_iter(final_state_b).collect();
        a_state_std.sort();
        a_state_std.dedup();
        b_state_std.sort();
        b_state_std.dedup();
        prop_assert_eq!(a_state_std, b_state_std);

        if add_vec_a.is_empty() {
            prop_assert!(!add_events_a.iter().any(|event| event.kind == BlEventKind::Add));
            prop_assert!(!add_events_b.iter().any(|event| event.kind == BlEventKind::Add));
        }
        if rem_vec_a.is_empty() {
            prop_assert!(!rem_events_a.iter().any(|event| event.kind == BlEventKind::Rem));
            prop_assert!(!rem_events_b.iter().any(|event| event.kind == BlEventKind::Rem));
        }

        // Compare batch events as a multiset; duplicates remain significant.
        let mut a_events_std = add_events_a;
        a_events_std.extend(rem_events_a);
        let mut b_events_std = add_events_b;
        b_events_std.extend(rem_events_b);
        a_events_std.sort();
        b_events_std.sort();
        prop_assert_eq!(a_events_std, b_events_std);
    }
}
