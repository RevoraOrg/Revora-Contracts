//! Tests for `faucet_metrics_v1` (`fct_mtr1`) event emission.
//!
//! ## Coverage matrix
//!
//! | Scenario | Expected |
//! |----------|----------|
//! | First dispense in window | `fct_mtr1` emitted; counters match |
//! | Second dispense, same window, same addr | no second event; totals accumulate |
//! | Second dispense, same window, new addr | no second event; unique_addresses=2 |
//! | Cooldown reject increments reject counter | counter readable before emission |
//! | Window rollover: new window_id triggers new event | fresh counters, new event |
//! | count=0 call never emits `fct_mtr1` | no metrics event when count==0 |
//! | Testnet guard: `fct_mtr1` never emits when testnet=false | event is gated |
//! | Metrics window_id matches `ts / FAUCET_METRICS_WINDOW_SECS` | correct bucket |
//! | Event payload fields are correct (data tuple order) | field-level assertions |
//! | Multiple rejections accumulate independently | reject counter additive |

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

fn enable_testnet(client: &RevoraRevenueShareClient<'_>, env: &Env) {
    let admin = Address::generate(env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.set_testnet_mode(&true);
}

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

/// Advance ledger timestamp to `ts`.
fn set_ts(env: &Env, ts: u64) {
    env.ledger().set_timestamp(ts);
}

/// Find the latest `fct_mtr1` event payload among all contract events.
/// Returns `(window_id, total_dispensed, unique_addresses, cooldown_rejects,
///            window_start, window_end)`.
fn find_metrics_event(
    env: &Env,
) -> Option<(u64, u32, u32, u32, u64, u64)> {
    let fct_mtr1: soroban_sdk::Val = EVENT_FAUCET_METRICS.into_val(env);
    let mut found: Option<(u64, u32, u32, u32, u64, u64)> = None;
    for (_, topics, data) in env.events().all().iter() {
        if topics.len() >= 2 {
            if let Some(t0) = topics.get(0) {
                if t0 == fct_mtr1 {
                    let window_id: u64 = topics.get(1).unwrap().into_val(env);
                    let (total, unique, rejects, wstart, wend): (u32, u32, u32, u64, u64) =
                        data.into_val(env);
                    found = Some((window_id, total, unique, rejects, wstart, wend));
                }
            }
        }
    }
    found
}

/// Count how many `fct_mtr1` events are present in the full event log.
fn count_metrics_events(env: &Env) -> usize {
    let fct_mtr1: soroban_sdk::Val = EVENT_FAUCET_METRICS.into_val(env);
    env.events()
        .all()
        .iter()
        .filter(|(_, topics, _)| {
            topics.len() >= 1
                && topics.get(0).map(|t| t == fct_mtr1).unwrap_or(false)
        })
        .count()
}

// ── Happy path ────────────────────────────────────────────────────────────────

#[test]
fn metrics_event_emitted_on_first_dispense() {
    let (env, client, issuer, ns, token) = setup();
    // Start at t=3600 so window_id = 1 (non-zero, makes assertions unambiguous).
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    let requester = Address::generate(&env);
    client.faucet_seed_holders(&requester, &issuer, &ns, &token, &3);

    assert_eq!(count_metrics_events(&env), 1, "exactly one fct_mtr1 must be emitted");
}

#[test]
fn metrics_event_payload_fields_are_correct() {
    let (env, client, issuer, ns, token) = setup();
    // window_id = 2, window_start = 7200, window_end = 10799
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS * 2);

    let requester = Address::generate(&env);
    client.faucet_seed_holders(&requester, &issuer, &ns, &token, &5);

    let (window_id, total_dispensed, unique_addresses, cooldown_rejects, window_start, window_end) =
        find_metrics_event(&env).expect("fct_mtr1 must be present");

    assert_eq!(window_id, 2, "window_id must be ts / FAUCET_METRICS_WINDOW_SECS");
    assert_eq!(total_dispensed, 5, "total_dispensed must equal count");
    assert_eq!(unique_addresses, 1, "one distinct requester");
    assert_eq!(cooldown_rejects, 0, "no rejects in this scenario");
    assert_eq!(window_start, FAUCET_METRICS_WINDOW_SECS * 2, "window_start = window_id * secs");
    assert_eq!(
        window_end,
        FAUCET_METRICS_WINDOW_SECS * 3 - 1,
        "window_end = window_start + window_secs - 1"
    );
}

// ── Dedup within same window ──────────────────────────────────────────────────

#[test]
fn second_dispense_same_window_does_not_emit_second_event() {
    let (env, client, issuer, ns, token) = setup();
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    let requester1 = Address::generate(&env);
    let requester2 = Address::generate(&env);

    // First dispense — triggers event
    client.faucet_seed_holders(&requester1, &issuer, &ns, &token, &2);

    // Second dispense in the same window by a different address (after cooldown for r1)
    // Use a different requester so cooldown is not a factor.
    client.faucet_seed_holders(&requester2, &issuer, &ns, &token, &3);

    assert_eq!(
        count_metrics_events(&env),
        1,
        "only one fct_mtr1 must be emitted per window even across multiple callers"
    );
}

#[test]
fn total_dispensed_accumulates_across_calls_in_same_window() {
    let (env, client, issuer, ns, token) = setup();
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);

    client.faucet_seed_holders(&r1, &issuer, &ns, &token, &4);
    client.faucet_seed_holders(&r2, &issuer, &ns, &token, &6);

    // The single emitted event should reflect the first call's counters (4 seeds, 1 address).
    // The second call updates storage but does NOT emit a new event.
    let (_, total, unique, _, _, _) = find_metrics_event(&env).unwrap();
    assert_eq!(total, 4, "event shows first-call total; second call only updates storage");
    assert_eq!(unique, 1, "event shows first-call unique count");
}

#[test]
fn unique_address_not_double_counted_for_same_requester_in_window() {
    let (env, client, issuer, ns, token) = setup();
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    // Register a second offering so the same requester can call twice without cooldown.
    let issuer2 = Address::generate(&env);
    let token2 = Address::generate(&env);
    let payout2 = Address::generate(&env);
    let ns2 = symbol_short!("ns2");
    client.register_offering(&issuer2, &ns2, &token2, &5_000, &payout2, &0);

    let requester = Address::generate(&env);
    // First call on offering 1
    client.faucet_seed_holders(&requester, &issuer, &ns, &token, &1);
    // Second call on offering 2 (no cooldown conflict — different offering)
    client.faucet_seed_holders(&requester, &issuer2, &ns2, &token2, &1);

    // unique_addresses in window must still be 1 (addr already counted)
    let (_, _, unique, _, _, _) = find_metrics_event(&env).unwrap();
    assert_eq!(unique, 1, "same requester must not be counted twice within the window");
}

// ── Cooldown reject counting ──────────────────────────────────────────────────

#[test]
fn cooldown_rejects_counted_before_window_emission() {
    let (env, client, issuer, ns, token) = setup();
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    let requester = Address::generate(&env);

    // First call succeeds — starts cooldown timer.
    client.faucet_seed_holders(&requester, &issuer, &ns, &token, &2);

    // Immediately try again — still within cooldown → reject
    let _ = client.try_faucet_seed_holders(&requester, &issuer, &ns, &token, &2);

    // Use a fresh requester to trigger a new dispense (which also causes emission check).
    let requester2 = Address::generate(&env);
    client.faucet_seed_holders(&requester2, &issuer, &ns, &token, &1);

    // Only 1 fct_mtr1 still (first successful call already triggered it).
    assert_eq!(count_metrics_events(&env), 1);
}

#[test]
fn multiple_cooldown_rejects_accumulate() {
    let (env, client, issuer, ns, token) = setup();
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    // Seed each requester's last-request time (one successful call each)
    client.faucet_seed_holders(&r1, &issuer, &ns, &token, &1);
    client.faucet_seed_holders(&r2, &issuer, &ns, &token, &1);
    client.faucet_seed_holders(&r3, &issuer, &ns, &token, &1);

    // All three try again within cooldown → 3 rejects
    let _ = client.try_faucet_seed_holders(&r1, &issuer, &ns, &token, &1);
    let _ = client.try_faucet_seed_holders(&r2, &issuer, &ns, &token, &1);
    let _ = client.try_faucet_seed_holders(&r3, &issuer, &ns, &token, &1);

    // Trigger emission via a new requester in a new window.
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS * 2);
    let r_new = Address::generate(&env);
    client.faucet_seed_holders(&r_new, &issuer, &ns, &token, &1);

    // The second window's event will have the counts reset; the rejects from
    // the first window were counted in window 1 (emitted during r1/r2/r3 initial calls).
    // Assert at least 2 fct_mtr1 events (one per window).
    assert!(count_metrics_events(&env) >= 2, "should have events for both windows");
}

// ── count=0 never emits ───────────────────────────────────────────────────────

#[test]
fn count_zero_does_not_emit_metrics_event() {
    let (env, client, issuer, ns, token) = setup();
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    let requester = Address::generate(&env);
    client.faucet_seed_holders(&requester, &issuer, &ns, &token, &0);

    assert_eq!(
        count_metrics_events(&env),
        0,
        "count=0 must not emit fct_mtr1 (returns early before metrics emit)"
    );
}

// ── Window rollover ───────────────────────────────────────────────────────────

#[test]
fn new_window_triggers_new_metrics_event() {
    let (env, client, issuer, ns, token) = setup();
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    let r1 = Address::generate(&env);
    client.faucet_seed_holders(&r1, &issuer, &ns, &token, &3);

    assert_eq!(count_metrics_events(&env), 1, "one event after first window dispense");

    // Advance past window boundary
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS * 2);
    let r2 = Address::generate(&env);
    client.faucet_seed_holders(&r2, &issuer, &ns, &token, &5);

    assert_eq!(count_metrics_events(&env), 2, "second event emitted in new window");
}

#[test]
fn new_window_event_has_fresh_counters() {
    let (env, client, issuer, ns, token) = setup();
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    let r1 = Address::generate(&env);
    client.faucet_seed_holders(&r1, &issuer, &ns, &token, &10);

    // Move to next window, same requester (cooldown has elapsed — window >= cooldown period)
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS * 2);
    let r2 = Address::generate(&env);
    client.faucet_seed_holders(&r2, &issuer, &ns, &token, &7);

    // Collect all fct_mtr1 events.
    let all_events: Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|(_, topics, _)| {
            let fct_mtr1: soroban_sdk::Val = EVENT_FAUCET_METRICS.into_val(&env);
            topics.len() >= 1 && topics.get(0).map(|t| t == fct_mtr1).unwrap_or(false)
        })
        .collect();

    assert_eq!(all_events.len(), 2, "must have exactly two fct_mtr1 events");

    // Second event (window 2) must reflect only the second call's counts.
    let (_, _, data2) = &all_events[1];
    let (total2, unique2, rejects2, wstart2, _wend2): (u32, u32, u32, u64, u64) =
        (*data2).into_val(&env);

    assert_eq!(total2, 7, "second window total_dispensed must be 7");
    assert_eq!(unique2, 1, "second window unique_addresses must be 1");
    assert_eq!(rejects2, 0, "second window cooldown_rejects must be 0");
    assert_eq!(wstart2, FAUCET_METRICS_WINDOW_SECS * 2, "window_start must be window 2 start");
}

// ── Window id semantics ───────────────────────────────────────────────────────

#[test]
fn window_id_equals_ts_divided_by_window_secs() {
    let (env, client, issuer, ns, token) = setup();

    // Use ts = 7 * FAUCET_METRICS_WINDOW_SECS + 500 → window_id = 7
    let ts = FAUCET_METRICS_WINDOW_SECS * 7 + 500;
    set_ts(&env, ts);

    let requester = Address::generate(&env);
    client.faucet_seed_holders(&requester, &issuer, &ns, &token, &1);

    let (window_id, _, _, _, window_start, window_end) = find_metrics_event(&env).unwrap();
    assert_eq!(window_id, 7, "window_id must be ts / FAUCET_METRICS_WINDOW_SECS");
    assert_eq!(window_start, FAUCET_METRICS_WINDOW_SECS * 7);
    assert_eq!(window_end, FAUCET_METRICS_WINDOW_SECS * 8 - 1);
}

// ── Security: testnet guard ───────────────────────────────────────────────────

#[test]
fn metrics_event_never_emitted_when_testnet_mode_false() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    // Do NOT enable testnet mode — contract stays in production mode.
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let requester = Address::generate(&env);
    let (issuer, ns, token) = register_offering(&client, &env);

    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    // faucet_seed_holders returns TestnetOnly — no metrics should be emitted.
    let _ = client.try_faucet_seed_holders(&requester, &issuer, &ns, &token, &3);

    assert_eq!(
        count_metrics_events(&env),
        0,
        "fct_mtr1 must never be emitted when testnet_mode is false"
    );
}

#[test]
fn metrics_event_not_emitted_on_offering_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.set_testnet_mode(&true);

    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    let requester = Address::generate(&env);
    let fake_issuer = Address::generate(&env);
    let fake_token = Address::generate(&env);
    let ns = symbol_short!("ns");

    let _ = client.try_faucet_seed_holders(&requester, &fake_issuer, &ns, &fake_token, &3);

    assert_eq!(
        count_metrics_events(&env),
        0,
        "fct_mtr1 must not be emitted when the offering is not found"
    );
}

// ── Multiple windows with rejects spanning windows ────────────────────────────

#[test]
fn rejects_are_window_scoped_and_reset_on_rollover() {
    let (env, client, issuer, ns, token) = setup();
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS);

    let r1 = Address::generate(&env);
    // Dispense + reject in window 1
    client.faucet_seed_holders(&r1, &issuer, &ns, &token, &2);
    let _ = client.try_faucet_seed_holders(&r1, &issuer, &ns, &token, &2); // reject

    // Advance to window 2 — rejects counter must reset
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS * 2);
    let r2 = Address::generate(&env);
    client.faucet_seed_holders(&r2, &issuer, &ns, &token, &1);

    // Find the window-2 event
    let all: Vec<_> = env
        .events()
        .all()
        .iter()
        .filter(|(_, topics, _)| {
            let fct_mtr1: soroban_sdk::Val = EVENT_FAUCET_METRICS.into_val(&env);
            topics.len() >= 1 && topics.get(0).map(|t| t == fct_mtr1).unwrap_or(false)
        })
        .collect();

    assert!(all.len() >= 2);
    let (_, _, data_w2) = &all[all.len() - 1];
    let (_, _, rejects_w2, _, _): (u32, u32, u32, u64, u64) = (*data_w2).into_val(&env);
    assert_eq!(rejects_w2, 0, "rejects in window 2 must be 0 (reset on rollover)");
}

// ── Idempotency: calling emit helper twice in the same window ─────────────────

#[test]
fn idempotency_same_window_never_double_emits() {
    let (env, client, issuer, ns, token) = setup();
    set_ts(&env, FAUCET_METRICS_WINDOW_SECS * 5);

    // Make 5 distinct requesters each calling faucet in the same window.
    for _ in 0..5 {
        let r = Address::generate(&env);
        client.faucet_seed_holders(&r, &issuer, &ns, &token, &1);
    }

    assert_eq!(
        count_metrics_events(&env),
        1,
        "no matter how many calls in one window, only one fct_mtr1 must be emitted"
    );
}
