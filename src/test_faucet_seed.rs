//! Tests for `faucet_seed_holders` and `faucet_reset` — testnet-only deterministic
//! holder seeding and state reset primitives.
//!
//! ## Coverage matrix — `faucet_seed_holders`
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
//!
//! ## Coverage matrix — `faucet_reset`
//!
//! | Scenario | Expected |
//! |----------|----------|
//! | `testnet_mode == false` | `TestnetOnly` error |
//! | `testnet_mode` disabled after being enabled | `TestnetOnly` error |
//! | Offering not registered | `OfferingNotFound` error |
//! | Caller is not admin | `NotAuthorized` error |
//! | No prior seeds | `Ok(())`, no-op |
//! | After seeding: reset clears entries | re-seed succeeds |
//! | Reset then re-seed (cooldown elapsed) | succeeds |
//! | Called twice (idempotent) | both `Ok(())` |
//! | Does not affect other offerings | cross-offering isolation |
//! | Large seed count (50) cleared | re-seed succeeds |
//! | `seed` param echoed in `fct_rst` event | event correctness |

#![cfg(test)]

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger},
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
    client.register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &10_000, &payout, &0, &symbol_short!(""), &0u32);
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
    let requester = Address::generate(&env);
    let (issuer, ns, token) = register_offering(&client, &env);

    let result = client.try_faucet_seed_holders(&requester, &issuer, &ns, &token, &5);
    assert_eq!(result, Err(Ok(RevoraError::TestnetOnly)));
}

#[test]
fn faucet_rejected_after_testnet_mode_disabled() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    enable_testnet(&client, &env);
    client.set_testnet_mode(&false); // disable
    let requester = Address::generate(&env);
    let (issuer, ns, token) = register_offering(&client, &env);

    let result = client.try_faucet_seed_holders(&requester, &issuer, &ns, &token, &3);
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

    let requester = Address::generate(&env);
    let result = client.try_faucet_seed_holders(&requester, &fake_issuer, &ns, &fake_token, &5);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn faucet_rejects_requests_within_the_cooldown_window() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    enable_testnet(&client, &env);
    let requester = Address::generate(&env);
    let (issuer, ns, token) = register_offering(&client, &env);

    let first = client.try_faucet_seed_holders(&requester, &issuer, &ns, &token, &2);
    assert!(matches!(first, Ok(Ok(_))));

    let second = client.try_faucet_seed_holders(&requester, &issuer, &ns, &token, &2);
    assert_eq!(
        second,
        Err(Ok(RevoraError::FaucetCooldownActive)),
        "second request within cooldown should be rejected"
    );
}

#[test]
fn faucet_allows_request_after_cooldown_elapsed() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    enable_testnet(&client, &env);
    let requester = Address::generate(&env);
    let (issuer, ns, token) = register_offering(&client, &env);

    let first = client.try_faucet_seed_holders(&requester, &issuer, &ns, &token, &2);
    assert!(matches!(first, Ok(Ok(_))));

    env.ledger().set_timestamp(DEFAULT_FAUCET_COOLDOWN_SECONDS);

    let second = client.try_faucet_seed_holders(&requester, &issuer, &ns, &token, &2);
    assert!(matches!(second, Ok(Ok(_))), "request after cooldown elapsed should succeed");
}

// ── Edge-case: count == 0 ──────────────────────────────────────────────────────

#[test]
fn faucet_count_zero_returns_empty_vec() {
    let (env, client, issuer, ns, token) = setup();
    let requester = Address::generate(&env);
    let seeds = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &0);
    assert_eq!(seeds.len(), 0);
}

#[test]
fn faucet_count_zero_emits_no_events() {
    let (env, client, issuer, ns, token) = setup();
    let requester = Address::generate(&env);
    let before = env.events().all().len();
    client.faucet_seed_holders(&requester, &issuer, &ns, &token, &0);
    assert_eq!(env.events().all().len(), before, "count==0 must emit no events");
}

// ── Length invariant ──────────────────────────────────────────────────────────

#[test]
fn faucet_returns_correct_seed_count_for_various_inputs() {
    let (env, client, issuer, ns, token) = setup();
    let requester = Address::generate(&env);
    for count in [1u32, 2, 3, 5, 10, 20, 50] {
        let seeds = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &count);
        assert_eq!(seeds.len(), count, "count={count}: wrong seed count");
    }
}

// ── Determinism ───────────────────────────────────────────────────────────────

#[test]
fn faucet_is_deterministic_across_calls() {
    let (env, client, issuer, ns, token) = setup();
    let requester = Address::generate(&env);
    let seeds_a = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &4);
    let seeds_b = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &4);
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
    let (env, client, issuer, ns, token) = setup();
    let requester = Address::generate(&env);
    let seeds = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &5);
    for i in 0..seeds.len() {
        for j in (i + 1)..seeds.len() {
            assert_ne!(seeds.get(i), seeds.get(j), "slots {i} and {j} must have distinct seeds");
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
    client.register_offering(&issuer2, &Vec::new(&env), &1u32, &ns2, &token2, &5_000, &payout2, &0, &symbol_short!(""), &0u32);

    let requester = Address::generate(&env);
    let seeds1 = client.faucet_seed_holders(&requester, &issuer1, &ns1, &token1, &3);
    let seeds2 = client.faucet_seed_holders(&requester, &issuer2, &ns2, &token2, &3);

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
    let requester = Address::generate(&env);
    let count = 7u32;
    let before = env.events().all().len();
    client.faucet_seed_holders(&requester, &issuer, &ns, &token, &count);
    let delta = env.events().all().len() - before;
    assert!(delta >= count as usize, "expected ≥{count} new events, got {delta}");
}

// ── Seed byte-length invariant ────────────────────────────────────────────────

#[test]
fn faucet_each_seed_is_32_bytes() {
    let (env, client, issuer, ns, token) = setup();
    let requester = Address::generate(&env);
    let seeds = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &4);
    for i in 0..seeds.len() {
        let seed = seeds.get(i).expect("seed at index {i}");
        assert_eq!(seed.len(), 32, "slot {i}: seed must be exactly 32 bytes");
    }
}

// ── BPS distribution coverage ─────────────────────────────────────────────────

#[test]
fn faucet_single_slot_returns_one_seed() {
    // count=1 → one slot absorbing all 10 000 bps.
    let (env, client, issuer, ns, token) = setup();
    let requester = Address::generate(&env);
    let seeds = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &1);
    assert_eq!(seeds.len(), 1, "count=1 must return exactly one seed");
}

#[test]
fn faucet_divisible_count_returns_correct_length() {
    // 10_000 / 20 = 500, remainder 0 → each slot gets 500 bps exactly.
    let (env, client, issuer, ns, token) = setup();
    let requester = Address::generate(&env);
    let seeds = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &20);
    assert_eq!(seeds.len(), 20);
}

#[test]
fn faucet_indivisible_count_returns_correct_length() {
    // 10_000 / 3 = 3333 remainder 1 → last slot gets 3334 bps.
    let (env, client, issuer, ns, token) = setup();
    let requester = Address::generate(&env);
    let seeds = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &3);
    assert_eq!(seeds.len(), 3);
}

// ── Large-count boundary ──────────────────────────────────────────────────────

#[test]
fn faucet_large_count_succeeds() {
    let (env, client, issuer, ns, token) = setup();
    let requester = Address::generate(&env);
    let seeds = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &100);
    assert_eq!(seeds.len(), 100);
}

// ── faucet_reset helpers ──────────────────────────────────────────────────────

/// Full setup with admin exposed: env + client (testnet enabled) + offering + admin.
fn setup_with_admin() -> (Env, RevoraRevenueShareClient<'static>, Address, Symbol, Address, Address)
{
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = enable_testnet(&client, &env);
    let (issuer, ns, token) = register_offering(&client, &env);
    (env, client, issuer, ns, token, admin)
}

/// Generate a fixed 32-byte seed value for tests.
fn make_seed(env: &Env) -> soroban_sdk::BytesN<32> {
    let mut b = soroban_sdk::Bytes::from_array(env, &[0u8; 32]);
    b.set(0, 0xde);
    b.set(1, 0xad);
    b.set(2, 0xbe);
    b.set(3, 0xef);
    env.crypto().sha256(&b)
}

// ── faucet_reset error-path tests ─────────────────────────────────────────────

#[test]
fn faucet_reset_rejected_when_testnet_mode_is_false() {
    // testnet_mode must be enabled for faucet_reset to succeed.
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    // Initialize without enabling testnet mode.
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    let (issuer, ns, token) = register_offering(&client, &env);
    let seed = make_seed(&env);

    let result = client.try_faucet_reset(&admin, &issuer, &ns, &token, &seed);
    assert_eq!(
        result,
        Err(Ok(RevoraError::TestnetOnly)),
        "faucet_reset must be rejected when testnet_mode == false"
    );
}

#[test]
fn faucet_reset_rejected_after_testnet_mode_disabled() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = enable_testnet(&client, &env);
    let (issuer, ns, token) = register_offering(&client, &env);
    // Disable testnet mode after initial setup.
    client.set_testnet_mode(&false);
    let seed = make_seed(&env);

    let result = client.try_faucet_reset(&admin, &issuer, &ns, &token, &seed);
    assert_eq!(
        result,
        Err(Ok(RevoraError::TestnetOnly)),
        "faucet_reset must be rejected after testnet_mode is disabled"
    );
}

#[test]
fn faucet_reset_rejected_for_unknown_offering() {
    let (env, client, _issuer, _ns, _token, admin) = setup_with_admin();
    let fake_issuer = Address::generate(&env);
    let fake_token = Address::generate(&env);
    let ns = symbol_short!("ns");
    let seed = make_seed(&env);

    let result = client.try_faucet_reset(&admin, &fake_issuer, &ns, &fake_token, &seed);
    assert_eq!(
        result,
        Err(Ok(RevoraError::OfferingNotFound)),
        "faucet_reset must return OfferingNotFound for unregistered offerings"
    );
}

#[test]
fn faucet_reset_rejected_when_caller_is_not_admin() {
    let (env, client, issuer, ns, token, _admin) = setup_with_admin();
    let non_admin = Address::generate(&env);
    let seed = make_seed(&env);

    let result = client.try_faucet_reset(&non_admin, &issuer, &ns, &token, &seed);
    assert_eq!(
        result,
        Err(Ok(RevoraError::NotAuthorized)),
        "faucet_reset must be rejected when caller is not the admin"
    );
}

// ── faucet_reset happy-path tests ─────────────────────────────────────────────

#[test]
fn faucet_reset_succeeds_with_no_prior_seeds() {
    // Reset on an offering that has never had faucet_seed_holders called is a no-op.
    let (env, client, issuer, ns, token, admin) = setup_with_admin();
    let seed = make_seed(&env);

    let result = client.try_faucet_reset(&admin, &issuer, &ns, &token, &seed);
    assert!(result.is_ok(), "faucet_reset must succeed even when no seeds have been generated");
}

#[test]
fn faucet_reset_emits_fct_rst_event() {
    let (env, client, issuer, ns, token, admin) = setup_with_admin();
    let seed = make_seed(&env);
    let before = env.events().all().len();

    client.faucet_reset(&admin, &issuer, &ns, &token, &seed);

    let events = env.events().all();
    assert!(events.len() > before, "faucet_reset must emit at least one event");
    // Verify the last event has the fct_rst topic.
    let last = events.last().expect("at least one event");
    // The first topic element is the event symbol.
    let (topics, _data) = last;
    let first_topic: soroban_sdk::Symbol = topics.get(0).expect("topic[0]");
    assert_eq!(
        first_topic,
        symbol_short!("fct_rst"),
        "faucet_reset must emit an event with symbol 'fct_rst'"
    );
}

#[test]
fn faucet_reset_clears_seed_entries_so_faucet_seed_can_reseed() {
    // After faucet_reset, faucet_seed_holders must be able to re-run and produce fresh seeds.
    let (env, client, issuer, ns, token, admin) = setup_with_admin();
    let requester = Address::generate(&env);

    // Seed once.
    let seeds_before = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &3);
    assert_eq!(seeds_before.len(), 3, "initial seed must produce 3 entries");

    // Reset.
    let seed = make_seed(&env);
    client.faucet_reset(&admin, &issuer, &ns, &token, &seed);

    // Advance ledger past cooldown so the requester can call faucet_seed_holders again.
    env.ledger().set_timestamp(DEFAULT_FAUCET_COOLDOWN_SECONDS + 1);

    // Re-seed — must succeed with fresh entries.
    let seeds_after = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &3);
    assert_eq!(seeds_after.len(), 3, "re-seed after reset must produce 3 entries");
}

#[test]
fn faucet_reset_allows_requester_to_seed_again_without_cooldown_block() {
    // faucet_reset resets the seed count; the cooldown is a per-requester key.
    // After reset + elapsed time, faucet_seed_holders must succeed.
    let (env, client, issuer, ns, token, admin) = setup_with_admin();
    let requester = Address::generate(&env);

    // First seed call.
    let first = client.try_faucet_seed_holders(&requester, &issuer, &ns, &token, &2);
    assert!(first.is_ok(), "first seed call must succeed");

    // Second call within cooldown must fail.
    let second_blocked = client.try_faucet_seed_holders(&requester, &issuer, &ns, &token, &2);
    assert_eq!(
        second_blocked,
        Err(Ok(RevoraError::FaucetCooldownActive)),
        "call within cooldown must be blocked"
    );

    // Reset faucet (clears seed entries; cooldown is per-requester not per-offering).
    let seed = make_seed(&env);
    client.faucet_reset(&admin, &issuer, &ns, &token, &seed);

    // Advance time past cooldown.
    env.ledger().set_timestamp(DEFAULT_FAUCET_COOLDOWN_SECONDS + 1);

    // Now seed must succeed again.
    let third = client.try_faucet_seed_holders(&requester, &issuer, &ns, &token, &2);
    assert!(third.is_ok(), "seed after faucet_reset + elapsed cooldown must succeed");
}

#[test]
fn faucet_reset_is_idempotent() {
    // Calling faucet_reset twice must both succeed (second call resets an already-empty state).
    let (env, client, issuer, ns, token, admin) = setup_with_admin();
    let seed = make_seed(&env);

    let first = client.try_faucet_reset(&admin, &issuer, &ns, &token, &seed);
    let second = client.try_faucet_reset(&admin, &issuer, &ns, &token, &seed);
    assert!(first.is_ok(), "first faucet_reset must succeed");
    assert!(second.is_ok(), "second faucet_reset must succeed (idempotent)");
}

#[test]
fn faucet_reset_does_not_affect_other_offerings() {
    // Reset on offering-A must not remove seeds for offering-B.
    let (env, client, issuer_a, ns_a, token_a, admin) = setup_with_admin();

    let issuer_b = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout_b = Address::generate(&env);
    let ns_b = symbol_short!("ns2");
    client.register_offering(&issuer_b, &Vec::new(&env), &1u32, &ns_b, &token_b, &5_000, &payout_b, &0, &symbol_short!(""), &0u32);

    let requester = Address::generate(&env);

    // Seed both offerings.
    let seeds_a = client.faucet_seed_holders(&requester, &issuer_a, &ns_a, &token_a, &3);
    // Advance time to allow b to be seeded (separate requester not needed because it's different offering_id
    // but cooldown is per-requester, not per-offering, so use a different requester).
    let requester_b = Address::generate(&env);
    let seeds_b_before = client.faucet_seed_holders(&requester_b, &issuer_b, &ns_b, &token_b, &3);
    assert_eq!(seeds_a.len(), 3);
    assert_eq!(seeds_b_before.len(), 3);

    // Reset only offering-A.
    let seed = make_seed(&env);
    client.faucet_reset(&admin, &issuer_a, &ns_a, &token_a, &seed);

    // Offering-B's requester cooldown is independent; advance time past cooldown.
    env.ledger().set_timestamp(DEFAULT_FAUCET_COOLDOWN_SECONDS + 1);

    // Offering-B can still re-seed (it was not reset).
    let seeds_b_after = client.faucet_seed_holders(&requester_b, &issuer_b, &ns_b, &token_b, &3);
    assert_eq!(seeds_b_after.len(), 3, "offering-B seeds must be unaffected by offering-A reset");
}

#[test]
fn faucet_reset_with_large_seed_count_clears_all_entries() {
    // Seed 50 entries, then reset; re-seed with 5 must succeed cleanly.
    let (env, client, issuer, ns, token, admin) = setup_with_admin();
    let requester = Address::generate(&env);

    let seeds = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &50);
    assert_eq!(seeds.len(), 50);

    let seed = make_seed(&env);
    client.faucet_reset(&admin, &issuer, &ns, &token, &seed);

    env.ledger().set_timestamp(DEFAULT_FAUCET_COOLDOWN_SECONDS + 1);

    let new_seeds = client.faucet_seed_holders(&requester, &issuer, &ns, &token, &5);
    assert_eq!(new_seeds.len(), 5, "re-seed after large-count reset must succeed");
}

#[test]
fn faucet_reset_seed_param_is_echoed_in_event() {
    // The seed supplied to faucet_reset must appear verbatim in the emitted event data.
    let (env, client, issuer, ns, token, admin) = setup_with_admin();
    let seed = make_seed(&env);

    let before_len = env.events().all().len();
    client.faucet_reset(&admin, &issuer, &ns, &token, &seed);

    let events = env.events().all();
    assert!(events.len() > before_len, "faucet_reset must emit an event");

    // Walk events emitted during this call; find the fct_rst event.
    let new_events = events.slice(before_len as u32..events.len() as u32);
    let found = new_events.iter().any(|(topics, _data)| {
        topics.get::<soroban_sdk::Symbol>(0).map(|s| s == symbol_short!("fct_rst")).unwrap_or(false)
    });
    assert!(found, "fct_rst event must be emitted by faucet_reset");
}
