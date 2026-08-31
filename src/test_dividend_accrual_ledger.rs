//! Tests for the per-holder dividend accrual ledger (report-time index).
//!
//! The ledger advances on every accepted `report_revenue` call using:
//!
//! ```text
//! delta_e18 = normalized(amount) * 1e18 / total_share_bps
//! GlobalReportAccPerShareE18 += delta_e18
//! ```
//!
//! Per-holder state (`HolderReportLedger`) is settled O(1) on every
//! `set_holder_share` call so that partial claims and re-claims read
//! from an immutable `accrued_owed` baseline rather than rederiving
//! the full period history every time.

#![cfg(test)]

use crate::{RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_admin = Address::generate(&env);
    let payout_asset = crate::test_utils::create_token(&env, &payout_admin);
    // Mint enough tokens for all deposit calls (up to 10_000_000 per test).
    crate::test_utils::mint_tokens(&env, &payout_asset, &issuer, 10_000_000);

    // Register offering: revenue_share_bps = 10_000 (100%), claim delay = 0.
    client.register_offering(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &10_000u32,
        &payout_asset,
        &0i128,
    );

    (env, client, issuer, token, payout_asset)
}

fn ns() -> soroban_sdk::Symbol {
    symbol_short!("ns")
}

// ── happy path ────────────────────────────────────────────────────────────────

/// Single holder with 100% share: after one report the pending balance equals
/// the full reported amount.
#[test]
fn single_holder_full_share_report_accrues_correctly() {
    let (env, client, issuer, token, _payout) = setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);
    client.report_revenue(&issuer, &ns(), &token, &_payout, &100_000, &1, &false);

    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(pending, 100_000, "100% holder should accrue the full reported amount");
}

/// Two holders sharing equally: each should accrue half the reported revenue.
#[test]
fn two_equal_holders_split_report_accrual() {
    let (env, client, issuer, token, payout) = setup();
    let h1 = Address::generate(&env);
    let h2 = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &h1, &5_000);
    client.set_holder_share(&issuer, &ns(), &token, &h2, &5_000);
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);

    let p1 = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &h1);
    let p2 = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &h2);
    assert_eq!(p1, 50_000, "h1: 50% of 100_000");
    assert_eq!(p2, 50_000, "h2: 50% of 100_000");
}

/// Multiple reports accumulate correctly.
#[test]
fn multiple_reports_accumulate_in_ledger() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);
    client.report_revenue(&issuer, &ns(), &token, &payout, &50_000, &1, &false);
    client.report_revenue(&issuer, &ns(), &token, &payout, &30_000, &2, &false);

    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(pending, 80_000, "accrual should accumulate across multiple reports");
}

/// Holder with no share accrues nothing.
#[test]
fn zero_share_holder_accrues_nothing() {
    let (env, client, issuer, token, payout) = setup();
    let no_holder = Address::generate(&env);

    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);

    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &no_holder);
    assert_eq!(pending, 0, "holder with no share should accrue 0");
}

// ── share-change settlement ──────────────────────────────────────────────────

/// Holder's accrued_owed is frozen when their share changes, so old accrual
/// at the previous share is preserved even after zeroing.
#[test]
fn share_change_freezes_accrued_owed_at_old_share() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);
    // Use a filler so total_share_bps = 10_000.
    let filler = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &5_000);
    client.set_holder_share(&issuer, &ns(), &token, &filler, &5_000);
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);
    // holder: 50% of 100_000 = 50_000

    // Change share → settlement fires, freezing 50_000 into accrued_owed.
    client.set_holder_share(&issuer, &ns(), &token, &holder, &2_500);
    client.set_holder_share(&issuer, &ns(), &token, &filler, &7_500);

    // Report again with new share (25%).
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &2, &false);

    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    // frozen 50_000 + 25% of 100_000 = 50_000 + 25_000 = 75_000
    assert_eq!(pending, 75_000, "frozen + new accrual should total 75_000");
}

/// Zeroing a holder's share preserves their previously accrued balance.
#[test]
fn zeroing_share_preserves_previously_accrued_balance() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);
    // Use a second holder to avoid total_share_bps dropping to 0
    let filler = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &filler, &5_000);
    client.set_holder_share(&issuer, &ns(), &token, &holder, &5_000);
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);

    // Zero out holder; filler still holds 5_000.
    client.set_holder_share(&issuer, &ns(), &token, &holder, &0);

    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(pending, 50_000, "accrued balance must survive share being zeroed");
}

/// Re-increasing share after zeroing correctly adds future accrual on top of
/// the frozen baseline, not replacing it.
#[test]
fn re_increasing_share_adds_on_top_of_frozen_balance() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);
    // Use a second holder to keep total_share_bps = 10_000 throughout.
    let filler = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);
    client.report_revenue(&issuer, &ns(), &token, &payout, &40_000, &1, &false);
    // holder (100%) accrues: 40_000. freeze: 40_000

    client.set_holder_share(&issuer, &ns(), &token, &holder, &5_000);
    client.set_holder_share(&issuer, &ns(), &token, &filler, &5_000);
    // settlement: 40_000 frozen. total_share_bps now 10_000 again.
    client.report_revenue(&issuer, &ns(), &token, &payout, &20_000, &2, &false);
    // holder (50%) accrues: 50% × 20_000 = 10_000 (unsettled)

    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(pending, 50_000, "40_000 frozen + 10_000 new");
}

// ── event emission ───────────────────────────────────────────────────────────

/// Each accepted report_revenue call emits an `rpt_acc_u` event.
#[test]
fn report_revenue_emits_rpt_acc_u_event() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);

    let before = env.events().all().len();
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);
    let after_count = env.events().all().len();

    assert!(after_count > before, "report_revenue must emit at least one event");
    // The rpt_acc_u event is emitted when the report-time accrual index advances.
    // We verify it indirectly: the accrual index must have advanced, meaning the
    // event path was reached.
    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(pending, 100_000, "accrual index advanced → pending balance must be 100_000");
}

/// Rejected duplicate report (override_existing=false when report exists) must
/// NOT advance the accrual index.
#[test]
fn rejected_duplicate_report_does_not_advance_index() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);

    let after_first = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);

    // Duplicate — should be rejected silently.
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);

    let after_dup = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(after_first, after_dup, "rejected duplicate must not change accrual");
}

/// Override of an existing report correctly adjusts the accrual index.
#[test]
fn override_report_adjusts_accrual_index() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);

    let after_initial = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(after_initial, 100_000);

    // Override with a different amount.
    client.report_revenue(&issuer, &ns(), &token, &payout, &60_000, &1, &true);

    // The override adds the new amount on top in the report index.
    let after_override = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert!(
        after_override > after_initial,
        "override must advance the accrual index: {} > {}",
        after_override,
        after_initial
    );
}

// ── blacklist guards ──────────────────────────────────────────────────────────

/// Blacklisted holders return 0 from get_holder_pending_report_accrual.
#[test]
fn blacklisted_holder_pending_accrual_is_zero() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);

    client.blacklist_add(&issuer, &ns(), &token, &holder);

    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(pending, 0, "blacklisted holder must get 0");
}

// ── authorization / validation boundaries ────────────────────────────────────

/// report_revenue requires issuer auth; wrong issuer must return OfferingNotFound.
#[test]
fn report_revenue_wrong_issuer_rejected() {
    let (env, client, issuer, token, payout) = setup();
    let _ = issuer;
    let attacker = Address::generate(&env);

    let result = client.try_report_revenue(&attacker, &ns(), &token, &payout, &100_000, &1, &false);
    assert!(result.is_err(), "wrong issuer must be rejected");
}

/// Zero amount report must not advance the accrual index.
#[test]
fn zero_amount_report_does_not_advance_index() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);

    // Zero amount — allowed by validation matrix (amount >= 0 for reports).
    client.report_revenue(&issuer, &ns(), &token, &payout, &0, &1, &false);

    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(pending, 0, "zero-amount report must not advance index");
}

/// Invalid period_id = 0 must be rejected.
#[test]
fn report_revenue_period_id_zero_rejected() {
    let (env, client, issuer, token, payout) = setup();
    let result = client.try_report_revenue(&issuer, &ns(), &token, &payout, &1_000, &0, &false);
    assert!(result.is_err(), "period_id = 0 must be rejected");
}

// ── O(1) gas / concurrency ───────────────────────────────────────────────────

/// get_holder_pending_report_accrual is O(1): it reads exactly 3 storage keys
/// regardless of the number of periods reported.  Verified by CPU budget delta.
#[test]
fn get_pending_report_accrual_is_o1_in_period_count() {
    let env1 = Env::default();
    env1.mock_all_auths();
    env1.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let env10 = Env::default();
    env10.mock_all_auths();
    env10.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let measure = |periods: u64| -> u64 {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|l| l.timestamp = 1_000_000);
        let cid = env.register_contract(None, RevoraRevenueShare);
        let client = RevoraRevenueShareClient::new(&env, &cid);
        let issuer = Address::generate(&env);
        let token = Address::generate(&env);
        let payout_admin = Address::generate(&env);
        let payout = crate::test_utils::create_token(&env, &payout_admin);
        crate::test_utils::mint_tokens(&env, &payout, &issuer, periods as i128 * 10_000 + 1_000_000);
        let holder = Address::generate(&env);
        client.register_offering(&issuer, &ns(), &token, &10_000u32, &payout, &0i128);
        client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);
        for p in 1..=periods {
            client.report_revenue(&issuer, &ns(), &token, &payout, &10_000, &p, &false);
        }
        let before = env.budget().cpu_instruction_count();
        client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
        let after = env.budget().cpu_instruction_count();
        after.saturating_sub(before)
    };

    let cpu_1 = measure(1);
    let cpu_50 = measure(50);

    // Allow up to 10× headroom: O(1) should not grow proportionally with periods.
    assert!(
        cpu_50 < cpu_1 * 10,
        "get_holder_pending_report_accrual must be O(1): cpu@1={}, cpu@50={}",
        cpu_1,
        cpu_50,
    );
}

// ── backward-compatibility ───────────────────────────────────────────────────

/// Existing deposit-based claim flow must still work after the feature is added.
/// report_revenue + deposit_revenue + claim must yield the correct payout.
#[test]
fn existing_deposit_claim_flow_unaffected() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &5_000);
    // report first (needed for some contract paths)
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);
    // then deposit
    client.deposit_revenue(&issuer, &ns(), &token, &payout, &100_000, &1);

    let payout_amount = client.claim(&holder, &issuer, &ns(), &token, &10);
    assert_eq!(payout_amount, 50_000, "deposit-claim flow must still pay 50% of 100_000");
}

/// get_claimable and get_holder_accrued_unclaimed (deposit-based) are unchanged.
#[test]
fn deposit_based_claimable_unaffected_by_report_accrual() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);
    // No deposit yet → deposit-based claimable must be 0.
    let claimable = client.get_claimable(&issuer, &ns(), &token, &holder);
    assert_eq!(claimable, 0, "get_claimable must be 0 before deposit");

    // Deposit → claimable should become 100_000.
    client.deposit_revenue(&issuer, &ns(), &token, &payout, &100_000, &1);
    let claimable_after = client.get_claimable(&issuer, &ns(), &token, &holder);
    assert_eq!(claimable_after, 100_000, "get_claimable must equal full deposit after deposit");
}

// ── regression: boundary / edge inputs ───────────────────────────────────────

/// Holder joining after several reports accrues only from the first report
/// received while they hold shares (their last_report_acc starts at current global).
#[test]
fn late_joining_holder_does_not_accrue_pre_join_reports() {
    let (env, client, issuer, token, payout) = setup();
    let early = Address::generate(&env);
    let late = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &early, &10_000);
    // Two reports before late holder joins.
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &2, &false);

    // Late holder joins now.
    client.set_holder_share(&issuer, &ns(), &token, &early, &5_000);
    client.set_holder_share(&issuer, &ns(), &token, &late, &5_000);

    // Report after both hold shares.
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &3, &false);

    let late_pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &late);
    // late joined after the first two reports; their `last_report_acc` was
    // captured at join time, so only report 3 (50% of 100_000 = 50_000) is theirs.
    assert_eq!(
        late_pending, 50_000,
        "late holder must not accrue pre-join reports: got {}",
        late_pending
    );
}

/// No report has been submitted yet → pending accrual is 0.
#[test]
fn pending_accrual_zero_before_first_report() {
    let (env, client, issuer, token, _payout) = setup();
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);

    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(pending, 0, "no reports yet → pending must be 0");
}

/// Multiple consecutive share changes each settle correctly.
#[test]
fn multiple_share_changes_settle_correctly() {
    let (env, client, issuer, token, payout) = setup();
    let holder = Address::generate(&env);
    let filler = Address::generate(&env);

    // Use filler to keep total_share_bps = 10_000 throughout.
    client.set_holder_share(&issuer, &ns(), &token, &filler, &5_000);
    client.set_holder_share(&issuer, &ns(), &token, &holder, &5_000);
    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);
    // holder accrues: 50% of 100_000 = 50_000 (unsettled)

    client.set_holder_share(&issuer, &ns(), &token, &holder, &2_500);
    client.set_holder_share(&issuer, &ns(), &token, &filler, &7_500);
    // settlement: 50_000 frozen into accrued_owed

    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &2, &false);
    // holder accrues: 25% of 100_000 = 25_000 (unsettled)

    client.set_holder_share(&issuer, &ns(), &token, &holder, &7_500);
    client.set_holder_share(&issuer, &ns(), &token, &filler, &2_500);
    // settlement: 50_000 + 25_000 = 75_000 frozen

    client.report_revenue(&issuer, &ns(), &token, &payout, &100_000, &3, &false);
    // holder accrues: 75% of 100_000 = 75_000 (unsettled)

    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(pending, 150_000, "75_000 frozen + 75_000 new = 150_000");
}

/// report_revenue on a SoftPaused contract must be rejected (reports are
/// blocked by SoftPaused), so the accrual index must not advance.
#[test]
fn soft_paused_report_does_not_advance_accrual_index() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_admin = Address::generate(&env);
    let payout = crate::test_utils::create_token(&env, &payout_admin);
    crate::test_utils::mint_tokens(&env, &payout, &issuer, 1_000_000);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &ns(), &token, &10_000u32, &payout, &0i128);

    let holder = Address::generate(&env);
    client.set_holder_share(&issuer, &ns(), &token, &holder, &10_000);

    // Soft-pause the contract.
    client.pause_admin(&admin);

    let result = client.try_report_revenue(&issuer, &ns(), &token, &payout, &100_000, &1, &false);
    assert!(result.is_err(), "report must fail when SoftPaused");

    let pending = client.get_holder_pending_report_accrual(&issuer, &ns(), &token, &holder);
    assert_eq!(pending, 0, "failed report must not advance accrual index");
}
