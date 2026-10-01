//! Adversarial coverage for `reconcile_audit_summary` (#1085).
//!
//! `reconcile_audit_summary` is a read-only comparison between the stored
//! `AuditSummary` cache and the authoritative `RevenueReports` map. The happy
//! path is covered elsewhere; this suite pins the paths a happy-path test
//! misses:
//!
//! - the empty offering (no reports, no cached summary) is trivially consistent,
//! - reconcile is read-only and must never create the cache it reads,
//! - report-count drift is flagged even when total revenue matches,
//! - the comparison is scoped to the requested offering, and
//! - the recomputation reflects an override correction.

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec as SdkVec};

fn setup_offering() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &SdkVec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    (env, contract_id, issuer, token, payout_asset)
}

fn offering_id(issuer: &Address, token: &Address) -> OfferingId {
    OfferingId {
        issuer: issuer.clone(),
        namespace: symbol_short!("def"),
        token: token.clone(),
    }
}

#[test]
fn reconcile_with_no_reports_and_no_stored_summary_is_trivially_consistent() {
    let (env, contract_id, issuer, token, _payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    assert!(!env
        .storage()
        .persistent()
        .has(&DataKey::AuditSummary(offering_id(&issuer, &token))));

    let reconciliation = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    assert_eq!(reconciliation.stored_total_revenue, 0);
    assert_eq!(reconciliation.stored_report_count, 0);
    assert_eq!(reconciliation.computed_total_revenue, 0);
    assert_eq!(reconciliation.computed_report_count, 0);
    assert!(!reconciliation.is_saturated);
    assert!(reconciliation.is_consistent);
}

#[test]
fn reconcile_is_read_only_and_does_not_materialise_the_summary_cache() {
    let (env, contract_id, issuer, token, _payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let first = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    let second = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);

    assert_eq!(
        first, second,
        "repeated reconcile calls must be deterministic"
    );
    assert!(
        !env.storage()
            .persistent()
            .has(&DataKey::AuditSummary(offering_id(&issuer, &token))),
        "reconcile must not create the AuditSummary cache it reads"
    );
    assert!(client
        .get_audit_summary(&issuer, &symbol_short!("def"), &token)
        .is_none());
}

#[test]
fn reconcile_flags_report_count_drift_even_when_revenue_matches() {
    let (env, contract_id, issuer, token, payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &250,
        &1,
        &false,
    );

    env.storage().persistent().set(
        &DataKey::AuditSummary(offering_id(&issuer, &token)),
        &AuditSummary {
            total_revenue: 250,
            report_count: 9,
        },
    );

    let reconciliation = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    assert!(
        !reconciliation.is_consistent,
        "report_count drift must be detected"
    );
    assert!(!reconciliation.is_saturated);
    assert_eq!(reconciliation.stored_total_revenue, 250);
    assert_eq!(reconciliation.computed_total_revenue, 250);
    assert_eq!(reconciliation.stored_report_count, 9);
    assert_eq!(reconciliation.computed_report_count, 1);
}

#[test]
fn reconcile_is_scoped_to_the_requested_offering() {
    let (env, contract_id, issuer, token, payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let other_token = Address::generate(&env);

    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &100,
        &1,
        &false,
    );

    let other = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &other_token);
    assert_eq!(other.computed_total_revenue, 0);
    assert_eq!(other.computed_report_count, 0);
    assert!(other.is_consistent);

    let target = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    assert_eq!(target.computed_total_revenue, 100);
    assert_eq!(target.computed_report_count, 1);
    assert!(target.is_consistent);
}

#[test]
fn reconcile_recomputes_after_an_override_correction() {
    let (env, contract_id, issuer, token, payout_asset) = setup_offering();
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &500,
        &1,
        &false,
    );
    let before = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    assert_eq!(before.computed_total_revenue, 500);
    assert_eq!(before.computed_report_count, 1);

    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout_asset,
        &125,
        &1,
        &true,
    );
    let after = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    assert_eq!(after.computed_total_revenue, 125);
    assert_eq!(after.computed_report_count, 1);
    assert!(after.is_consistent);
}
