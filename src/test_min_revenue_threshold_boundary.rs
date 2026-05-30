//! Boundary tests for `set_min_revenue_threshold` / `report_revenue` interaction.
//!
//! Test matrix (issue #366):
//! | Case                                  | Expected outcome                          |
//! |---------------------------------------|-------------------------------------------|
//! | threshold == 0 (default)              | every amount accepted                     |
//! | amount == threshold - 1               | `rev_below` emitted, no audit update      |
//! | amount == threshold                   | accepted (inclusive boundary)             |
//! | amount == threshold + 1               | accepted                                  |
//! | threshold reset to 0                  | disables check; skipped period accepted   |
//! | override_existing=true below threshold| bypasses threshold; `rev_ovrd` emitted    |

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val, Vec as SdkVec,
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &payout, &0);
    (env, contract_id, issuer, token, payout)
}

fn event_topics_since(env: &Env, start: u32) -> alloc::vec::Vec<Symbol> {
    let events = env.events().all();
    let mut out = alloc::vec::Vec::new();
    for i in start..events.len() {
        let (_, topics, _) = events.get(i).unwrap();
        let v: SdkVec<Val> = topics.clone().into_val(env);
        let sym: Symbol = v.get(0).unwrap().into_val(env);
        out.push(sym);
    }
    out
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// threshold == 0 (default): any non-negative amount is accepted.
#[test]
fn threshold_zero_accepts_any_amount() {
    let (env, contract_id, issuer, token, payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &symbol_short!("def"), &token), 0);
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout, &0, &1, &false);
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout, &1, &2, &false);
    let s = client.get_audit_summary(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(s.report_count, 2);
    assert_eq!(s.total_revenue, 1);
}

/// amount == threshold - 1: `rev_below` emitted, no audit mutation, period stays open.
#[test]
fn amount_one_below_threshold_emits_rev_below_and_skips_audit() {
    let (env, contract_id, issuer, token, payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &1_000);

    let before = env.events().all().len();
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout, &999, &1, &false);

    let topics = event_topics_since(&env, before);
    assert!(topics.contains(&symbol_short!("rev_below")), "must emit rev_below");
    assert!(!topics.contains(&symbol_short!("rev_init")), "must not emit rev_init");
    assert!(!topics.contains(&symbol_short!("rev_rep")), "must not emit rev_rep");
    assert!(
        client.get_audit_summary(&issuer, &symbol_short!("def"), &token).is_none(),
        "audit must not be created"
    );
    assert_eq!(
        client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1),
        0,
        "period must remain unset"
    );
}

/// amount == threshold: accepted (boundary is inclusive), audit updated.
#[test]
fn amount_equal_to_threshold_is_accepted() {
    let (env, contract_id, issuer, token, payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &1_000);

    let before = env.events().all().len();
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout, &1_000, &1, &false);

    let topics = event_topics_since(&env, before);
    assert!(!topics.contains(&symbol_short!("rev_below")), "must NOT emit rev_below at threshold");
    assert!(topics.contains(&symbol_short!("rev_init")), "must emit rev_init");
    let s = client.get_audit_summary(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(s.report_count, 1);
    assert_eq!(s.total_revenue, 1_000);
}

/// amount == threshold + 1: accepted, audit updated.
#[test]
fn amount_one_above_threshold_is_accepted() {
    let (env, contract_id, issuer, token, payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &1_000);
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout, &1_001, &1, &false);
    let s = client.get_audit_summary(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(s.report_count, 1);
    assert_eq!(s.total_revenue, 1_001);
}

/// Resetting threshold to 0 disables the check; a previously-skipped period
/// can then be reported normally.
#[test]
fn reset_threshold_to_zero_allows_previously_skipped_period() {
    let (env, contract_id, issuer, token, payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &1_000);
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout, &500, &1, &false); // skipped
    assert!(client.get_audit_summary(&issuer, &symbol_short!("def"), &token).is_none());

    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &0);
    let before = env.events().all().len();
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout, &500, &1, &false);

    let topics = event_topics_since(&env, before);
    assert!(topics.contains(&symbol_short!("rev_init")));
    let s = client.get_audit_summary(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(s.report_count, 1);
    assert_eq!(s.total_revenue, 500);
}

/// override_existing=true on a persisted period bypasses the threshold check:
/// `rev_ovrd` emitted, audit delta applied, no `rev_below`.
#[test]
fn override_existing_below_threshold_bypasses_check() {
    let (env, contract_id, issuer, token, payout) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    // Persist a period above threshold.
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout, &2_000, &1, &false);
    assert_eq!(
        client.get_audit_summary(&issuer, &symbol_short!("def"), &token).unwrap().total_revenue,
        2_000
    );

    // Raise threshold above the correction amount.
    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &5_000);

    let before = env.events().all().len();
    // Override with amount below threshold — must succeed.
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout, &300, &1, &true);

    let topics = event_topics_since(&env, before);
    assert!(topics.contains(&symbol_short!("rev_ovrd")), "must emit rev_ovrd");
    assert!(topics.contains(&symbol_short!("rev_rep")), "must emit rev_rep");
    assert!(!topics.contains(&symbol_short!("rev_below")), "must NOT emit rev_below for override");

    let s = client.get_audit_summary(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(s.total_revenue, 300, "audit must reflect corrected amount");
    assert_eq!(s.report_count, 1, "report_count must not change on override");
    assert_eq!(
        client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1),
        300
    );
}

/// get_min_revenue_threshold returns the stored value and updates correctly.
#[test]
fn get_min_revenue_threshold_reflects_set_value() {
    let (env, contract_id, issuer, token, _) = setup();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &symbol_short!("def"), &token), 0);
    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &7_500);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &symbol_short!("def"), &token), 7_500);
    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(client.get_min_revenue_threshold(&issuer, &symbol_short!("def"), &token), 0);
}
