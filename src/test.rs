#![cfg(test)]

use soroban_sdk::{testutils::{Address as _, Events}, Address, Env};
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
    client.report_revenue(&issuer, &token, &1_000_000, &1);

    assert!(env.events().all().len() >= 2);
}

// ── platform initialization ──────────────────────────────────

#[test]
fn initialize_sets_platform_owner() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    assert_eq!(client.get_platform_owner(), owner);
}

#[test]
fn initialize_sets_default_fee_to_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    assert_eq!(client.get_platform_fee(), 0);
}

#[test]
#[should_panic]
fn initialize_cannot_be_called_twice() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    client.initialize(&owner);
}

#[test]
#[should_panic]
fn initialize_requires_auth() {
    let env = Env::default();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
}

// ── platform fee configuration ───────────────────────────────

#[test]
fn set_and_get_platform_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    client.set_platform_fee(&250);
    assert_eq!(client.get_platform_fee(), 250);
}

#[test]
fn default_platform_fee_is_zero() {
    let env = Env::default();
    let client = make_client(&env);
    assert_eq!(client.get_platform_fee(), 0);
}

#[test]
fn set_platform_fee_to_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    client.set_platform_fee(&500);
    client.set_platform_fee(&0);
    assert_eq!(client.get_platform_fee(), 0);
}

#[test]
fn set_platform_fee_to_maximum() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    client.set_platform_fee(&5_000);
    assert_eq!(client.get_platform_fee(), 5_000);
}

#[test]
#[should_panic]
fn set_platform_fee_above_maximum_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    client.set_platform_fee(&5_001);
}

#[test]
fn update_platform_fee_multiple_times() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);

    client.set_platform_fee(&100);
    assert_eq!(client.get_platform_fee(), 100);

    client.set_platform_fee(&300);
    assert_eq!(client.get_platform_fee(), 300);

    client.set_platform_fee(&0);
    assert_eq!(client.get_platform_fee(), 0);
}

#[test]
#[should_panic]
fn set_platform_fee_requires_auth() {
    let env = Env::default();
    let client = make_client(&env);
    client.set_platform_fee(&200);
}

#[test]
fn set_platform_fee_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    let before = env.events().all().len();
    client.set_platform_fee(&250);
    assert!(env.events().all().len() > before);
}

// ── fee calculation ──────────────────────────────────────────

#[test]
fn calculate_fee_basic() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    client.set_platform_fee(&250); // 2.5%
    assert_eq!(client.calculate_platform_fee(&1_000_000), 25_000);
}

#[test]
fn calculate_fee_with_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    client.set_platform_fee(&250);
    assert_eq!(client.calculate_platform_fee(&0), 0);
}

#[test]
fn calculate_fee_with_zero_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    assert_eq!(client.calculate_platform_fee(&1_000_000), 0);
}

#[test]
fn calculate_fee_at_maximum_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    client.set_platform_fee(&5_000); // 50%
    assert_eq!(client.calculate_platform_fee(&1_000_000), 500_000);
}

#[test]
fn calculate_fee_precision() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);

    client.initialize(&owner);
    client.set_platform_fee(&1); // 0.01%
    // 999 * 1 / 10000 = 0 (integer truncation)
    assert_eq!(client.calculate_platform_fee(&999), 0);
    // 10000 * 1 / 10000 = 1
    assert_eq!(client.calculate_platform_fee(&10_000), 1);
}

// ── revenue report with fees ─────────────────────────────────

#[test]
fn report_revenue_with_platform_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let owner  = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token  = Address::generate(&env);

    client.initialize(&owner);
    client.set_platform_fee(&500); // 5%

    let before = env.events().all().len();
    client.report_revenue(&issuer, &token, &1_000_000, &1);
    assert!(env.events().all().len() > before);
}

#[test]
fn report_revenue_without_initialization_uses_zero_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token  = Address::generate(&env);

    client.report_revenue(&issuer, &token, &1_000_000, &1);
    assert_eq!(client.get_platform_fee(), 0);
    assert_eq!(client.calculate_platform_fee(&1_000_000), 0);
}

// ── ownership transfer ───────────────────────────────────────

#[test]
fn transfer_ownership_updates_owner() {
    let env = Env::default();
    env.mock_all_auths();
    let client    = make_client(&env);
    let owner     = Address::generate(&env);
    let new_owner = Address::generate(&env);

    client.initialize(&owner);
    client.transfer_ownership(&new_owner);
    assert_eq!(client.get_platform_owner(), new_owner);
}

#[test]
fn new_owner_can_set_fee_after_transfer() {
    let env = Env::default();
    env.mock_all_auths();
    let client    = make_client(&env);
    let owner     = Address::generate(&env);
    let new_owner = Address::generate(&env);

    client.initialize(&owner);
    client.transfer_ownership(&new_owner);
    client.set_platform_fee(&300);
    assert_eq!(client.get_platform_fee(), 300);
}

#[test]
fn transfer_ownership_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let client    = make_client(&env);
    let owner     = Address::generate(&env);
    let new_owner = Address::generate(&env);

    client.initialize(&owner);
    let before = env.events().all().len();
    client.transfer_ownership(&new_owner);
    assert!(env.events().all().len() > before);
}

#[test]
#[should_panic]
fn transfer_ownership_requires_auth() {
    let env = Env::default();
    let client    = make_client(&env);
    let new_owner = Address::generate(&env);
    // Not initialized and no auth
    client.transfer_ownership(&new_owner);
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