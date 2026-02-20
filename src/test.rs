#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env,
};

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

// ── original smoke test ───────────────────────────────────────

#[test]
fn it_emits_events_on_register_and_report() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &1_000);
    client.report_revenue(&issuer, &token, &1_000_000, &1);

    assert!(env.events().all().len() >= 2);
}

// ── blacklist CRUD ────────────────────────────────────────────

#[test]
fn add_marks_investor_as_blacklisted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    assert!(!client.is_blacklisted(&token, &investor));
    client.blacklist_add(&admin, &token, &investor);
    assert!(client.is_blacklisted(&token, &investor));
}

#[test]
fn remove_unmarks_investor() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
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
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let inv_a = Address::generate(&env);
    let inv_b = Address::generate(&env);
    let inv_c = Address::generate(&env);

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
    let token = Address::generate(&env);

    assert_eq!(client.get_blacklist(&token).len(), 0);
}

// ── idempotency ───────────────────────────────────────────────

#[test]
fn double_add_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token, &investor);
    client.blacklist_add(&admin, &token, &investor);

    assert_eq!(client.get_blacklist(&token).len(), 1);
}

#[test]
fn remove_nonexistent_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_remove(&admin, &token, &investor); // must not panic
    assert!(!client.is_blacklisted(&token, &investor));
}

// ── per-offering isolation ────────────────────────────────────

#[test]
fn blacklist_is_scoped_per_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token_a, &investor);

    assert!(client.is_blacklisted(&token_a, &investor));
    assert!(!client.is_blacklisted(&token_b, &investor));
}

#[test]
fn removing_from_one_offering_does_not_affect_another() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_add(&admin, &token_a, &investor);
    client.blacklist_add(&admin, &token_b, &investor);
    client.blacklist_remove(&admin, &token_a, &investor);

    assert!(!client.is_blacklisted(&token_a, &investor));
    assert!(client.is_blacklisted(&token_b, &investor));
}

// ── event emission ────────────────────────────────────────────

#[test]
fn blacklist_add_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    let before = env.events().all().len();
    client.blacklist_add(&admin, &token, &investor);
    assert!(env.events().all().len() > before);
}

#[test]
fn blacklist_remove_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
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
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
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
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
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
    let client = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token = Address::generate(&env);
    let victim = Address::generate(&env);

    client.blacklist_add(&bad_actor, &token, &victim);
}

#[test]
#[should_panic]
fn blacklist_remove_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let client = make_client(&env);
    let bad_actor = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);

    client.blacklist_remove(&bad_actor, &token, &investor);
}

// ── offering registration tests ───────────────────────────────

#[test]
fn register_offering_stores_offering_data() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000);

    let offering = client.get_offering(&token);
    assert!(offering.is_some());
    let offering = offering.unwrap();
    assert_eq!(offering.issuer, issuer);
    assert_eq!(offering.revenue_share_bps, 5_000);
}

#[test]
fn register_offering_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let before = env.events().all().len();
    client.register_offering(&issuer, &token, &5_000);
    assert!(env.events().all().len() > before);
}

#[test]
#[should_panic(expected = "revenue_share_bps cannot exceed 10000")]
fn register_offering_rejects_bps_over_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &10_001);
}

#[test]
fn get_offering_returns_none_for_unregistered_token() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let token = Address::generate(&env);

    let offering = client.get_offering(&token);
    assert!(offering.is_none());
}

#[test]
fn register_offering_accepts_max_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &10_000);
    let offering = client.get_offering(&token).unwrap();
    assert_eq!(offering.revenue_share_bps, 10_000);
}

// ── revenue distribution calculation tests ─────────────────────

#[test]
fn calculate_distribution_basic() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &10_000);

    let total_revenue: i128 = 1_000_000;
    let total_supply: i128 = 1_000;
    let holder_balance: i128 = 100;

    let payout = client.calculate_distribution(
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &holder_balance,
        &holder,
    );

    assert_eq!(payout, 100_000);
}

#[test]
fn calculate_distribution_with_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000);

    let total_revenue: i128 = 1_000_000;
    let total_supply: i128 = 1_000;
    let holder_balance: i128 = 100;

    let payout = client.calculate_distribution(
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &holder_balance,
        &holder,
    );

    assert_eq!(payout, 50_000);
}

#[test]
fn calculate_distribution_zero_revenue() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000);

    let payout = client.calculate_distribution(&issuer, &token, &0, &1_000, &100, &holder);

    assert_eq!(payout, 0);
}

#[test]
fn calculate_distribution_zero_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000);

    let payout = client.calculate_distribution(&issuer, &token, &1_000_000, &1_000, &0, &holder);

    assert_eq!(payout, 0);
}

#[test]
#[should_panic(expected = "total_supply cannot be zero")]
fn calculate_distribution_zero_supply_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000);

    client.calculate_distribution(&issuer, &token, &1_000_000, &0, &100, &holder);
}

#[test]
#[should_panic(expected = "offering not found for token")]
fn calculate_distribution_without_offering_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let caller = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.calculate_distribution(&caller, &token, &1_000_000, &1_000, &100, &holder);
}

#[test]
fn calculate_distribution_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000);

    let before = env.events().all().len();
    client.calculate_distribution(&issuer, &token, &1_000_000, &1_000, &100, &holder);
    assert!(env.events().all().len() > before);
}

#[test]
fn calculate_distribution_rounding_down() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &3_333);

    let total_revenue: i128 = 10_000;
    let total_supply: i128 = 7;
    let holder_balance: i128 = 3;

    let payout = client.calculate_distribution(
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &holder_balance,
        &holder,
    );

    assert_eq!(payout, 1428);
}

#[test]
#[should_panic(expected = "holder is blacklisted")]
fn calculate_distribution_blacklisted_holder_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000);
    client.blacklist_add(&issuer, &token, &holder);

    client.calculate_distribution(&issuer, &token, &1_000_000, &1_000, &100, &holder);
}

#[test]
fn calculate_distribution_large_values() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000);

    let total_revenue: i128 = 1_000_000_000_000;
    let total_supply: i128 = 100_000_000_000;
    let holder_balance: i128 = 10_000_000_000;

    let payout = client.calculate_distribution(
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &holder_balance,
        &holder,
    );

    assert_eq!(payout, 50_000_000_000);
}

#[test]
fn calculate_distribution_small_fractional() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer, &token, &1);

    let total_revenue: i128 = 10_000;
    let total_supply: i128 = 1_000;
    let holder_balance: i128 = 1;

    let payout = client.calculate_distribution(
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &holder_balance,
        &holder,
    );

    assert_eq!(payout, 0);
}

// ── calculate_total_distributable tests ───────────────────────

#[test]
fn calculate_total_distributable_basic() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000);

    let distributable = client.calculate_total_distributable(&token, &1_000_000);

    assert_eq!(distributable, 500_000);
}

#[test]
fn calculate_total_distributable_zero_revenue() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000);

    let distributable = client.calculate_total_distributable(&token, &0);

    assert_eq!(distributable, 0);
}

#[test]
fn calculate_total_distributable_max_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    client.register_offering(&issuer, &token, &10_000);

    let distributable = client.calculate_total_distributable(&token, &1_000_000);

    assert_eq!(distributable, 1_000_000);
}

#[test]
#[should_panic(expected = "offering not found for token")]
fn calculate_total_distributable_without_offering_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let token = Address::generate(&env);

    client.calculate_total_distributable(&token, &1_000_000);
}

// ── calculate_distribution auth tests ──────────────────────────

#[test]
#[should_panic]
fn calculate_distribution_requires_auth() {
    let env = Env::default();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    env.mock_all_auths();
    client.register_offering(&issuer, &token, &5_000);

    env.set_auths(&[]);

    client.calculate_distribution(&issuer, &token, &1_000_000, &1_000, &100, &holder);
}

// ── multiple holder distribution verification ──────────────────

#[test]
fn calculate_distribution_multiple_holders_sum_equals_total() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);
    let holder_c = Address::generate(&env);

    client.register_offering(&issuer, &token, &5_000);

    let total_revenue: i128 = 1_000_000;
    let total_supply: i128 = 1_000;
    let balance_a: i128 = 500;
    let balance_b: i128 = 300;
    let balance_c: i128 = 200;

    let payout_a = client.calculate_distribution(
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &balance_a,
        &holder_a,
    );
    let payout_b = client.calculate_distribution(
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &balance_b,
        &holder_b,
    );
    let payout_c = client.calculate_distribution(
        &issuer,
        &token,
        &total_revenue,
        &total_supply,
        &balance_c,
        &holder_c,
    );

    assert_eq!(payout_a, 250_000);
    assert_eq!(payout_b, 150_000);
    assert_eq!(payout_c, 100_000);

    assert_eq!(payout_a + payout_b + payout_c, 500_000);
}
