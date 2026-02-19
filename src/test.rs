#![cfg(test)]
use soroban_sdk::{testutils::Address as _, testutils::Events as _, Address, Env};

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};

const BOUNDARY_AMOUNTS: [i128; 7] = [
    i128::MIN,
    i128::MIN + 1,
    -1,
    0,
    1,
    i128::MAX - 1,
    i128::MAX,
];
const BOUNDARY_PERIODS: [u64; 6] = [0, 1, 2, 10_000, u64::MAX - 1, u64::MAX];
const FUZZ_ITERATIONS: usize = 512;

fn next_u64(seed: &mut u64) -> u64 {
    // Deterministic LCG for repeatable pseudo-random test values.
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed
}

fn next_amount(seed: &mut u64) -> i128 {
    let hi = next_u64(seed) as u128;
    let lo = next_u64(seed) as u128;
    ((hi << 64) | lo) as i128
}

fn next_period(seed: &mut u64) -> u64 {
    next_u64(seed)
}

#[test]
fn it_emits_events_on_register_and_report() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &1_000); // 10% in bps
    client.report_revenue(&issuer, &token, &1_000_000, &1);

    // In a real test, inspect events / state here.
    assert!(env.events().all().len() >= 2);
}

#[test]
fn fuzz_period_and_amount_boundaries_do_not_panic() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let mut calls = 0usize;
    for amount in BOUNDARY_AMOUNTS {
        for period in BOUNDARY_PERIODS {
            client.report_revenue(&issuer, &token, &amount, &period);
            calls += 1;
        }
    }

    assert_eq!(env.events().all().len(), calls as u32);
}

#[test]
fn fuzz_period_and_amount_repeatable_sweep_do_not_panic() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // Same seed must produce the exact same sequence.
    let mut seed_a = 0xA11C_E5ED_19u64;
    let mut seed_b = 0xA11C_E5ED_19u64;
    for _ in 0..64 {
        assert_eq!(next_amount(&mut seed_a), next_amount(&mut seed_b));
        assert_eq!(next_period(&mut seed_a), next_period(&mut seed_b));
    }

    // Reset and run deterministic fuzz-style inputs through contract entrypoint.
    let mut seed = 0xA11C_E5ED_19u64;
    for i in 0..FUZZ_ITERATIONS {
        let mut amount = next_amount(&mut seed);
        let mut period = next_period(&mut seed);

        // Periodically force hard boundaries into the sweep.
        if i % 64 == 0 {
            amount = i128::MAX;
        } else if i % 64 == 1 {
            amount = i128::MIN;
        }
        if i % 97 == 0 {
            period = u64::MAX;
        } else if i % 97 == 1 {
            period = 0;
        }

        client.report_revenue(&issuer, &token, &amount, &period);
    }

    assert_eq!(env.events().all().len(), FUZZ_ITERATIONS as u32);
}
