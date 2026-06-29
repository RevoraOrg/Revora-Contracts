#![cfg(test)]

extern crate alloc;

use super::*;
use alloc::vec::Vec as RustVec;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val, Vec as SdkVec,
};

fn setup_offering() -> (Env, Address, Address, Address, Address) {
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

fn event_symbols_since(env: &Env, start: u32) -> RustVec<Symbol> {
    let events = env.events().all();
    let mut symbols = RustVec::new();
    for i in start..events.len() {
        let (_, topics, _) = events.get(i).unwrap();
        let topics_vec: SdkVec<Val> = topics.clone().into_val(env);
        let symbol: Symbol = topics_vec.get(0).unwrap().into_val(env);
        symbols.push(symbol);
    }
    symbols
}

fn audit_summary(
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    token: &Address,
) -> AuditSummary {
    client.get_audit_summary(issuer, &symbol_short!("def"), token).unwrap()
}

fn default_offering_id(issuer: &Address, token: &Address) -> OfferingId {
    OfferingId {
        issuer: issuer.clone(),
        namespace: symbol_short!("def"),
        token: token.clone(),
    }
}

#[test]
fn duplicate_report_without_override_emits_rejection_and_preserves_audit_summary() {
    let (env, contract_id, issuer, token, payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100, &1, &false);

    let before = env.events().all().len();
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &250, &1, &false);

    let symbols = event_symbols_since(&env, before);
    assert!(symbols.contains(&symbol_short!("rev_rej")));
    assert!(symbols.contains(&symbol_short!("rev_reja")));
    assert!(!symbols.contains(&symbol_short!("rev_rep")));
    assert_eq!(audit_summary(&client, &issuer, &token).total_revenue, 100);
    assert_eq!(audit_summary(&client, &issuer, &token).report_count, 1);
    assert_eq!(client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1), 100);
}

#[test]
fn override_report_updates_period_and_applies_net_audit_delta() {
    let (env, contract_id, issuer, token, payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100, &1, &false);
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &60, &2, &false);

    let before = env.events().all().len();
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &40, &1, &true);

    let symbols = event_symbols_since(&env, before);
    assert!(symbols.contains(&symbol_short!("rev_ovrd")));
    assert!(symbols.contains(&symbol_short!("rev_ovra")));
    assert!(symbols.contains(&symbol_short!("rev_rep")));
    assert_eq!(audit_summary(&client, &issuer, &token).total_revenue, 100);
    assert_eq!(audit_summary(&client, &issuer, &token).report_count, 2);
    assert_eq!(client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1), 40);

    let reconciliation = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    assert!(reconciliation.is_consistent);
    assert_eq!(reconciliation.computed_total_revenue, 100);
    assert_eq!(reconciliation.computed_report_count, 2);
}

#[test]
fn reconcile_tracks_multiple_overrides_and_detects_forced_summary_drift() {
    let (env, contract_id, issuer, token, payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100, &1, &false);
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &60, &2, &false);
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &40, &3, &false);

    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &150, &1, &true);
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &10, &2, &true);
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &40, &3, &true);

    let summary = audit_summary(&client, &issuer, &token);
    assert_eq!(summary.total_revenue, 200);
    assert_eq!(summary.report_count, 3);
    assert_eq!(
        client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1),
        150
    );
    assert_eq!(
        client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &2),
        10
    );
    assert_eq!(
        client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &3),
        40
    );

    let reconciliation = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    assert!(reconciliation.is_consistent);
    assert!(!reconciliation.is_saturated);
    assert_eq!(reconciliation.stored_total_revenue, 200);
    assert_eq!(reconciliation.stored_report_count, 3);
    assert_eq!(reconciliation.computed_total_revenue, 200);
    assert_eq!(reconciliation.computed_report_count, 3);

    let offering_id = default_offering_id(&issuer, &token);
    env.storage().persistent().set(
        &DataKey::AuditSummary(offering_id),
        &AuditSummary {
            total_revenue: 201,
            report_count: 4,
        },
    );

    let drifted = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    assert!(!drifted.is_consistent);
    assert!(!drifted.is_saturated);
    assert_eq!(drifted.stored_total_revenue, 201);
    assert_eq!(drifted.stored_report_count, 4);
    assert_eq!(drifted.computed_total_revenue, 200);
    assert_eq!(drifted.computed_report_count, 3);
}

#[test]
fn reconcile_marks_saturated_total_revenue() {
    let (env, contract_id, issuer, token, payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &(i128::MAX - 5),
        &1,
        &false,
    );
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &10, &2, &false);

    let summary = audit_summary(&client, &issuer, &token);
    assert_eq!(summary.total_revenue, i128::MAX);
    assert_eq!(summary.report_count, 2);

    let reconciliation = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    assert_eq!(reconciliation.stored_total_revenue, i128::MAX);
    assert_eq!(reconciliation.computed_total_revenue, i128::MAX);
    assert_eq!(reconciliation.stored_report_count, 2);
    assert_eq!(reconciliation.computed_report_count, 2);
    assert!(reconciliation.is_saturated);
    assert!(!reconciliation.is_consistent);
}

#[test]
fn below_threshold_report_is_no_op_and_threshold_toggle_allows_same_period_later() {
    let (env, contract_id, issuer, token, payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &1_000);

    let before = env.events().all().len();
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &999, &1, &false);

    let below_symbols = event_symbols_since(&env, before);
    assert!(below_symbols.contains(&symbol_short!("rev_below")));
    assert!(!below_symbols.contains(&symbol_short!("rev_init")));
    assert!(!below_symbols.contains(&symbol_short!("rev_rep")));
    assert!(client.get_audit_summary(&issuer, &symbol_short!("def"), &token).is_none());
    assert_eq!(client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1), 0);

    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &0);
    let before_accept = env.events().all().len();
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &999, &1, &false);

    let accepted_symbols = event_symbols_since(&env, before_accept);
    assert!(accepted_symbols.contains(&symbol_short!("rev_init")));
    assert!(accepted_symbols.contains(&symbol_short!("rev_rep")));
    assert_eq!(audit_summary(&client, &issuer, &token).total_revenue, 999);
    assert_eq!(audit_summary(&client, &issuer, &token).report_count, 1);
    assert_eq!(client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1), 999);
}

#[test]
fn override_can_correct_below_current_threshold_without_emitting_rev_below() {
    let (env, contract_id, issuer, token, payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &1_500,
        &1,
        &false,
    );
    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &2_000);

    let before = env.events().all().len();
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &500, &1, &true);

    let symbols = event_symbols_since(&env, before);
    assert!(symbols.contains(&symbol_short!("rev_ovrd")));
    assert!(symbols.contains(&symbol_short!("rev_rep")));
    assert!(!symbols.contains(&symbol_short!("rev_below")));
    assert_eq!(audit_summary(&client, &issuer, &token).total_revenue, 500);
    assert_eq!(audit_summary(&client, &issuer, &token).report_count, 1);
    assert_eq!(client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1), 500);
}
