#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, events::Events};
use crate::{RevoraRevenueShare, RevoraRevenueShareClient};

// ── helper ────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

// ── original smoke test ───────────────────────────────────────

#[test]
fn it_emits_events_on_register_and_report() {
    let env = Env::default();
    env.mock_all_auths();
    let client  = make_client(&env);
    let issuer  = Address::generate(&env);
    let token   = Address::generate(&env);

    client.register_offering(&issuer, &token, &1_000);
    client.report_revenue(&issuer, &token, &1_000_000, &1, &false);

    assert!(env.events().all().len() >= 2);
}

// ── blacklist CRUD ────────────────────────────────────────────

#[test]
fn add_marks_investor_as_blacklisted() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    assert!(!client.is_blacklisted(&token, &investor));
    client.blacklist_add(&admin, &token, &investor);
    assert!(client.is_blacklisted(&token, &investor));
}

#[test]
fn remove_unmarks_investor() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    client.blacklist_remove(&admin, &token, &investor);
    assert!(!client.is_blacklisted(&token, &investor));
}

#[test]
fn get_blacklist_returns_all_blocked_investors() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin  = Address::generate(&env);
    let token  = Address::generate(&env);
    let inv_a  = Address::generate(&env);
    let inv_b  = Address::generate(&env);
    let inv_c  = Address::generate(&env);

    client.blacklist_add(&admin, &token, &inv_a);
    client.blacklist_add(&admin, &token, &inv_b);
    client.blacklist_add(&admin, &token, &inv_c);

    let list = client.get_blacklist(&token);
    assert_eq!(list.len(), 3);
    assert!(list.contains(&inv_a));
    assert!(list.contains(&inv_b));
    assert!(list.contains(&inv_c));
}

#[test]
fn get_blacklist_empty_before_any_add() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let token  = Address::generate(&env);

    assert_eq!(client.get_blacklist(&token).len(), 0);
}

// ── idempotency ───────────────────────────────────────────────

#[test]
fn double_add_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    client.blacklist_add(&admin, &token, &investor);

    assert_eq!(client.get_blacklist(&token).len(), 1);
}

#[test]
fn remove_nonexistent_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_remove(&admin, &token, &investor); // must not panic
    assert!(!client.is_blacklisted(&token, &investor));
}

// ── per-offering isolation ────────────────────────────────────

#[test]
fn blacklist_is_scoped_per_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token_a  = Address::generate(&env);
    let token_b  = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token_a, &investor);

    assert!( client.is_blacklisted(&token_a, &investor));
    assert!(!client.is_blacklisted(&token_b, &investor));
}

#[test]
fn removing_from_one_offering_does_not_affect_another() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token_a  = Address::generate(&env);
    let token_b  = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token_a, &investor);
    client.blacklist_add(&admin, &token_b, &investor);
    client.blacklist_remove(&admin, &token_a, &investor);

    assert!(!client.is_blacklisted(&token_a, &investor));
    assert!( client.is_blacklisted(&token_b, &investor));
}

// ── event emission ────────────────────────────────────────────

#[test]
fn blacklist_add_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    let before = env.events().all().len();
    client.blacklist_add(&admin, &token, &investor);
    assert!(env.events().all().len() > before);
}

#[test]
fn blacklist_remove_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    let before = env.events().all().len();
    client.blacklist_remove(&admin, &token, &investor);
    assert!(env.events().all().len() > before);
}

// ── distribution enforcement ──────────────────────────────────

#[test]
fn blacklisted_investor_excluded_from_distribution_filter() {
    let env = Env::default();
    env.mock_all_auths();
    let client  = make_client(&env);
    let admin   = Address::generate(&env);
    let token   = Address::generate(&env);
    let allowed = Address::generate(&env);
    let blocked = Address::generate(&env);

    client.blacklist_add(&admin, &token, &blocked);

    let investors = [allowed.clone(), blocked.clone()];
    let eligible = investors
        .iter()
        .filter(|inv| !client.is_blacklisted(&token, inv))
        .count();

    assert_eq!(eligible, 1);
}

#[test]
fn blacklist_takes_precedence_over_whitelist() {
    let env = Env::default();
    env.mock_all_auths();
    let client   = make_client(&env);
    let admin    = Address::generate(&env);
    let token    = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);

    // Even if investor were on a whitelist, blacklist must win
    assert!(client.is_blacklisted(&token, &investor));
}

// ── auth enforcement ──────────────────────────────────────────

#[test]
#[should_panic]
fn blacklist_add_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client    = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token     = Address::generate(&env);
    let victim    = Address::generate(&env);

    client.blacklist_add(&bad_actor, &token, &victim);
}

#[test]
#[should_panic]
fn blacklist_remove_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client    = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token     = Address::generate(&env);
    let investor  = Address::generate(&env);

    client.blacklist_remove(&bad_actor, &token, &investor);
}

// ── Revenue Report Idempotency Tests ───────────────────────────

#[test]
fn initial_revenue_report_emits_initial_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.report_revenue(&issuer, &token, &1_000_000, &1, &false);

    let events = env.events().all();
    let initial_events: Vec<_> = events.iter()
        .filter(|e| e.topics[0] == soroban_sdk::symbol_short!("rev_ini"))
        .collect();
    
    assert_eq!(initial_events.len(), 1);
}

#[test]
fn duplicate_revenue_report_without_override_emits_rejection_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // First report
    client.report_revenue(&issuer, &token, &1_000_000, &1, &false);
    
    // Duplicate report without override
    client.report_revenue(&issuer, &token, &2_000_000, &1, &false);

    let events = env.events().all();
    let rejection_events: Vec<_> = events.iter()
        .filter(|e| e.topics[0] == soroban_sdk::symbol_short!("rev_rej"))
        .collect();
    
    assert_eq!(rejection_events.len(), 1);
}

#[test]
fn duplicate_revenue_report_with_override_emits_override_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // First report
    client.report_revenue(&issuer, &token, &1_000_000, &1, &false);
    
    // Duplicate report with override
    client.report_revenue(&issuer, &token, &2_000_000, &1, &true);

    let events = env.events().all();
    let override_events: Vec<_> = events.iter()
        .filter(|e| e.topics[0] == soroban_sdk::symbol_short!("rev_ovr"))
        .collect();
    
    assert_eq!(override_events.len(), 1);
}

#[test]
fn has_revenue_report_returns_correct_status() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // Before any report
    assert!(!client.has_revenue_report(&issuer, &token, &1));

    // After first report
    client.report_revenue(&issuer, &token, &1_000_000, &1, &false);
    assert!(client.has_revenue_report(&issuer, &token, &1));

    // Different period should still be false
    assert!(!client.has_revenue_report(&issuer, &token, &2));
}

#[test]
fn get_revenue_report_returns_correct_data() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // Before any report
    assert_eq!(client.get_revenue_report(&issuer, &token, &1), None);

    // After first report
    client.report_revenue(&issuer, &token, &1_000_000, &1, &false);
    let report = client.get_revenue_report(&issuer, &token, &1);
    assert!(report.is_some());
    let (amount, timestamp) = report.unwrap();
    assert_eq!(amount, 1_000_000);
    assert!(timestamp > 0);
}

#[test]
fn get_revenue_report_history_returns_all_reports() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // Add multiple reports
    client.report_revenue(&issuer, &token, &1_000_000, &1, &false);
    client.report_revenue(&issuer, &token, &2_000_000, &2, &false);
    client.report_revenue(&issuer, &token, &1_500_000, &3, &false);

    let history = client.get_revenue_report_history(&issuer, &token);
    assert_eq!(history.len(), 3);
    
    // Check that all periods are present
    let periods: Vec<u64> = history.iter().map(|(period, _, _)| *period).collect();
    assert!(periods.contains(&1));
    assert!(periods.contains(&2));
    assert!(periods.contains(&3));
}

#[test]
fn override_updates_stored_report() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // Initial report
    client.report_revenue(&issuer, &token, &1_000_000, &1, &false);
    let initial_report = client.get_revenue_report(&issuer, &token, &1);
    assert_eq!(initial_report.unwrap().0, 1_000_000);

    // Override with different amount
    client.report_revenue(&issuer, &token, &2_500_000, &1, &true);
    let updated_report = client.get_revenue_report(&issuer, &token, &1);
    assert_eq!(updated_report.unwrap().0, 2_500_000);
}

#[test]
fn revenue_reports_are_isolated_per_issuer_token_pair() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token = Address::generate(&env);

    // Report for issuer A
    client.report_revenue(&issuer_a, &token, &1_000_000, &1, &false);
    
    // Check that issuer B doesn't have the report
    assert!(!client.has_revenue_report(&issuer_b, &token, &1));
    
    // Report for issuer B
    client.report_revenue(&issuer_b, &token, &2_000_000, &1, &false);
    
    // Both should have reports now
    assert!(client.has_revenue_report(&issuer_a, &token, &1));
    assert!(client.has_revenue_report(&issuer_b, &token, &1));
    
    // But with different amounts
    let report_a = client.get_revenue_report(&issuer_a, &token, &1);
    let report_b = client.get_revenue_report(&issuer_b, &token, &1);
    assert_eq!(report_a.unwrap().0, 1_000_000);
    assert_eq!(report_b.unwrap().0, 2_000_000);
}

#[test]
fn revenue_reports_are_isolated_per_token() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Report for token A
    client.report_revenue(&issuer, &token_a, &1_000_000, &1, &false);
    
    // Check that token B doesn't have the report
    assert!(!client.has_revenue_report(&issuer, &token_b, &1));
    
    // Report for token B
    client.report_revenue(&issuer, &token_b, &2_000_000, &1, &false);
    
    // Both should have reports now
    assert!(client.has_revenue_report(&issuer, &token_a, &1));
    assert!(client.has_revenue_report(&issuer, &token_b, &1));
}

#[test]
fn large_period_id_values_work_correctly() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let large_period_id = u64::MAX;
    
    // Should work with very large period IDs
    client.report_revenue(&issuer, &token, &1_000_000, &large_period_id, &false);
    
    assert!(client.has_revenue_report(&issuer, &token, &large_period_id));
    
    let report = client.get_revenue_report(&issuer, &token, &large_period_id);
    assert!(report.is_some());
    assert_eq!(report.unwrap().0, 1_000_000);
}

#[test]
fn multiple_periods_for_same_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // Report for multiple periods
    for period in 1..=5 {
        client.report_revenue(&issuer, &token, &(period as i128 * 1_000_000), &period, &false);
    }

    // Check all periods exist
    for period in 1..=5 {
        assert!(client.has_revenue_report(&issuer, &token, &period));
    }

    // Check history
    let history = client.get_revenue_report_history(&issuer, &token);
    assert_eq!(history.len(), 5);
}

#[test]
fn concurrent_like_submissions_handled_correctly() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    // Simulate multiple submissions for the same period
    client.report_revenue(&issuer, &token, &1_000_000, &1, &false); // First - should be accepted
    client.report_revenue(&issuer, &token, &1_100_000, &1, &false); // Duplicate - should be rejected
    client.report_revenue(&issuer, &token, &1_200_000, &1, &true);  // Override - should be accepted
    client.report_revenue(&issuer, &token, &1_300_000, &1, &false); // Duplicate - should be rejected

    // Final amount should be from the override
    let final_report = client.get_revenue_report(&issuer, &token, &1);
    assert_eq!(final_report.unwrap().0, 1_200_000);

    // Check event counts
    let events = env.events().all();
    let initial_events: Vec<_> = events.iter()
        .filter(|e| e.topics[0] == soroban_sdk::symbol_short!("rev_ini"))
        .collect();
    let rejection_events: Vec<_> = events.iter()
        .filter(|e| e.topics[0] == soroban_sdk::symbol_short!("rev_rej"))
        .collect();
    let override_events: Vec<_> = events.iter()
        .filter(|e| e.topics[0] == soroban_sdk::symbol_short!("rev_ovr"))
        .collect();

    assert_eq!(initial_events.len(), 1);
    assert_eq!(rejection_events.len(), 2);
    assert_eq!(override_events.len(), 1);
}

#[test]
#[should_panic]
fn revenue_report_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.report_revenue(&issuer, &token, &1_000_000, &1, &false);
}

#[test]
fn zero_amount_revenue_report_works() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.report_revenue(&issuer, &token, &0, &1, &false);
    
    assert!(client.has_revenue_report(&issuer, &token, &1));
    
    let report = client.get_revenue_report(&issuer, &token, &1);
    assert_eq!(report.unwrap().0, 0);
}

#[test]
fn negative_amount_revenue_report_works() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.report_revenue(&issuer, &token, &-500_000, &1, &false);
    
    assert!(client.has_revenue_report(&issuer, &token, &1));
    
    let report = client.get_revenue_report(&issuer, &token, &1);
    assert_eq!(report.unwrap().0, -500_000);
}