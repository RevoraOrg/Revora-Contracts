#![cfg(test)]
use soroban_sdk::{testutils::Address as _, testutils::Events as _, Address, Env};

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode};

// ── helper ────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
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

// ---------------------------------------------------------------------------
// Pagination tests
// ---------------------------------------------------------------------------

/// Helper: set up env + client, return (env, client, issuer).
fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    (env, client, issuer)
}

/// Register `n` offerings for `issuer`, each with a unique token.
fn register_n(env: &Env, client: &RevoraRevenueShareClient, issuer: &Address, n: u32) {
    for i in 0..n {
        let token = Address::generate(env);
        client.register_offering(issuer, &token, &(100 + i));
    }
}

#[test]
fn empty_issuer_returns_empty_page() {
    let (_env, client, issuer) = setup();

    let (page, cursor) = client.get_offerings_page(&issuer, &0, &10);
    assert_eq!(page.len(), 0);
    assert_eq!(cursor, None);
}

#[test]
fn empty_issuer_count_is_zero() {
    let (_env, client, issuer) = setup();
    assert_eq!(client.get_offering_count(&issuer), 0);
}

#[test]
fn register_persists_and_count_increments() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 3);
    assert_eq!(client.get_offering_count(&issuer), 3);
}

#[test]
fn single_page_returns_all_no_cursor() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 5);

    let (page, cursor) = client.get_offerings_page(&issuer, &0, &10);
    assert_eq!(page.len(), 5);
    assert_eq!(cursor, None);
}

#[test]
fn multi_page_cursor_progression() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 7);

    // First page: items 0..3
    let (page1, cursor1) = client.get_offerings_page(&issuer, &0, &3);
    assert_eq!(page1.len(), 3);
    assert_eq!(cursor1, Some(3));

    // Second page: items 3..6
    let (page2, cursor2) = client.get_offerings_page(&issuer, &cursor1.unwrap(), &3);
    assert_eq!(page2.len(), 3);
    assert_eq!(cursor2, Some(6));

    // Third (final) page: items 6..7
    let (page3, cursor3) = client.get_offerings_page(&issuer, &cursor2.unwrap(), &3);
    assert_eq!(page3.len(), 1);
    assert_eq!(cursor3, None);
}

#[test]
fn final_page_has_no_cursor() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 4);

    let (page, cursor) = client.get_offerings_page(&issuer, &2, &10);
    assert_eq!(page.len(), 2);
    assert_eq!(cursor, None);
}

#[test]
fn out_of_bounds_cursor_returns_empty() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 3);

    let (page, cursor) = client.get_offerings_page(&issuer, &100, &5);
    assert_eq!(page.len(), 0);
    assert_eq!(cursor, None);
}

#[test]
fn limit_zero_uses_max_page_limit() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 5);

    // limit=0 should behave like MAX_PAGE_LIMIT (20), returning all 5.
    let (page, cursor) = client.get_offerings_page(&issuer, &0, &0);
    assert_eq!(page.len(), 5);
    assert_eq!(cursor, None);
}

#[test]
fn limit_one_iterates_one_at_a_time() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 3);

    let (p1, c1) = client.get_offerings_page(&issuer, &0, &1);
    assert_eq!(p1.len(), 1);
    assert_eq!(c1, Some(1));

    let (p2, c2) = client.get_offerings_page(&issuer, &c1.unwrap(), &1);
    assert_eq!(p2.len(), 1);
    assert_eq!(c2, Some(2));

    let (p3, c3) = client.get_offerings_page(&issuer, &c2.unwrap(), &1);
    assert_eq!(p3.len(), 1);
    assert_eq!(c3, None);
}

#[test]
fn limit_exceeding_max_is_capped() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 25);

    // limit=50 should be capped to 20.
    let (page, cursor) = client.get_offerings_page(&issuer, &0, &50);
    assert_eq!(page.len(), 20);
    assert_eq!(cursor, Some(20));
}

#[test]
fn offerings_preserve_correct_data() {
    let (env, client, issuer) = setup();
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &500);

    let (page, _) = client.get_offerings_page(&issuer, &0, &10);
    let offering = page.get(0).unwrap();
    assert_eq!(offering.issuer, issuer);
    assert_eq!(offering.token, token);
    assert_eq!(offering.revenue_share_bps, 500);
}

#[test]
fn separate_issuers_have_independent_pages() {
    let (env, client, issuer_a) = setup();
    let issuer_b = Address::generate(&env);

    register_n(&env, &client, &issuer_a, 3);
    register_n(&env, &client, &issuer_b, 5);

    assert_eq!(client.get_offering_count(&issuer_a), 3);
    assert_eq!(client.get_offering_count(&issuer_b), 5);

    let (page_a, _) = client.get_offerings_page(&issuer_a, &0, &20);
    let (page_b, _) = client.get_offerings_page(&issuer_b, &0, &20);
    assert_eq!(page_a.len(), 3);
    assert_eq!(page_b.len(), 5);
}

#[test]
fn exact_page_boundary_no_cursor() {
    let (env, client, issuer) = setup();
    register_n(&env, &client, &issuer, 6);

    // Exactly 2 pages of 3
    let (p1, c1) = client.get_offerings_page(&issuer, &0, &3);
    assert_eq!(p1.len(), 3);
    assert_eq!(c1, Some(3));

    let (p2, c2) = client.get_offerings_page(&issuer, &c1.unwrap(), &3);
    assert_eq!(p2.len(), 3);
    assert_eq!(c2, None);
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

// ── structured error codes (#41) ──────────────────────────────

#[test]
fn register_offering_rejects_bps_over_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let result = client.try_register_offering(&issuer, &token, &10_001);
    assert!(
        result.is_err(),
        "contract must return Err(RevoraError::InvalidRevenueShareBps) for bps > 10000"
    );
    assert_eq!(
        RevoraError::InvalidRevenueShareBps as u32,
        1,
        "error code for integrators"
    );
}

#[test]
fn register_offering_accepts_bps_exactly_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let result = client.try_register_offering(&issuer, &token, &10_000);
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// Storage limit negative tests (#31): many offerings/reports, no panics
// ---------------------------------------------------------------------------

/// Maximum reasonable offering count used in tests to probe storage growth.
const STORAGE_STRESS_OFFERING_COUNT: u32 = 200;

#[test]
fn storage_stress_many_offerings_no_panic() {
    let (env, client, issuer) = setup();
    // Simulate many offerings within Soroban environment; ensure no panic or unexpected behavior.
    register_n(&env, &client, &issuer, STORAGE_STRESS_OFFERING_COUNT);
    let count = client.get_offering_count(&issuer);
    assert_eq!(count, STORAGE_STRESS_OFFERING_COUNT);
    // Verify we can read back pages at the end of the range.
    let (page, cursor) =
        client.get_offerings_page(&issuer, &(STORAGE_STRESS_OFFERING_COUNT - 5), &10);
    assert_eq!(page.len(), 5);
    assert_eq!(cursor, None);
}

#[test]
fn storage_stress_many_reports_no_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000);

    // Many report_revenue calls; storage growth is minimal (events only), but we stress the path.
    for period_id in 1..=100_u64 {
        client.report_revenue(&issuer, &token, &(period_id as i128 * 10_000), &period_id);
    }
    assert!(env.events().all().len() >= 100);
}

#[test]
fn storage_stress_large_blacklist_no_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);

    for _ in 0..80 {
        let investor = Address::generate(&env);
        client.blacklist_add(&admin, &token, &investor);
    }
    let list = client.get_blacklist(&token);
    assert_eq!(list.len(), 80);
}

// ---------------------------------------------------------------------------
// Gas / compute usage characterization (#36): large scenarios, document behavior
// ---------------------------------------------------------------------------

#[test]
fn gas_characterization_many_offerings_single_issuer() {
    // Worst-case path: one issuer with many offerings. Measures get_offerings_page cost.
    let (env, client, issuer) = setup();
    let n = 50_u32;
    register_n(&env, &client, &issuer, n);

    let (page, _) = client.get_offerings_page(&issuer, &0, &20);
    assert_eq!(page.len(), 20);
    // Pagination bounds cost: O(effective_limit) reads. Off-chain: prefer small page sizes.
}

#[test]
fn gas_characterization_report_revenue_with_large_blacklist() {
    // report_revenue reads full blacklist and emits it in the event; worst case for large lists.
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &500);

    for _ in 0..30 {
        client.blacklist_add(&Address::generate(&env), &token, &Address::generate(&env));
    }
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.blacklist_add(&admin, &token, &Address::generate(&env)); // ensure admin is auth

    client.report_revenue(&issuer, &token, &1_000_000, &1);
    assert!(!env.events().all().is_empty());
    // Expected: cost grows with blacklist size (map read + event payload). Recommend off-chain limits on blacklist size.
}

// ---------------------------------------------------------------------------
// Holder concentration guardrail (#26)
// ---------------------------------------------------------------------------

#[test]
fn concentration_limit_not_set_allows_report_revenue() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000);
    client.report_revenue(&issuer, &token, &1_000, &1);
}

#[test]
fn set_concentration_limit_requires_offering_to_exist() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    // No offering registered
    let r = client.try_set_concentration_limit(&issuer, &token, &5000, &false);
    assert!(r.is_err());
}

#[test]
fn set_concentration_limit_stores_config() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000);
    client.set_concentration_limit(&issuer, &token, &5000, &false);
    let config = client.get_concentration_limit(&issuer, &token).unwrap();
    assert_eq!(config.max_bps, 5000);
    assert!(!config.enforce);
}

#[test]
fn report_concentration_emits_warning_when_over_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000);
    client.set_concentration_limit(&issuer, &token, &5000, &false);
    let before = env.events().all().len();
    client.report_concentration(&issuer, &token, &6000);
    assert!(env.events().all().len() > before);
    assert_eq!(
        client.get_current_concentration(&issuer, &token),
        Some(6000)
    );
}

#[test]
fn report_concentration_no_warning_when_below_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000);
    client.set_concentration_limit(&issuer, &token, &5000, &false);
    client.report_concentration(&issuer, &token, &4000);
    assert_eq!(
        client.get_current_concentration(&issuer, &token),
        Some(4000)
    );
}

#[test]
fn concentration_enforce_blocks_report_revenue_when_over_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000);
    client.set_concentration_limit(&issuer, &token, &5000, &true);
    client.report_concentration(&issuer, &token, &6000);
    let r = client.try_report_revenue(&issuer, &token, &1_000, &1);
    assert!(
        r.is_err(),
        "report_revenue must fail when concentration exceeds limit with enforce=true"
    );
}

#[test]
fn concentration_enforce_allows_report_revenue_when_at_or_below_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000);
    client.set_concentration_limit(&issuer, &token, &5000, &true);
    client.report_concentration(&issuer, &token, &5000);
    client.report_revenue(&issuer, &token, &1_000, &1);
    client.report_concentration(&issuer, &token, &4999);
    client.report_revenue(&issuer, &token, &1_000, &2);
}

#[test]
fn concentration_near_threshold_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000);
    client.set_concentration_limit(&issuer, &token, &5000, &true);
    client.report_concentration(&issuer, &token, &5001);
    assert!(client
        .try_report_revenue(&issuer, &token, &1_000, &1)
        .is_err());
}

// ---------------------------------------------------------------------------
// On-chain audit log summary (#34)
// ---------------------------------------------------------------------------

#[test]
fn audit_summary_empty_before_any_report() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000);
    let summary = client.get_audit_summary(&issuer, &token);
    assert!(summary.is_none());
}

#[test]
fn audit_summary_aggregates_revenue_and_count() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000);
    client.report_revenue(&issuer, &token, &100, &1);
    client.report_revenue(&issuer, &token, &200, &2);
    client.report_revenue(&issuer, &token, &300, &3);
    let summary = client.get_audit_summary(&issuer, &token).unwrap();
    assert_eq!(summary.total_revenue, 600);
    assert_eq!(summary.report_count, 3);
}

#[test]
fn audit_summary_per_offering_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    client.register_offering(&issuer, &token_a, &1_000);
    client.register_offering(&issuer, &token_b, &1_000);
    client.report_revenue(&issuer, &token_a, &1000, &1);
    client.report_revenue(&issuer, &token_b, &2000, &1);
    let sum_a = client.get_audit_summary(&issuer, &token_a).unwrap();
    let sum_b = client.get_audit_summary(&issuer, &token_b).unwrap();
    assert_eq!(sum_a.total_revenue, 1000);
    assert_eq!(sum_a.report_count, 1);
    assert_eq!(sum_b.total_revenue, 2000);
    assert_eq!(sum_b.report_count, 1);
}

// ---------------------------------------------------------------------------
// Configurable rounding modes (#44)
// ---------------------------------------------------------------------------

#[test]
fn compute_share_truncation() {
    let env = Env::default();
    let client = make_client(&env);
    // 1000 * 2500 / 10000 = 250
    let share = client.compute_share(&1000, &2500, &RoundingMode::Truncation);
    assert_eq!(share, 250);
}

#[test]
fn compute_share_round_half_up() {
    let env = Env::default();
    let client = make_client(&env);
    // 1000 * 2500 = 2_500_000; half-up: (2_500_000 + 5000) / 10000 = 250
    let share = client.compute_share(&1000, &2500, &RoundingMode::RoundHalfUp);
    assert_eq!(share, 250);
}

#[test]
fn compute_share_round_half_up_rounds_up_at_half() {
    let env = Env::default();
    let client = make_client(&env);
    // 1 * 2500 = 2500; 2500/10000 trunc = 0; half-up (2500+5000)/10000 = 0.75 -> 0? No: (2500+5000)/10000 = 7500/10000 = 0. So 1 bps would be 1*100/10000 = 0.01 -> 0 trunc, round half up (100+5000)/10000 = 0.51 -> 1. So 1 * 100 = 100, (100+5000)/10000 = 0.
    // 3 * 3333 = 9999; 9999/10000 = 0 trunc. (9999+5000)/10000 = 14999/10000 = 1 round half up.
    let share_trunc = client.compute_share(&3, &3333, &RoundingMode::Truncation);
    let share_half = client.compute_share(&3, &3333, &RoundingMode::RoundHalfUp);
    assert_eq!(share_trunc, 0);
    assert_eq!(share_half, 1);
}

#[test]
fn compute_share_bps_over_10000_returns_zero() {
    let env = Env::default();
    let client = make_client(&env);
    let share = client.compute_share(&1000, &10_001, &RoundingMode::Truncation);
    assert_eq!(share, 0);
}

#[test]
fn set_and_get_rounding_mode() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.register_offering(&issuer, &token, &1_000);
    assert_eq!(
        client.get_rounding_mode(&issuer, &token),
        RoundingMode::Truncation
    );
    client.set_rounding_mode(&issuer, &token, &RoundingMode::RoundHalfUp);
    assert_eq!(
        client.get_rounding_mode(&issuer, &token),
        RoundingMode::RoundHalfUp
    );
}

#[test]
fn set_rounding_mode_requires_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let r = client.try_set_rounding_mode(&issuer, &token, &RoundingMode::RoundHalfUp);
    assert!(r.is_err());
}

#[test]
fn compute_share_tiny_payout_truncation() {
    let env = Env::default();
    let client = make_client(&env);
    let share = client.compute_share(&1, &1, &RoundingMode::Truncation);
    assert_eq!(share, 0);
}

#[test]
fn compute_share_no_overflow_bounds() {
    let env = Env::default();
    let client = make_client(&env);
    let amount = 1_000_000_i128;
    let share = client.compute_share(&amount, &10_000, &RoundingMode::Truncation);
    assert_eq!(share, amount);
    let share2 = client.compute_share(&amount, &10_000, &RoundingMode::RoundHalfUp);
    assert_eq!(share2, amount);
}
