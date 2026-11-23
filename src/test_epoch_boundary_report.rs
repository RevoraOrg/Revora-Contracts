//! Epoch-boundary `report_revenue` tests (#835)
//!
//! Validates that `require_next_period_id` monotonicity is preserved when a
//! reporting window is reconfigured across an epoch boundary (end of epoch N /
//! start of epoch N+1) between two `report_revenue` calls.
//!
//! # Security assumptions
//! - `set_report_window` is issuer-auth-gated; only the offering issuer may
//!   reconfigure the window.
//! - `report_revenue` enforces `require_report_window_open` at call time, so
//!   the window visible to the transaction is the one stored at ledger close.
//! - `require_next_period_id` enforces strict sequential ordering
//!   (`period_id == last + 1`) regardless of wall-clock time or window state.
//! - A window cutover must not permit skipping period_id slots or reusing old ones.
//!
//! # Coverage
//! - Happy path: two reports straddling a window cutover succeed in order.
//! - Zero-width window at exact boundary timestamp.
//! - Overlapping windows during cutover.
//! - Skipped period_id rejected after cutover.
//! - Window reset to zero-width still enforces ordering.

#![cfg(test)]
#![allow(unused_imports)]

use crate::{DataKey2, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    token, Address, Env, Symbol, Vec,
};

// â”€â”€ Helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    (token_id, admin)
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    token::StellarAssetClient::new(env, token).mint(to, &amount);
}

fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

fn setup_offering() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (payment_token, _) = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &offering_token,
        &1_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0,
    );
    mint(&env, &payment_token, &issuer, 10_000_000);

    (env, client, issuer, offering_token, payment_token)
}

fn last_reported_period_id(
    env: &Env,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) -> Option<u64> {
    let offering_id = crate::OfferingId {
        issuer: issuer.clone(),
        namespace: namespace.clone(),
        token: token.clone(),
    };
    env.storage().persistent().get(&DataKey2::LastReportedPeriodId(offering_id))
}

// â”€â”€ SECTION 1 â€” Happy path: epoch-boundary cutover preserves ordering â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Configure window [A, B], report period 1 at A, move to B+1, reconfigure
/// to [B+1, C], report period 2. Both must succeed and last_report_period_id == 2.
#[test]
fn epoch_boundary_cutover_preserves_period_ordering() {
    let (env, client, issuer, token, _payment_token) = setup_offering();

    let epoch_a = 1_000u64;
    let epoch_b = 2_000u64;
    let epoch_c = 3_000u64;

    // Window [A, B] = [1000, 2000]
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &epoch_a, &epoch_b);

    // Report period 1 at exactly A (boundary is inclusive)
    set_time(&env, epoch_a);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false);

    // Advance past B and reconfigure to [B+1, C] = [2001, 3000]
    set_time(&env, epoch_b + 1);
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &(epoch_b + 1), &epoch_c);

    // Report period 2 at B+1 (new window start)
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &200, &2, &false);

    // Invariant: last reported period must be 2
    assert_eq!(last_reported_period_id(&env, &issuer, &symbol_short!("ns"), &token), Some(2));

    // Next expected period is 3; reporting 3 must succeed (no skipped slots)
    set_time(&env, epoch_b + 2);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &300, &3, &false);

    // Reporting 5 (skipping 4) must fail
    let r =
        client.try_report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &400, &5, &false);
    assert_eq!(r, Err(Ok(RevoraError::InvalidPeriodId)));
}

/// Zero-width window [B+1, B+1] at the cutover instant must still allow the
/// next sequential period through.
#[test]
fn epoch_boundary_zero_width_window_allows_next_period() {
    let (env, client, issuer, token, _payment_token) = setup_offering();

    let epoch_a = 1_000u64;
    let epoch_b = 2_000u64;

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &epoch_a, &epoch_b);

    set_time(&env, epoch_a);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false);

    // Zero-width window at B+1
    set_time(&env, epoch_b + 1);
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &(epoch_b + 1), &(epoch_b + 1));

    // Period 2 must succeed at the exact boundary instant
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &200, &2, &false);
    assert_eq!(last_reported_period_id(&env, &issuer, &symbol_short!("ns"), &token), Some(2));
}

/// Overlapping windows [A, B] then [B-1, C] must not break sequential ordering.
#[test]
fn epoch_boundary_overlapping_windows_preserve_ordering() {
    let (env, client, issuer, token, _payment_token) = setup_offering();

    let epoch_a = 1_000u64;
    let epoch_b = 2_000u64;
    let epoch_c = 3_000u64;

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &epoch_a, &epoch_b);

    set_time(&env, epoch_a);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false);

    // Overlapping new window: [B-1, C] = [1999, 3000]
    set_time(&env, epoch_b + 1);
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &(epoch_b - 1), &epoch_c);

    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &200, &2, &false);
    assert_eq!(last_reported_period_id(&env, &issuer, &symbol_short!("ns"), &token), Some(2));
}

/// After a cutover, attempting to reuse the old period_id must fail.
#[test]
fn epoch_boundary_old_period_id_rejected_after_cutover() {
    let (env, client, issuer, token, _payment_token) = setup_offering();

    let epoch_a = 1_000u64;
    let epoch_b = 2_000u64;
    let epoch_c = 3_000u64;

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &epoch_a, &epoch_b);

    set_time(&env, epoch_a);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false);

    set_time(&env, epoch_b + 1);
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &(epoch_b + 1), &epoch_c);

    // Re-reporting period 1 without override must be silently rejected (no state change)
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &999, &1, &false);

    // last_reported_period_id must still be 1
    assert_eq!(last_reported_period_id(&env, &issuer, &symbol_short!("ns"), &token), Some(1));
}

/// A window reset to zero-width [0, 0] after reporting period 1 must still
/// enforce that period 2 is the next valid period_id.
#[test]
fn epoch_boundary_zero_width_window_reset_enforces_ordering() {
    let (env, client, issuer, token, _payment_token) = setup_offering();

    let epoch_a = 1_000u64;

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &epoch_a, &(epoch_a + 500));

    set_time(&env, epoch_a);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false);

    // Reset window to zero-width [0, 0] — only T=0 is open
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &0, &0);

    // At T=0, report period 2
    set_time(&env, 0);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &200, &2, &false);
    assert_eq!(last_reported_period_id(&env, &issuer, &symbol_short!("ns"), &token), Some(2));

    // At T=1, window is closed; reporting period 3 must fail with ReportingWindowClosed
    set_time(&env, 1);
    let r =
        client.try_report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &300, &3, &false);
    assert_eq!(r, Err(Ok(RevoraError::ReportingWindowClosed)));
}

// â”€â”€ SECTION 2 â€” Authorization boundary during cutover â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// A non-issuer cannot reconfigure the window mid-flight to cheat the ordering.
#[test]
fn epoch_boundary_non_issuer_cannot_reconfigure_window() {
    let (env, client, issuer, token, _payment_token) = setup_offering();
    let attacker = Address::generate(&env);

    let epoch_a = 1_000u64;
    let epoch_b = 2_000u64;
    let epoch_c = 3_000u64;

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &epoch_a, &epoch_b);

    set_time(&env, epoch_a);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false);

    set_time(&env, epoch_b + 1);
    let r = client.try_set_report_window(
        &attacker,
        &symbol_short!("ns"),
        &token,
        &(epoch_b + 1),
        &epoch_c,
    );
    assert!(r.is_err(), "non-issuer must not be able to set report window");
}

// â”€â”€ SECTION 3 â€” Backward-compat / regression: no window set remains always open â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// When no window is ever set, sequential period reporting still enforces ordering
/// across what would have been an epoch boundary.
#[test]
fn epoch_boundary_no_window_set_still_enforces_ordering() {
    let (env, client, issuer, token, _payment_token) = setup_offering();

    // No window configured — always open
    set_time(&env, 1_000);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false);

    set_time(&env, 2_001);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &200, &2, &false);

    assert_eq!(last_reported_period_id(&env, &issuer, &symbol_short!("ns"), &token), Some(2));

    // Gap still rejected
    let r =
        client.try_report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &300, &4, &false);
    assert_eq!(r, Err(Ok(RevoraError::InvalidPeriodId)));
}

// â”€â”€ SECTION 4 â€” Concurrency / retry safety: override flag semantics unchanged â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// With `override_existing=true`, re-reporting period 1 after a cutover still
/// updates the amount but does not advance `last_reported_period_id`.
#[test]
fn epoch_boundary_override_does_not_advance_period_pointer() {
    let (env, client, issuer, token, _payment_token) = setup_offering();

    let epoch_a = 1_000u64;
    let epoch_b = 2_000u64;
    let epoch_c = 3_000u64;

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &epoch_a, &epoch_b);

    set_time(&env, epoch_a);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false);

    set_time(&env, epoch_b + 1);
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &(epoch_b + 1), &epoch_c);

    // Override period 1
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &999, &1, &true);

    // last_reported_period_id must still be 1 — override is not a new period
    assert_eq!(last_reported_period_id(&env, &issuer, &symbol_short!("ns"), &token), Some(1));

    // Period 2 is still the next valid sequential period
    set_time(&env, epoch_b + 2);
    client.report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &200, &2, &false);
    assert_eq!(last_reported_period_id(&env, &issuer, &symbol_short!("ns"), &token), Some(2));
}
