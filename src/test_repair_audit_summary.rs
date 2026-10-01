//! Adversarial coverage for `repair_audit_summary` (#1084).
//!
//! `repair_audit_summary` recomputes an offering's `AuditSummary` cache from the
//! authoritative `RevenueReports` map and overwrites the stored value. It is the
//! recovery path for drift detected by `reconcile_audit_summary`.
//!
//! Coverage matrix
//!
//! | Scenario                                             | Expected                                             |
//! |------------------------------------------------------|------------------------------------------------------|
//! | Unknown (issuer, namespace, token)                   | `OfferingNotFound`, no summary written               |
//! | Unrelated caller                                     | `NotAuthorized`, stored summary unchanged            |
//! | No reports ever filed                                | `{total_revenue: 0, report_count: 0}`                |
//! | Forced cache drift                                   | corrected to the recomputed totals                   |
//! | Repeat call (idempotency)                            | identical result, reconciliation still consistent    |
//! | Contract admin as caller                             | permitted                                            |
//! | Successful repair                                    | emits at least one event                             |

#![cfg(test)]

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
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
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0u32,
    );

    (env, client, issuer, token, payout_asset)
}

#[test]
fn repair_unknown_offering_returns_offering_not_found() {
    let (env, client, issuer, _token, _payout_asset) = setup_offering();
    let unknown_token = Address::generate(&env);

    let result =
        client.try_repair_audit_summary(&issuer, &issuer, &symbol_short!("def"), &unknown_token);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert!(client.get_audit_summary(&issuer, &symbol_short!("def"), &unknown_token).is_none());
}

#[test]
fn repair_by_unrelated_caller_is_rejected_and_leaves_state_unchanged() {
    let (env, client, issuer, token, payout_asset) = setup_offering();

    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100, &1, &false);
    let before = client.get_audit_summary(&issuer, &symbol_short!("def"), &token).unwrap();

    let stranger = Address::generate(&env);
    let result = client.try_repair_audit_summary(&stranger, &issuer, &symbol_short!("def"), &token);
    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));

    let after = client.get_audit_summary(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(after, before, "a rejected repair must not touch the stored summary");
    assert_eq!(after.total_revenue, 100);
    assert_eq!(after.report_count, 1);
}

#[test]
fn repair_without_reports_writes_a_zeroed_summary() {
    let (_env, client, issuer, token, _payout_asset) = setup_offering();

    let repaired =
        client.repair_audit_summary(&issuer, &issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(repaired.total_revenue, 0);
    assert_eq!(repaired.report_count, 0);

    let stored = client.get_audit_summary(&issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(stored, repaired);
}

#[test]
fn repair_corrects_forced_drift_and_restores_reconciliation() {
    let (env, client, issuer, token, payout_asset) = setup_offering();

    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100, &1, &false);
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &250, &2, &false);

    // Force the cached summary out of sync with the authoritative reports.
    let offering_id = OfferingId {
        issuer: issuer.clone(),
        namespace: symbol_short!("def"),
        token: token.clone(),
    };
    env.storage().persistent().set(
        &DataKey::AuditSummary(offering_id),
        &AuditSummary { total_revenue: 9_999, report_count: 42 },
    );

    let drifted = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    assert!(!drifted.is_consistent, "forced drift must be observable");

    let repaired =
        client.repair_audit_summary(&issuer, &issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(repaired.total_revenue, 350);
    assert_eq!(repaired.report_count, 2);

    let reconciled = client.reconcile_audit_summary(&issuer, &symbol_short!("def"), &token);
    assert!(reconciled.is_consistent);
    assert_eq!(reconciled.stored_total_revenue, 350);
    assert_eq!(reconciled.stored_report_count, 2);
}

#[test]
fn repair_is_idempotent_for_the_issuer() {
    let (_env, client, issuer, token, payout_asset) = setup_offering();

    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100, &1, &false);

    let first =
        client.repair_audit_summary(&issuer, &issuer, &symbol_short!("def"), &token).unwrap();
    let second =
        client.repair_audit_summary(&issuer, &issuer, &symbol_short!("def"), &token).unwrap();

    assert_eq!(first, second);
    assert_eq!(second.total_revenue, 100);
    assert_eq!(second.report_count, 1);
}

#[test]
fn repair_is_permitted_for_the_contract_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0u32,
    );
    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &500, &1, &false);

    let repaired =
        client.repair_audit_summary(&admin, &issuer, &symbol_short!("def"), &token).unwrap();
    assert_eq!(repaired.total_revenue, 500);
    assert_eq!(repaired.report_count, 1);
}

#[test]
fn repair_emits_at_least_one_event_on_success() {
    let (env, client, issuer, token, payout_asset) = setup_offering();

    client.report_revenue(&issuer, &symbol_short!("def"), &token, &payout_asset, &100, &1, &false);

    let before = env.events().all().len();
    client.repair_audit_summary(&issuer, &issuer, &symbol_short!("def"), &token).unwrap();
    assert!(env.events().all().len() > before);
}
