extern crate alloc;

use super::*;
use alloc::vec::Vec as RustVec;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Val,
};

fn setup_test() -> (Env, RevoraRevenueShareClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None, &None);

    (env, client, admin)
}

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let (env, client, issuer) = setup_test();
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let namespace = symbol_short!("ns");

    client.register_offering(&issuer, &namespace, &token, &5000, &payout_asset, &0);

    (env, client, issuer, token, payout_asset)
}

fn event_symbols_since(env: &Env, start: u32) -> RustVec<Symbol> {
    let events = env.events().all();
    let mut symbols = RustVec::new();
    for i in start..events.len() {
        let (_, topics, _) = events.get(i).unwrap();
        let topics_vec: soroban_sdk::Vec<Val> = topics.clone().into_val(env);
        let symbol: Symbol = topics_vec.get(0).unwrap().into_val(env);
        symbols.push(symbol);
    }
    symbols
}

#[test]
fn test_register_duplicate_offering_is_idempotent() {
    let (env, client, issuer) = setup_test();
    let token = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let payout_asset = Address::generate(&env);

    // First registration
    client.register_offering(&issuer, &namespace, &token, &5000, &payout_asset, &0);
    assert_eq!(client.get_offering_count(&issuer, &namespace), 1);

    let offering1 = client.get_offering(&issuer, &namespace, &token).unwrap();
    assert_eq!(offering1.revenue_share_bps, 5000);

    // Second registration (same identity, different bps)
    client.register_offering(&issuer, &namespace, &token, &6000, &payout_asset, &0);

    // Count should still be 1
    assert_eq!(client.get_offering_count(&issuer, &namespace), 1);

    // Offering parameters should NOT have changed (preserving original)
    let offering2 = client.get_offering(&issuer, &namespace, &token).unwrap();
    assert_eq!(offering2.revenue_share_bps, 5000);
}

#[test]
fn test_pagination_stability_with_idempotency() {
    let (env, client, issuer) = setup_test();
    let namespace = symbol_short!("ns");
    let payout_asset = Address::generate(&env);

    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Register A, then B, then A again
    client.register_offering(&issuer, &namespace, &token_a, &1000, &payout_asset, &0);
    client.register_offering(&issuer, &namespace, &token_b, &2000, &payout_asset, &0);
    client.register_offering(&issuer, &namespace, &token_a, &3000, &payout_asset, &0);

    let (offerings, _) = client.get_offerings_page(&issuer, &namespace, &0, &10);

    // Should only have 2 unique offerings in the order they were first registered
    assert_eq!(offerings.len(), 2);
    assert_eq!(offerings.get(0).unwrap().token, token_a);
    assert_eq!(offerings.get(1).unwrap().token, token_b);
}

#[test]
fn test_get_offering_matches_first_registration() {
    let (env, client, issuer) = setup_test();
    let token = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let payout_asset = Address::generate(&env);

    client.register_offering(&issuer, &namespace, &token, &1000, &payout_asset, &0);
    client.register_offering(&issuer, &namespace, &token, &2000, &payout_asset, &0);

    let offering = client.get_offering(&issuer, &namespace, &token).unwrap();
    assert_eq!(offering.revenue_share_bps, 1000);
}

#[test]
fn test_duplicate_report_revenue_rejects_with_rev_rej_and_preserves_state() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let namespace = symbol_short!("ns");

    // Initial report: period 1, amount 100
    client.report_revenue(&issuer, &namespace, &token, &payout_asset, &100, &1, &false);

    let audit = client.get_audit_summary(&issuer, &namespace, &token).unwrap();
    assert_eq!(audit.total_revenue, 100);
    assert_eq!(audit.report_count, 1);
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 100);

    // Edge case 1: duplicate with larger amount
    let before = env.events().all().len();
    client.report_revenue(&issuer, &namespace, &token, &payout_asset, &250, &1, &false);

    let symbols = event_symbols_since(&env, before);
    assert!(symbols.contains(&symbol_short!("rev_rej")));
    assert!(symbols.contains(&symbol_short!("rev_reja")));
    assert!(!symbols.contains(&symbol_short!("rev_rep")));

    let audit_after = client.get_audit_summary(&issuer, &namespace, &token).unwrap();
    assert_eq!(audit_after.total_revenue, 100);
    assert_eq!(audit_after.report_count, 1);
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 100);

    // Edge case 2: duplicate with smaller amount
    let before2 = env.events().all().len();
    client.report_revenue(&issuer, &namespace, &token, &payout_asset, &50, &1, &false);

    let symbols2 = event_symbols_since(&env, before2);
    assert!(symbols2.contains(&symbol_short!("rev_rej")));
    assert!(!symbols2.contains(&symbol_short!("rev_rep")));

    let audit_after2 = client.get_audit_summary(&issuer, &namespace, &token).unwrap();
    assert_eq!(audit_after2.total_revenue, 100);
    assert_eq!(audit_after2.report_count, 1);

    // Edge case 3: duplicate with identical amount
    let before3 = env.events().all().len();
    client.report_revenue(&issuer, &namespace, &token, &payout_asset, &100, &1, &false);

    let symbols3 = event_symbols_since(&env, before3);
    assert!(symbols3.contains(&symbol_short!("rev_rej")));
    assert!(!symbols3.contains(&symbol_short!("rev_rep")));

    let audit_after3 = client.get_audit_summary(&issuer, &namespace, &token).unwrap();
    assert_eq!(audit_after3.total_revenue, 100);
    assert_eq!(audit_after3.report_count, 1);
    assert_eq!(client.get_revenue_by_period(&issuer, &namespace, &token, &1), 100);
}

#[test]
fn test_duplicate_report_revenue_rev_rej_event_payload() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let namespace = symbol_short!("ns");
    let initial_amount: i128 = 100;
    let duplicate_amount: i128 = 250;

    // Initial report
    client.report_revenue(&issuer, &namespace, &token, &payout_asset, &initial_amount, &1, &false);

    // Duplicate call
    let before = env.events().all().len();
    client.report_revenue(
        &issuer,
        &namespace,
        &token,
        &payout_asset,
        &duplicate_amount,
        &1,
        &false,
    );

    // Find rev_rej event and verify its data payload
    let rev_rej_sym: Symbol = symbol_short!("rev_rej");
    let mut found = false;
    for i in before..env.events().all().len() {
        let (_, topics, data) = env.events().all().get(i).unwrap();
        let topics_vec: soroban_sdk::Vec<Val> = topics.clone().into_val(&env);
        let topic_sym: Symbol = topics_vec.get(0).unwrap().into_val(&env);
        if topic_sym == rev_rej_sym {
            let data_vec: soroban_sdk::Vec<Val> = data.clone().into_val(&env);
            let attempted_amount: i128 = data_vec.get(0).unwrap().into_val(&env);
            let period_id_val: u64 = data_vec.get(1).unwrap().into_val(&env);
            let existing_amount: i128 = data_vec.get(2).unwrap().into_val(&env);
            assert_eq!(attempted_amount, duplicate_amount);
            assert_eq!(period_id_val, 1);
            assert_eq!(existing_amount, initial_amount);
            found = true;
            break;
        }
    }
    assert!(found, "rev_rej event with correct payload must be emitted");

    // Also verify rev_reja (asset) event
    let rev_reja_sym: Symbol = symbol_short!("rev_reja");
    let mut found_asset = false;
    for i in before..env.events().all().len() {
        let (_, topics, data) = env.events().all().get(i).unwrap();
        let topics_vec: soroban_sdk::Vec<Val> = topics.clone().into_val(&env);
        let topic_sym: Symbol = topics_vec.get(0).unwrap().into_val(&env);
        if topic_sym == rev_reja_sym {
            let data_vec: soroban_sdk::Vec<Val> = data.clone().into_val(&env);
            let asset: Address = data_vec.get(0).unwrap().into_val(&env);
            let amount: i128 = data_vec.get(1).unwrap().into_val(&env);
            let pid: u64 = data_vec.get(2).unwrap().into_val(&env);
            let existing: i128 = data_vec.get(3).unwrap().into_val(&env);
            assert_eq!(asset, payout_asset);
            assert_eq!(amount, duplicate_amount);
            assert_eq!(pid, 1);
            assert_eq!(existing, initial_amount);
            found_asset = true;
            break;
        }
    }
    assert!(found_asset, "rev_reja event with correct payload must be emitted");
}

#[test]
fn test_override_missing_report_emits_rev_omiss_and_returns_missing_override() {
    let (env, client, issuer, token, payout_asset) = setup_offering();
    let namespace = symbol_short!("ns");
    let attempted_amount: i128 = 123;
    let period_id: u64 = 1;

    let before = env.events().all().len();
    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &payout_asset,
        &attempted_amount,
        &period_id,
        &true,
    );

    assert_eq!(result, Err(Ok(RevoraError::MissingReportForOverride)));

    let mut found_rev_omiss = false;
    for i in before..env.events().all().len() {
        let (_, topics, data) = env.events().all().get(i).unwrap();
        let topics_vec: soroban_sdk::Vec<Val> = topics.clone().into_val(&env);
        let topic_sym: Symbol = topics_vec.get(0).unwrap().into_val(&env);
        if topic_sym == symbol_short!("rev_omiss") {
            let data_vec: soroban_sdk::Vec<Val> = data.clone().into_val(&env);
            let amount: i128 = data_vec.get(0).unwrap().into_val(&env);
            let pid: u64 = data_vec.get(1).unwrap().into_val(&env);
            assert_eq!(amount, attempted_amount);
            assert_eq!(pid, period_id);
            found_rev_omiss = true;
            break;
        }
    }
    assert!(found_rev_omiss, "rev_omiss event with correct payload must be emitted");
}
