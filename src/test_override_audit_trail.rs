//! Override-Revenue Audit Trail Reconstructibility Tests
//!
//! # Purpose
//! Verifies that the sequence of `rev_ovrd` events emitted by `report_revenue`
//! (with `override_existing=true`) is sufficient to deterministically reconstruct
//! the final persisted amount for any period — without reading on-chain storage.
//!
//! # Reconstruction Algorithm (for off-chain indexers)
//! 1. Collect the initial `rev_init` event for a period → `current = init_amount`.
//! 2. For each subsequent `rev_ovrd` event for that period (in emission order):
//!    `current = new_amount`  (the event carries both `new_amount` and `old_amount`).
//! 3. After replaying all events, `current` equals `get_revenue_by_period(period_id)`.
//!
//! # Security Assumptions
//! - Events are emitted in the same transaction that mutates storage; they cannot
//!   diverge from the persisted state within a single successful call.
//! - `AuditSummary.total_revenue` is updated via `saturating_add(new - old)` on
//!   each override; the same delta can be reconstructed from `rev_ovrd` events.
//! - `report_count` is never incremented on override; only `rev_init` events
//!   contribute to the count.
//!
//! # Event Payload Layout
//! `rev_ovrd` data tuple: `(new_amount: i128, period_id: u64, old_amount: i128, blacklist: Vec<Address>)`
//! `rev_ovra` data tuple: `(payout_asset: Address, new_amount: i128, period_id: u64, old_amount: i128, blacklist: Vec<Address>)`

#![cfg(test)]

extern crate alloc;

use super::*;
use alloc::vec::Vec as RustVec;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val, Vec as SdkVec,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &payout_asset, &0);
    (env, contract_id, issuer, token, payout_asset)
}

/// Collect `(new_amount, period_id, old_amount)` tuples from all `rev_ovrd`
/// events emitted at or after `start_idx` in the environment event log.
fn collect_override_events(env: &Env, start_idx: u32) -> RustVec<(i128, u64, i128)> {
    let rev_ovrd_sym: Symbol = symbol_short!("rev_ovrd");
    let mut out = RustVec::new();
    let all = env.events().all();
    for i in start_idx..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        let topics_vec: SdkVec<Val> = topics.into_val(env);
        let topic_sym: Symbol = topics_vec.get(0).unwrap().into_val(env);
        if topic_sym == rev_ovrd_sym {
            let data_vec: SdkVec<Val> = data.into_val(env);
            let new_amount: i128 = data_vec.get(0).unwrap().into_val(env);
            let period_id: u64 = data_vec.get(1).unwrap().into_val(env);
            let old_amount: i128 = data_vec.get(2).unwrap().into_val(env);
            out.push((new_amount, period_id, old_amount));
        }
    }
    out
}

/// Collect the initial amount from the first `rev_init` event for `period_id`
/// emitted at or after `start_idx`.
fn collect_init_amount(env: &Env, start_idx: u32, period_id: u64) -> Option<i128> {
    let rev_init_sym: Symbol = symbol_short!("rev_init");
    let all = env.events().all();
    for i in start_idx..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        let topics_vec: SdkVec<Val> = topics.into_val(env);
        let topic_sym: Symbol = topics_vec.get(0).unwrap().into_val(env);
        if topic_sym == rev_init_sym {
            let data_vec: SdkVec<Val> = data.into_val(env);
            let amount: i128 = data_vec.get(0).unwrap().into_val(env);
            let pid: u64 = data_vec.get(1).unwrap().into_val(env);
            if pid == period_id {
                return Some(amount);
            }
        }
    }
    None
}

/// Replay the override event sequence for `period_id` and return the
/// reconstructed final amount.  Panics if no `rev_init` is found.
fn reconstruct_from_events(env: &Env, start_idx: u32, period_id: u64) -> i128 {
    let init = collect_init_amount(env, start_idx, period_id)
        .expect("rev_init event must exist for the period");
    let overrides = collect_override_events(env, start_idx);
    // Apply overrides in emission order; each carries the authoritative new_amount.
    let mut current = init;
    for (new_amount, pid, _old_amount) in &overrides {
        if *pid == period_id {
            current = *new_amount;
        }
    }
    current
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Five sequential overrides on a single period; the event sequence must
/// reconstruct the final persisted amount exactly.
///
/// Override sequence: 100 → 200 → 50 → 300 → 150 → 250
/// Expected final: 250
#[test]
fn override_audit_trail_five_overrides_reconstructible() {
    let (env, contract_id, issuer, token, payout_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let period_id: u64 = 1;

    let start_idx = env.events().all().len();

    // Initial report
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &100, &period_id, &false);

    // Five overrides with varied deltas
    let override_amounts: [i128; 5] = [200, 50, 300, 150, 250];
    for &amt in &override_amounts {
        client.report_revenue(&issuer, &ns, &token, &payout_asset, &amt, &period_id, &true);
    }

    let persisted = client.get_revenue_by_period(&issuer, &ns, &token, &period_id);
    let reconstructed = reconstruct_from_events(&env, start_idx, period_id);

    assert_eq!(
        reconstructed, persisted,
        "event-reconstructed amount must equal persisted storage value"
    );
    assert_eq!(persisted, 250, "final persisted amount must be the last override value");

    // Exactly 5 rev_ovrd events must have been emitted
    let overrides = collect_override_events(&env, start_idx);
    assert_eq!(overrides.len(), 5, "must emit exactly one rev_ovrd per override call");

    // Each override event must carry the correct (new, old) pair
    let expected_pairs: [(i128, i128); 5] =
        [(200, 100), (50, 200), (300, 50), (150, 300), (250, 150)];
    for (i, &(new_amount, _pid, old_amount)) in overrides.iter().enumerate() {
        assert_eq!(
            (new_amount, old_amount),
            expected_pairs[i],
            "override event {i} must carry correct (new_amount, old_amount)"
        );
    }
}

/// AuditSummary.total_revenue must equal the sum reconstructed by replaying
/// rev_init + rev_ovrd deltas across multiple periods.
///
/// Periods: 1 (init=100, override→200), 2 (init=60, override→10), 3 (init=40, no override)
/// Expected total: 200 + 10 + 40 = 250
#[test]
fn override_audit_trail_total_revenue_reconstructible_from_events() {
    let (env, contract_id, issuer, token, payout_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    let start_idx = env.events().all().len();

    client.report_revenue(&issuer, &ns, &token, &payout_asset, &100, &1, &false);
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &60, &2, &false);
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &40, &3, &false);
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &200, &1, &true);
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &10, &2, &true);

    // Reconstruct total from events: start with all rev_init amounts, then apply deltas
    let init_amounts: [(u64, i128); 3] = [(1, 100), (2, 60), (3, 40)];
    let mut reconstructed_total: i128 = init_amounts.iter().map(|(_, a)| a).sum();
    for (new_amount, _pid, old_amount) in collect_override_events(&env, start_idx) {
        reconstructed_total = reconstructed_total.saturating_add(new_amount - old_amount);
    }

    let summary = client.get_audit_summary(&issuer, &ns, &token).unwrap();
    assert_eq!(
        reconstructed_total, summary.total_revenue,
        "event-reconstructed total_revenue must match AuditSummary"
    );
    assert_eq!(summary.total_revenue, 250);
    assert_eq!(summary.report_count, 3, "report_count must not change on override");
}

/// Overriding with a decreasing amount must still be reconstructible and
/// must correctly reduce total_revenue.
///
/// init=500, override→100: delta = -400, expected total = 100
#[test]
fn override_audit_trail_decreasing_amount_reconstructible() {
    let (env, contract_id, issuer, token, payout_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let period_id: u64 = 1;

    let start_idx = env.events().all().len();

    client.report_revenue(&issuer, &ns, &token, &payout_asset, &500, &period_id, &false);
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &100, &period_id, &true);

    let persisted = client.get_revenue_by_period(&issuer, &ns, &token, &period_id);
    let reconstructed = reconstruct_from_events(&env, start_idx, period_id);

    assert_eq!(reconstructed, persisted);
    assert_eq!(persisted, 100);

    let summary = client.get_audit_summary(&issuer, &ns, &token).unwrap();
    assert_eq!(summary.total_revenue, 100);

    // Verify the single override event carries the correct delta
    let overrides = collect_override_events(&env, start_idx);
    assert_eq!(overrides.len(), 1);
    let (new_amount, pid, old_amount) = overrides[0];
    assert_eq!(new_amount, 100);
    assert_eq!(old_amount, 500);
    assert_eq!(pid, period_id);
}

/// Overriding with the same amount (no-op delta) must still emit rev_ovrd
/// and the reconstructed amount must equal the persisted value.
#[test]
fn override_audit_trail_same_amount_emits_event_and_is_reconstructible() {
    let (env, contract_id, issuer, token, payout_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let period_id: u64 = 1;

    let start_idx = env.events().all().len();

    client.report_revenue(&issuer, &ns, &token, &payout_asset, &300, &period_id, &false);
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &300, &period_id, &true);

    let persisted = client.get_revenue_by_period(&issuer, &ns, &token, &period_id);
    let reconstructed = reconstruct_from_events(&env, start_idx, period_id);

    assert_eq!(reconstructed, persisted);
    assert_eq!(persisted, 300);

    // rev_ovrd must still be emitted even when new == old
    let overrides = collect_override_events(&env, start_idx);
    assert_eq!(overrides.len(), 1, "rev_ovrd must be emitted even for same-amount override");
    let (new_amount, _pid, old_amount) = overrides[0];
    assert_eq!(new_amount, 300);
    assert_eq!(old_amount, 300);

    // AuditSummary must be unchanged (delta = 0)
    let summary = client.get_audit_summary(&issuer, &ns, &token).unwrap();
    assert_eq!(summary.total_revenue, 300);
    assert_eq!(summary.report_count, 1);
}

/// Saturating override: init near i128::MAX, then override to i128::MAX.
/// The reconstructed amount must equal the persisted value and not overflow.
#[test]
fn override_audit_trail_saturating_override_reconstructible() {
    let (env, contract_id, issuer, token, payout_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let period_id: u64 = 1;

    let start_idx = env.events().all().len();

    let near_max: i128 = i128::MAX - 1;
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &near_max, &period_id, &false);
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &i128::MAX, &period_id, &true);

    let persisted = client.get_revenue_by_period(&issuer, &ns, &token, &period_id);
    let reconstructed = reconstruct_from_events(&env, start_idx, period_id);

    assert_eq!(reconstructed, persisted);
    assert_eq!(persisted, i128::MAX);

    let overrides = collect_override_events(&env, start_idx);
    assert_eq!(overrides.len(), 1);
    let (new_amount, _pid, old_amount) = overrides[0];
    assert_eq!(new_amount, i128::MAX);
    assert_eq!(old_amount, near_max);
}

/// rev_ovrd events for different periods must not interfere with each other's
/// reconstruction.  Two periods each overridden independently must both
/// reconstruct correctly.
#[test]
fn override_audit_trail_independent_periods_do_not_interfere() {
    let (env, contract_id, issuer, token, payout_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    let start_idx = env.events().all().len();

    client.report_revenue(&issuer, &ns, &token, &payout_asset, &100, &1, &false);
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &200, &2, &false);
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &999, &1, &true);
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &777, &2, &true);

    for period_id in [1u64, 2u64] {
        let persisted = client.get_revenue_by_period(&issuer, &ns, &token, &period_id);
        let reconstructed = reconstruct_from_events(&env, start_idx, period_id);
        assert_eq!(
            reconstructed, persisted,
            "period {period_id}: reconstructed must equal persisted"
        );
    }

    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 999);
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &2), 777);
}

/// rev_ovra (asset-tagged override) event must carry the same (new_amount,
/// old_amount, period_id) as rev_ovrd, plus the payout_asset address.
#[test]
fn override_audit_trail_rev_ovra_payload_matches_rev_ovrd() {
    let (env, contract_id, issuer, token, payout_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");
    let period_id: u64 = 1;

    client.report_revenue(&issuer, &ns, &token, &payout_asset, &100, &period_id, &false);

    let before = env.events().all().len();
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &400, &period_id, &true);

    let rev_ovrd_sym: Symbol = symbol_short!("rev_ovrd");
    let rev_ovra_sym: Symbol = symbol_short!("rev_ovra");
    let all = env.events().all();

    let mut ovrd_payload: Option<(i128, u64, i128)> = None;
    let mut ovra_payload: Option<(Address, i128, u64, i128)> = None;

    for i in before..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        let topics_vec: SdkVec<Val> = topics.into_val(&env);
        let sym: Symbol = topics_vec.get(0).unwrap().into_val(&env);
        let data_vec: SdkVec<Val> = data.into_val(&env);

        if sym == rev_ovrd_sym {
            let new_amount: i128 = data_vec.get(0).unwrap().into_val(&env);
            let pid: u64 = data_vec.get(1).unwrap().into_val(&env);
            let old_amount: i128 = data_vec.get(2).unwrap().into_val(&env);
            ovrd_payload = Some((new_amount, pid, old_amount));
        } else if sym == rev_ovra_sym {
            let asset: Address = data_vec.get(0).unwrap().into_val(&env);
            let new_amount: i128 = data_vec.get(1).unwrap().into_val(&env);
            let pid: u64 = data_vec.get(2).unwrap().into_val(&env);
            let old_amount: i128 = data_vec.get(3).unwrap().into_val(&env);
            ovra_payload = Some((asset, new_amount, pid, old_amount));
        }
    }

    let (ovrd_new, ovrd_pid, ovrd_old) = ovrd_payload.expect("rev_ovrd must be emitted");
    let (asset, ovra_new, ovra_pid, ovra_old) = ovra_payload.expect("rev_ovra must be emitted");

    assert_eq!(ovrd_new, ovra_new, "new_amount must match between rev_ovrd and rev_ovra");
    assert_eq!(ovrd_old, ovra_old, "old_amount must match between rev_ovrd and rev_ovra");
    assert_eq!(ovrd_pid, ovra_pid, "period_id must match between rev_ovrd and rev_ovra");
    assert_eq!(asset, payout_asset, "rev_ovra must carry the correct payout_asset");
    assert_eq!(ovrd_new, 400);
    assert_eq!(ovrd_old, 100);
    assert_eq!(ovrd_pid, period_id);
}

/// report_count must never increase on override; only rev_init events
/// contribute to the count.  Verified across N overrides.
#[test]
fn override_audit_trail_report_count_unchanged_across_overrides() {
    let (env, contract_id, issuer, token, payout_asset) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = symbol_short!("def");

    // Two initial periods
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &100, &1, &false);
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &200, &2, &false);

    let count_before = client.get_audit_summary(&issuer, &ns, &token).unwrap().report_count;
    assert_eq!(count_before, 2);

    // Five overrides across both periods
    for amt in [50i128, 75, 90, 110, 130] {
        client.report_revenue(&issuer, &ns, &token, &payout_asset, &amt, &1, &true);
    }
    client.report_revenue(&issuer, &ns, &token, &payout_asset, &999, &2, &true);

    let summary = client.get_audit_summary(&issuer, &ns, &token).unwrap();
    assert_eq!(
        summary.report_count, count_before,
        "report_count must not change after overrides"
    );
    assert_eq!(summary.report_count, 2);
}
