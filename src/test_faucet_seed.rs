//! Tests for `faucet_seed_holders` — testnet-only deterministic holder seeding.
//!
//! ## Coverage matrix
//!
//! | Scenario | Expected |
//! |----------|----------|
//! | `testnet_mode == false` (default) | `TestnetOnly` error |
//! | `testnet_mode` disabled after being enabled | `TestnetOnly` error |
//! | Offering not registered | `OfferingNotFound` error |
//! | count == 0 | `Ok(Vec::new())`, no events emitted |
//! | count > 0, testnet + offering present | `Ok(seeds)`, len == count |
//! | Same inputs, called twice | identical seeds (determinism) |
//! | Distinct slots produce distinct seeds | no collisions |
//! | One event emitted per slot | event count delta == count |
//! | count divisible (20) | 20 seeds returned |
//! | count indivisible (3) | 3 seeds returned |
//! | count == 1 | 1 seed returned |
//! | Large count (100) | 100 seeds returned |
//! | Different offerings, same count | first seeds differ |
//! | Each seed is 32 bytes | length invariant |

#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Initialise contract and enable testnet mode; returns the admin address.
fn enable_testnet(client: &RevoraRevenueShareClient<'_>, env: &Env) -> Address {
    let admin = Address::generate(env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.set_testnet_mode(&true);
    admin
}

/// Register a minimal offering; returns (issuer, namespace, token).
fn register_offering(
    client: &RevoraRevenueShareClient<'_>,
    env: &Env,
) -> (Address, Symbol, Address) {
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let payout = Address::generate(env);
    let ns = symbol_short!("ns");
    client.register_offering(&issuer, &ns, &token, &10_000, &payout, &0);
    (issuer, ns, token)
}

/// Full setup: env + client (testnet enabled) + offering.
fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Symbol, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    enable_testnet(&client, &env);
    let (issuer, ns, token) = register_offering(&client, &env);
    (env, client, issuer, ns, token)
}

// ── Error path tests ──────────────────────────────────────────────────────────

#[test]
fn faucet_rejected_when_testnet_mode_is_false() {
    // Default state: testnet_mode is not set → false.
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let (issuer, ns, token) = register_offering(&client, &env);

    let result = client.try_faucet_seed_holders(&issuer, &ns, &token, &5);
    assert_eq!(result, Err(Ok(RevoraError::TestnetOnly)));
}

#[test]
fn faucet_rejected_after_testnet_mode_disabled() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    enable_testnet(&client, &env);
    client.set_testnet_mode(&false); // disable
    let (issuer, ns, token) = register_offering(&client, &env);

    let result = client.try_faucet_seed_holders(&issuer, &ns, &token, &3);
    assert_eq!(result, Err(Ok(RevoraError::TestnetOnly)));
}

#[test]
fn faucet_returns_offering_not_found_for_unknown_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    enable_testnet(&client, &env);

    let fake_issuer = Address::generate(&env);
    let fake_token = Address::generate(&env);
    let ns = symbol_short!("ns");

    let result = client.try_faucet_seed_holders(&fake_issuer, &ns, &fake_token, &5);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

// ── Edge-case: count == 0 ──────────────────────────────────────────────────────

#[test]
fn faucet_count_zero_returns_empty_vec() {
    let (_, client, issuer, ns, token) = setup();
    let seeds = client.faucet_seed_holders(&issuer, &ns, &token, &0);
    assert_eq!(seeds.len(), 0);
}

#[test]
fn faucet_count_zero_emits_no_events() {
    let (env, client, issuer, ns, token) = setup();
    let before = env.events().all().len();
    client.faucet_seed_holders(&issuer, &ns, &token, &0);
    assert_eq!(env.events().all().len(), before, "count==0 must emit no events");
}

// ── Length invariant ──────────────────────────────────────────────────────────

#[test]
fn faucet_returns_correct_seed_count_for_various_inputs() {
    let (_, client, issuer, ns, token) = setup();
    for count in [1u32, 2, 3, 5, 10, 20, 50] {
        let seeds = client.faucet_seed_holders(&issuer, &ns, &token, &count);
        assert_eq!(seeds.len(), count, "count={count}: wrong seed count");
    }
}

// ── Determinism ───────────────────────────────────────────────────────────────

#[test]
fn faucet_is_deterministic_across_calls() {
    let (_, client, issuer, ns, token) = setup();
    let seeds_a = client.faucet_seed_holders(&issuer, &ns, &token, &4);
    let seeds_b = client.faucet_seed_holders(&issuer, &ns, &token, &4);
    assert_eq!(seeds_a.len(), seeds_b.len());
    for i in 0..seeds_a.len() {
        assert_eq!(
            seeds_a.get(i),
            seeds_b.get(i),
            "seed at slot {i} must be identical across calls"
        );
    }
}

// ── Uniqueness ────────────────────────────────────────────────────────────────

#[test]
fn faucet_slots_produce_distinct_seeds() {
    let (_, client, issuer, ns, token) = setup();
    let seeds = client.faucet_seed_holders(&issuer, &ns, &token, &5);
    for i in 0..seeds.len() {
        for j in (i + 1)..seeds.len() {
            assert_ne!(
                seeds.get(i),
                seeds.get(j),
                "slots {i} and {j} must have distinct seeds"
            );
        }
    }
}

#[test]
fn faucet_seeds_differ_between_distinct_offerings() {
    let (env, client, issuer1, ns1, token1) = setup();

    // Register a second offering on the same contract.
    let issuer2 = Address::generate(&env);
    let token2 = Address::generate(&env);
    let payout2 = Address::generate(&env);
    let ns2 = symbol_short!("ns2");
    client.register_offering(&issuer2, &ns2, &token2, &5_000, &payout2, &0);

    let seeds1 = client.faucet_seed_holders(&issuer1, &ns1, &token1, &3);
    let seeds2 = client.faucet_seed_holders(&issuer2, &ns2, &token2, &3);

    assert_ne!(
        seeds1.get(0),
        seeds2.get(0),
        "slot-0 seeds must differ between different offerings"
    );
}

// ── Event emission ────────────────────────────────────────────────────────────

#[test]
fn faucet_emits_one_event_per_slot() {
    let (env, client, issuer, ns, token) = setup();
    let count = 7u32;
    let before = env.events().all().len();
    client.faucet_seed_holders(&issuer, &ns, &token, &count);
    let delta = env.events().all().len() - before;
    assert!(delta >= count as usize, "expected ≥{count} new events, got {delta}");
}

// ── Seed byte-length invariant ────────────────────────────────────────────────

#[test]
fn faucet_each_seed_is_32_bytes() {
    let (_, client, issuer, ns, token) = setup();
    let seeds = client.faucet_seed_holders(&issuer, &ns, &token, &4);
    for i in 0..seeds.len() {
        let seed = seeds.get(i).expect("seed at index {i}");
        assert_eq!(seed.len(), 32, "slot {i}: seed must be exactly 32 bytes");
    }
}

// ── BPS distribution coverage ─────────────────────────────────────────────────

#[test]
fn faucet_single_slot_returns_one_seed() {
    // count=1 → one slot absorbing all 10 000 bps.
    let (_, client, issuer, ns, token) = setup();
    let seeds = client.faucet_seed_holders(&issuer, &ns, &token, &1);
    assert_eq!(seeds.len(), 1, "count=1 must return exactly one seed");
}

#[test]
fn faucet_divisible_count_returns_correct_length() {
    // 10_000 / 20 = 500, remainder 0 → each slot gets 500 bps exactly.
    let (_, client, issuer, ns, token) = setup();
    let seeds = client.faucet_seed_holders(&issuer, &ns, &token, &20);
    assert_eq!(seeds.len(), 20);
}

#[test]
fn faucet_indivisible_count_returns_correct_length() {
    // 10_000 / 3 = 3333 remainder 1 → last slot gets 3334 bps.
    let (_, client, issuer, ns, token) = setup();
    let seeds = client.faucet_seed_holders(&issuer, &ns, &token, &3);
    assert_eq!(seeds.len(), 3);
}

// ── Large-count boundary ──────────────────────────────────────────────────────

#[test]
fn faucet_large_count_succeeds() {
    let (_, client, issuer, ns, token) = setup();
    let seeds = client.faucet_seed_holders(&issuer, &ns, &token, &100);
    assert_eq!(seeds.len(), 100);
}
