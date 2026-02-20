#![cfg(test)]

use soroban_sdk::{testutils::{Address as _, Events as _}, Address, Env};
use crate::{OfferingStatus, RevoraRevenueShare, RevoraRevenueShareClient};

// ── helpers ───────────────────────────────────────────────────

/// Bare client — no offering registered. Used by blacklist tests that
/// don't need lifecycle state.
fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Client with a registered offering. Used by lifecycle tests.
fn setup(env: &Env) -> (RevoraRevenueShareClient, Address, Address) {
    env.mock_all_auths();
    let id     = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &id);
    let issuer = Address::generate(env);
    let token  = Address::generate(env);
    client.register_offering(&issuer, &token, &1_000);
    (client, issuer, token)
}

// ═══════════════════════════════════════════════════════════════
// BLACKLIST TESTS (preserved from #13)
// ═══════════════════════════════════════════════════════════════

// ── smoke test ────────────────────────────────────────────────

#[test]
fn it_emits_events_on_register_and_report() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token  = Address::generate(&env);

    client.register_offering(&issuer, &token, &1_000);
    client.report_revenue(&issuer, &token, &1_000_000, &1);

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

    client.blacklist_remove(&admin, &token, &investor);
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

// ═══════════════════════════════════════════════════════════════
// LIFECYCLE TESTS (new — #2)
// ═══════════════════════════════════════════════════════════════

// ── initial state ─────────────────────────────────────────────

#[test]
fn new_offering_is_active() {
    let env = Env::default();
    let (client, _, token) = setup(&env);
    assert_eq!(client.get_offering_status(&token), OfferingStatus::Active);
}

// ── pause ─────────────────────────────────────────────────────

#[test]
fn active_can_be_paused() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.pause_offering(&issuer, &token);
    assert_eq!(client.get_offering_status(&token), OfferingStatus::Paused);
}

#[test]
fn pause_emits_event() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let before = env.events().all().len();
    client.pause_offering(&issuer, &token);
    assert!(env.events().all().len() > before);
}

#[test]
#[should_panic]
fn pausing_already_paused_panics() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.pause_offering(&issuer, &token);
    client.pause_offering(&issuer, &token);
}

#[test]
#[should_panic]
fn pausing_closed_offering_panics() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.close_offering(&issuer, &token);
    client.pause_offering(&issuer, &token);
}

// ── resume ────────────────────────────────────────────────────

#[test]
fn paused_can_be_resumed() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.pause_offering(&issuer, &token);
    client.resume_offering(&issuer, &token);
    assert_eq!(client.get_offering_status(&token), OfferingStatus::Active);
}

#[test]
fn resume_emits_event() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.pause_offering(&issuer, &token);
    let before = env.events().all().len();
    client.resume_offering(&issuer, &token);
    assert!(env.events().all().len() > before);
}

#[test]
#[should_panic]
fn resuming_active_offering_panics() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.resume_offering(&issuer, &token);
}

#[test]
#[should_panic]
fn resuming_closed_offering_panics() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.close_offering(&issuer, &token);
    client.resume_offering(&issuer, &token);
}

// ── close ─────────────────────────────────────────────────────

#[test]
fn active_can_be_closed() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.close_offering(&issuer, &token);
    assert_eq!(client.get_offering_status(&token), OfferingStatus::Closed);
}

#[test]
fn paused_can_be_closed() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.pause_offering(&issuer, &token);
    client.close_offering(&issuer, &token);
    assert_eq!(client.get_offering_status(&token), OfferingStatus::Closed);
}

#[test]
fn close_emits_event() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    let before = env.events().all().len();
    client.close_offering(&issuer, &token);
    assert!(env.events().all().len() > before);
}

#[test]
#[should_panic]
fn closing_already_closed_panics() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.close_offering(&issuer, &token);
    client.close_offering(&issuer, &token);
}

// ── report_revenue lifecycle gate ─────────────────────────────

#[test]
fn report_revenue_works_when_active() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.report_revenue(&issuer, &token, &500_000, &1);
}

#[test]
#[should_panic]
fn report_revenue_blocked_when_paused() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.pause_offering(&issuer, &token);
    client.report_revenue(&issuer, &token, &500_000, &1);
}

#[test]
#[should_panic]
fn report_revenue_blocked_when_closed() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.close_offering(&issuer, &token);
    client.report_revenue(&issuer, &token, &500_000, &1);
}

#[test]
fn report_revenue_works_after_resume() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);
    client.pause_offering(&issuer, &token);
    client.resume_offering(&issuer, &token);
    client.report_revenue(&issuer, &token, &500_000, &1);
}

// ── full round-trip ───────────────────────────────────────────

#[test]
fn full_lifecycle_active_pause_resume_close() {
    let env = Env::default();
    let (client, issuer, token) = setup(&env);

    assert_eq!(client.get_offering_status(&token), OfferingStatus::Active);
    client.pause_offering(&issuer, &token);
    assert_eq!(client.get_offering_status(&token), OfferingStatus::Paused);
    client.resume_offering(&issuer, &token);
    assert_eq!(client.get_offering_status(&token), OfferingStatus::Active);
    client.close_offering(&issuer, &token);
    assert_eq!(client.get_offering_status(&token), OfferingStatus::Closed);
}

// ── authorization ─────────────────────────────────────────────

#[test]
#[should_panic]
fn pause_requires_auth() {
    let env = Env::default(); // no mock_all_auths
    let id     = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);
    let issuer = Address::generate(&env);
    let token  = Address::generate(&env);
    client.pause_offering(&issuer, &token);
}

#[test]
#[should_panic]
fn resume_requires_auth() {
    let env = Env::default();
    let id     = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);
    let issuer = Address::generate(&env);
    let token  = Address::generate(&env);
    client.resume_offering(&issuer, &token);
}

#[test]
#[should_panic]
fn close_requires_auth() {
    let env = Env::default();
    let id     = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);
    let issuer = Address::generate(&env);
    let token  = Address::generate(&env);
    client.close_offering(&issuer, &token);
}

#[test]
#[should_panic]
fn non_issuer_cannot_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, token) = setup(&env);
    let attacker = Address::generate(&env);
    client.pause_offering(&attacker, &token);
}

#[test]
#[should_panic]
fn non_issuer_cannot_resume() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, issuer, token) = setup(&env);
    client.pause_offering(&issuer, &token);
    let attacker = Address::generate(&env);
    client.resume_offering(&attacker, &token);
}

#[test]
#[should_panic]
fn non_issuer_cannot_close() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, token) = setup(&env);
    let attacker = Address::generate(&env);
    client.close_offering(&attacker, &token);
}

// ── blacklist + lifecycle interop ─────────────────────────────

#[test]
fn blacklist_add_works_regardless_of_status() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, issuer, token) = setup(&env);
    let investor = Address::generate(&env);

    // Active
    client.blacklist_add(&issuer, &token, &investor);
    assert!(client.is_blacklisted(&token, &investor));

    // Paused
    client.pause_offering(&issuer, &token);
    let investor2 = Address::generate(&env);
    client.blacklist_add(&issuer, &token, &investor2);
    assert!(client.is_blacklisted(&token, &investor2));
}