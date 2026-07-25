//! TWAP (Time-Weighted Average Price) accumulator tests.
//!
//! Covers:
//!  1. First sample initialises accumulator correctly
//!  2. Second sample accumulates price-time area
//!  3. Identical timestamp does NOT double-count cumulative price
//!  4. get_twap returns None before any sample
//!  5. get_twap basic weighted-average calculation
//!  6. get_twap window clamping (open interval capped at window_secs)
//!  7. window_secs = 0 rejected with LimitReached
//!  8. window_secs > MAX_TWAP_WINDOW_SECS rejected
//!  9. Per-offering max_window_secs enforced by set_twap_window
//! 10. set_twap_window rejects 0 and values > MAX_TWAP_WINDOW_SECS
//! 11. sample_twap requires issuer auth (host-panic test)
//! 12. sample_twap rejects negative price
//! 13. sample_twap rejects unknown offering
//! 14. set_twap_window requires known offering
//! 15. get_twap_accumulator returns raw state / None before sample
//! 16. get_twap_config returns None before set, correct value after
//! 17. Multiple samples accumulate area correctly
//! 18. Stale accumulator: last sample older than window uses last_price
//! 19. Large price values stay within i128 range (saturating, no panic)
//! 20. Frozen contract blocks sample_twap
//! 21. Regression: identical-ts guard preserves cumulative when non-zero
//! 22. Zero price is valid (price = 0 accepted)
//! 23. Single sample + open interval TWAP
//! 24. Namespace isolation — TWAPs are scoped per offering
//! 25. set_twap_window config can be updated multiple times
//! 26. get_twap at exact window boundary

#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Symbol,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);
    (env, id, client)
}

/// Register a minimal offering and return (issuer, namespace, token).
fn register(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Symbol, Address) {
    let issuer = Address::generate(env);
    let namespace = Symbol::new(env, "ns");
    let token = Address::generate(env);
    client.register_offering(&issuer, &namespace, &token, &500, &token, &0);
    (issuer, namespace, token)
}

fn set_ts(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

// ── 1. First sample initialises accumulator ──────────────────────────────────

#[test]
fn twap_first_sample_sets_accumulator() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 1_000);
    client.sample_twap(&issuer, &ns, &token, &500_000);

    let acc = client.get_twap_accumulator(&issuer, &ns, &token).unwrap();
    assert_eq!(acc.last_ts, 1_000);
    assert_eq!(acc.last_price, 500_000);
    // No previous sample → cumulative_price_secs must be 0
    assert_eq!(acc.cumulative_price_secs, 0);
}

// ── 2. Second sample accumulates area ────────────────────────────────────────

#[test]
fn twap_second_sample_accumulates_area() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 1_000);
    client.sample_twap(&issuer, &ns, &token, &1_000);

    set_ts(&env, 1_100); // elapsed = 100 s
    client.sample_twap(&issuer, &ns, &token, &2_000);

    let acc = client.get_twap_accumulator(&issuer, &ns, &token).unwrap();
    assert_eq!(acc.last_ts, 1_100);
    assert_eq!(acc.last_price, 2_000);
    // area = 1_000 * 100 = 100_000
    assert_eq!(acc.cumulative_price_secs, 100_000);
}

// ── 3. Identical timestamp does not double-count ─────────────────────────────

#[test]
fn twap_identical_timestamp_no_double_count() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 5_000);
    client.sample_twap(&issuer, &ns, &token, &1_000);

    // Same timestamp — elapsed = 0, area added = 0
    client.sample_twap(&issuer, &ns, &token, &9_999);

    let acc = client.get_twap_accumulator(&issuer, &ns, &token).unwrap();
    assert_eq!(acc.last_ts, 5_000);
    assert_eq!(acc.last_price, 9_999);
    assert_eq!(acc.cumulative_price_secs, 0, "identical-ts must not add area");
}

// ── 4. get_twap returns None before any sample ───────────────────────────────

#[test]
fn twap_get_twap_none_before_any_sample() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 1_000);
    // No sample recorded → accumulator absent → get_twap returns None (Option)
    // try_get_twap returns Ok(None)
    let result = client.try_get_twap(&issuer, &ns, &token, &3_600).unwrap().unwrap();
    assert_eq!(result, None);
}

// ── 5. get_twap basic calculation ────────────────────────────────────────────

#[test]
fn twap_basic_calculation() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    // t=0: price=1000
    set_ts(&env, 0);
    client.sample_twap(&issuer, &ns, &token, &1_000);

    // t=100: price=2000  → cumulative += 1000*100 = 100_000
    set_ts(&env, 100);
    client.sample_twap(&issuer, &ns, &token, &2_000);

    // t=200: ask TWAP over 200s window
    // open interval [100..200] = 2000*100 = 200_000
    // total area = 100_000 + 200_000 = 300_000
    // TWAP = 300_000 / 200 = 1_500
    set_ts(&env, 200);
    let twap = client.get_twap(&issuer, &ns, &token, &200).unwrap();
    assert_eq!(twap, 1_500);
}

// ── 6. get_twap window clamping ───────────────────────────────────────────────

#[test]
fn twap_window_clamps_open_interval() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    // t=0: price=1000; no previous sample
    set_ts(&env, 0);
    client.sample_twap(&issuer, &ns, &token, &1_000);

    // t=500: last sample 500s ago, window=100s
    // open_elapsed=500 clamped to 100 → area = 1000*100 = 100_000
    // TWAP = 100_000 / 100 = 1_000
    set_ts(&env, 500);
    let twap = client.get_twap(&issuer, &ns, &token, &100).unwrap();
    assert_eq!(twap, 1_000);
}

// ── 7. window_secs = 0 rejected ──────────────────────────────────────────────

#[test]
fn twap_zero_window_rejected() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 1_000);
    client.sample_twap(&issuer, &ns, &token, &500);

    let err = client.try_get_twap(&issuer, &ns, &token, &0).unwrap_err().unwrap();
    assert_eq!(err, RevoraError::LimitReached);
}

// ── 8. window_secs > MAX_TWAP_WINDOW_SECS rejected ───────────────────────────

#[test]
fn twap_window_exceeds_max_rejected() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 1_000);
    client.sample_twap(&issuer, &ns, &token, &500);

    let over = MAX_TWAP_WINDOW_SECS + 1;
    let err = client.try_get_twap(&issuer, &ns, &token, &over).unwrap_err().unwrap();
    assert_eq!(err, RevoraError::LimitReached);
}

// ── 9. Per-offering max_window_secs enforced ─────────────────────────────────

#[test]
fn twap_per_offering_window_cap_enforced() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    // Cap this offering at 3600s
    client.set_twap_window(&issuer, &ns, &token, &3_600);

    set_ts(&env, 1_000);
    client.sample_twap(&issuer, &ns, &token, &500);

    // Exactly at the cap — should succeed
    set_ts(&env, 5_000);
    let result = client.get_twap(&issuer, &ns, &token, &3_600);
    assert!(result.is_some());

    // One second over the cap — should fail
    let err = client.try_get_twap(&issuer, &ns, &token, &3_601).unwrap_err().unwrap();
    assert_eq!(err, RevoraError::LimitReached);
}

// ── 10. set_twap_window validates bounds ─────────────────────────────────────

#[test]
fn twap_set_window_zero_rejected() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    let err = client.try_set_twap_window(&issuer, &ns, &token, &0).unwrap_err().unwrap();
    assert_eq!(err, RevoraError::LimitReached);
}

#[test]
fn twap_set_window_over_max_rejected() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    let over = MAX_TWAP_WINDOW_SECS + 1;
    let err = client.try_set_twap_window(&issuer, &ns, &token, &over).unwrap_err().unwrap();
    assert_eq!(err, RevoraError::LimitReached);
}

#[test]
fn twap_set_window_max_boundary_accepted() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    // Exactly MAX_TWAP_WINDOW_SECS should be accepted.
    client.set_twap_window(&issuer, &ns, &token, &MAX_TWAP_WINDOW_SECS);
    let cfg = client.get_twap_config(&issuer, &ns, &token).unwrap();
    assert_eq!(cfg.max_window_secs, MAX_TWAP_WINDOW_SECS);
}

// ── 11. sample_twap requires issuer auth ─────────────────────────────────────

#[test]
#[should_panic]
fn twap_sample_requires_auth() {
    // env without mock_all_auths — require_auth() will panic
    let env = Env::default();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    // register with mocked auth
    env.mock_all_auths();
    let (issuer, ns, token) = register(&env, &client);
    // clear auth mocks
    env.set_auths(&[]);

    // This must panic because no auth is provided
    client.sample_twap(&issuer, &ns, &token, &100);
}

// ── 12. sample_twap rejects negative price ────────────────────────────────────

#[test]
fn twap_negative_price_rejected() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 1_000);
    let err = client.try_sample_twap(&issuer, &ns, &token, &-1).unwrap_err().unwrap();
    assert_eq!(err, RevoraError::InvalidAmount);
}

// ── 13. sample_twap rejects unknown offering ──────────────────────────────────

#[test]
fn twap_unknown_offering_rejected() {
    let (env, _id, client) = setup();
    let issuer = Address::generate(&env);
    let ns = Symbol::new(&env, "ns");
    let ghost = Address::generate(&env); // never registered

    set_ts(&env, 1_000);
    let err = client.try_sample_twap(&issuer, &ns, &ghost, &500).unwrap_err().unwrap();
    assert_eq!(err, RevoraError::OfferingNotFound);
}

// ── 14. set_twap_window requires known offering ───────────────────────────────

#[test]
fn twap_set_window_unknown_offering_rejected() {
    let (env, _id, client) = setup();
    let issuer = Address::generate(&env);
    let ns = Symbol::new(&env, "ns");
    let ghost = Address::generate(&env);

    let err = client.try_set_twap_window(&issuer, &ns, &ghost, &3_600).unwrap_err().unwrap();
    assert_eq!(err, RevoraError::OfferingNotFound);
}

// ── 15. get_twap_accumulator returns raw state / None before sample ───────────

#[test]
fn twap_get_accumulator_none_before_sample() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    assert_eq!(client.get_twap_accumulator(&issuer, &ns, &token), None);
}

#[test]
fn twap_get_accumulator_raw_state_after_sample() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 2_000);
    client.sample_twap(&issuer, &ns, &token, &300);

    let acc = client.get_twap_accumulator(&issuer, &ns, &token).unwrap();
    assert_eq!(acc.last_ts, 2_000);
    assert_eq!(acc.last_price, 300);
    assert_eq!(acc.cumulative_price_secs, 0);
}

// ── 16. get_twap_config before/after set ──────────────────────────────────────

#[test]
fn twap_get_config_none_by_default() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    assert_eq!(client.get_twap_config(&issuer, &ns, &token), None);
}

#[test]
fn twap_get_config_returns_set_value() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    client.set_twap_window(&issuer, &ns, &token, &7_200);
    let cfg = client.get_twap_config(&issuer, &ns, &token).unwrap();
    assert_eq!(cfg.max_window_secs, 7_200);
}

// ── 17. Multiple samples accumulate correctly ─────────────────────────────────

#[test]
fn twap_multiple_samples_accumulate() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    // t=0   price=100
    set_ts(&env, 0);
    client.sample_twap(&issuer, &ns, &token, &100);

    // t=100 price=200  → cumul += 100*100 = 10_000
    set_ts(&env, 100);
    client.sample_twap(&issuer, &ns, &token, &200);

    // t=300 price=400  → cumul += 200*200 = 40_000  ⇒ total = 50_000
    set_ts(&env, 300);
    client.sample_twap(&issuer, &ns, &token, &400);

    let acc = client.get_twap_accumulator(&issuer, &ns, &token).unwrap();
    assert_eq!(acc.cumulative_price_secs, 50_000);

    // At t=300 ask TWAP over 300s: open=0, total=50_000, TWAP=50_000/300=166
    let twap = client.get_twap(&issuer, &ns, &token, &300).unwrap();
    assert_eq!(twap, 166); // integer division truncation
}

// ── 18. Stale accumulator: last sample older than window ─────────────────────

#[test]
fn twap_stale_accumulator_uses_last_price() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    // t=0 price=1000 (only sample ever)
    set_ts(&env, 0);
    client.sample_twap(&issuer, &ns, &token, &1_000);

    // At t=10_000, ask 100s window:
    // open_elapsed=10_000 clamped to 100 → area=1000*100=100_000
    // cumulative=0, TWAP=100_000/100=1_000
    set_ts(&env, 10_000);
    let twap = client.get_twap(&issuer, &ns, &token, &100).unwrap();
    assert_eq!(twap, 1_000);
}

// ── 19. Large price values — saturating, no panic ────────────────────────────

#[test]
fn twap_large_price_no_panic() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    let big: i128 = i64::MAX as i128; // ~9.2e18, well within i128

    set_ts(&env, 0);
    client.sample_twap(&issuer, &ns, &token, &big);

    // Advance MAX_TWAP_WINDOW_SECS (worst-case elapsed)
    set_ts(&env, MAX_TWAP_WINDOW_SECS);
    client.sample_twap(&issuer, &ns, &token, &big);

    // Must not panic; saturating keeps result non-negative
    let acc = client.get_twap_accumulator(&issuer, &ns, &token).unwrap();
    assert!(acc.cumulative_price_secs >= 0);

    // get_twap must return Ok (not panic or error)
    let result = client.try_get_twap(&issuer, &ns, &token, &MAX_TWAP_WINDOW_SECS);
    assert!(result.is_ok());
}

// ── 20. Frozen contract blocks sample_twap ───────────────────────────────────

#[test]
fn twap_frozen_contract_blocks_sample() {
    let (env, contract_id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::Frozen, &true);
    });

    set_ts(&env, 1_000);
    let err = client.try_sample_twap(&issuer, &ns, &token, &500).unwrap_err().unwrap();
    assert_eq!(err, RevoraError::ContractFrozen);
}

// ── 21. Regression: identical-ts guard preserves existing cumulative ──────────
//
// Regression Test: Identical-timestamp guard with pre-existing cumulative state
//
// **Related Issue:** feat/twap-valuation-oracle
//
// **Original Bug:**
// An earlier draft cleared `cumulative_price_secs` on same-ledger re-sample.
// This test confirms that when `elapsed == 0`, the existing `cumulative_price_secs`
// value is preserved and only `last_price` is updated.
//
// **Expected Behavior:**
// cumulative_price_secs unchanged; last_price updated to newest value.
//
// **Fix Applied:**
// area explicitly set to 0_i128 when elapsed == 0; new_cum = old.saturating_add(0).

#[test]
fn regression_identical_ts_preserves_cumulative() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 1_000);
    client.sample_twap(&issuer, &ns, &token, &1_000);

    set_ts(&env, 1_100);
    client.sample_twap(&issuer, &ns, &token, &2_000);
    // cumulative = 1_000 * 100 = 100_000

    // Same ts=1_100 again: must NOT change cumulative
    client.sample_twap(&issuer, &ns, &token, &3_000);

    let acc = client.get_twap_accumulator(&issuer, &ns, &token).unwrap();
    assert_eq!(
        acc.cumulative_price_secs, 100_000,
        "cumulative must be unchanged on same-ledger re-sample"
    );
    assert_eq!(acc.last_price, 3_000, "last_price should update to newest value");
    assert_eq!(acc.last_ts, 1_100);
}

// ── 22. Zero price is valid ───────────────────────────────────────────────────

#[test]
fn twap_zero_price_accepted() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 1_000);
    client.sample_twap(&issuer, &ns, &token, &0);

    let acc = client.get_twap_accumulator(&issuer, &ns, &token).unwrap();
    assert_eq!(acc.last_price, 0);
    assert_eq!(acc.cumulative_price_secs, 0);
}

// ── 23. Single sample + open interval ────────────────────────────────────────

#[test]
fn twap_single_sample_open_interval() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 1_000);
    client.sample_twap(&issuer, &ns, &token, &500);

    // t=1_600 window=600: open=600, area=500*600=300_000, TWAP=300_000/600=500
    set_ts(&env, 1_600);
    let twap = client.get_twap(&issuer, &ns, &token, &600).unwrap();
    assert_eq!(twap, 500);
}

// ── 24. Namespace isolation ───────────────────────────────────────────────────

#[test]
fn twap_namespace_isolation() {
    let (env, _id, client) = setup();

    let issuer = Address::generate(&env);
    let ns_a = Symbol::new(&env, "alpha");
    let ns_b = Symbol::new(&env, "beta");
    let token = Address::generate(&env);

    client.register_offering(&issuer, &ns_a, &token, &100, &token, &0);
    client.register_offering(&issuer, &ns_b, &token, &100, &token, &0);

    set_ts(&env, 0);
    client.sample_twap(&issuer, &ns_a, &token, &100);
    set_ts(&env, 1_000);
    client.sample_twap(&issuer, &ns_a, &token, &200);

    // ns_b: no samples
    assert_eq!(client.get_twap_accumulator(&issuer, &ns_b, &token), None);
    let twap_b = client.try_get_twap(&issuer, &ns_b, &token, &3_600).unwrap().unwrap();
    assert_eq!(twap_b, None);

    // ns_a: correct cumulative = 100 * 1000 = 100_000
    let acc_a = client.get_twap_accumulator(&issuer, &ns_a, &token).unwrap();
    assert_eq!(acc_a.cumulative_price_secs, 100_000);
}

// ── 25. set_twap_window can be updated multiple times ────────────────────────

#[test]
fn twap_window_config_can_be_updated() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    client.set_twap_window(&issuer, &ns, &token, &3_600);
    client.set_twap_window(&issuer, &ns, &token, &7_200);

    let cfg = client.get_twap_config(&issuer, &ns, &token).unwrap();
    assert_eq!(cfg.max_window_secs, 7_200);
}

// ── 26. get_twap at exact window boundary ────────────────────────────────────

#[test]
fn twap_exact_window_boundary() {
    let (env, _id, client) = setup();
    let (issuer, ns, token) = register(&env, &client);

    set_ts(&env, 0);
    client.sample_twap(&issuer, &ns, &token, &1_000);

    // t=3_600: cumul += 1000*3600 = 3_600_000; open=0
    set_ts(&env, 3_600);
    client.sample_twap(&issuer, &ns, &token, &2_000);

    // window=3600 exactly: TWAP = 3_600_000 / 3_600 = 1_000
    let twap = client.get_twap(&issuer, &ns, &token, &3_600).unwrap();
    assert_eq!(twap, 1_000);
}
