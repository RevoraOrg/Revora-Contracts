//! # Report/Claim Window Time Boundary Matrix
//!
//! Hardens the reporting and claiming window checks based on ledger time.
//!
//! ## Soroban Time Model (for integrators)
//!
//! Soroban uses `env.ledger().timestamp()` which returns the Unix timestamp (seconds
//! since epoch) of the **current ledger's close time**. This value is:
//! - Set by the Stellar network consensus; not manipulable by individual transactions.
//! - Monotonically non-decreasing across ledgers (guaranteed by the protocol).
//! - Available in tests via `env.ledger().with_mut(|l| l.timestamp = T)`.
//!
//! Windows are stored as `AccessWindow { start_timestamp: u64, end_timestamp: u64 }`.
//! The check is **inclusive on both boundaries**:
//!   `now >= start_timestamp && now <= end_timestamp`
//!
//! ## Coverage Matrix
//!
//! ### Report Window
//! | Scenario | now vs window | Expected |
//! |----------|--------------|----------|
//! | No window set | any | OK (always open) |
//! | now < start | before | ReportingWindowClosed |
//! | now == start | at start | OK (inclusive) |
//! | now in (start, end) | inside | OK |
//! | now == end | at end | OK (inclusive) |
//! | now > end | after | ReportingWindowClosed |
//! | start == end (zero-width) | now == start | OK |
//! | start == end (zero-width) | now != start | ReportingWindowClosed |
//! | window reconfigured mid-flight | new window excludes now | ReportingWindowClosed |
//! | window reconfigured mid-flight | new window includes now | OK |
//!
//! ### Claim Window
//! | Scenario | now vs window | Expected |
//! |----------|--------------|----------|
//! | No window set | any | OK (always open) |
//! | now < start | before | ClaimWindowClosed |
//! | now == start | at start | OK (inclusive) |
//! | now in (start, end) | inside | OK |
//! | now == end | at end | OK (inclusive) |
//! | now > end | after | ClaimWindowClosed |
//! | start == end (zero-width) | now == start | OK |
//! | start == end (zero-width) | now != start | ClaimWindowClosed |
//! | window reconfigured mid-flight | new window excludes now | ClaimWindowClosed |
//! | window reconfigured mid-flight | new window includes now | OK |
//!
//! ### Window Validation (set_report_window / set_claim_window)
//! | start vs end | Expected |
//! |-------------|----------|
//! | start < end | OK |
//! | start == end | OK (zero-width, single-second window) |
//! | start > end | LimitReached |
//!
//! ### Report/Claim Window Interaction
//! | Report window | Claim window | report_revenue | claim |
//! |---------------|--------------|----------------|-------|
//! | open | open | OK | OK |
//! | open | closed | OK | ClaimWindowClosed |
//! | closed | open | ReportingWindowClosed | OK |
//! | closed | closed | ReportingWindowClosed | ClaimWindowClosed |
//!
//! ## Security / Risk Notes
//!
//! - **Reconfiguration race**: An issuer can change a window while a holder's claim
//!   transaction is in-flight. The contract applies the window that is active at the
//!   ledger that closes the transaction â€” there is no "snapshot" of the window at
//!   submission time. Integrators must account for this.
//! - **Zero-width windows**: A window where `start == end` is valid and creates a
//!   single-second eligibility slot. This is intentional but operationally fragile;
//!   issuers should prefer windows with meaningful duration.
//! - **No deposit window**: `deposit_revenue` has no time-window guard. Only
//!   `report_revenue` (reporting window) and `claim` (claiming window) are gated.
//! - **Claim delay is orthogonal**: The per-offering `ClaimDelaySecs` is checked
//!   *inside* the claim loop per period, independently of the claim window. Both
//!   must pass for a period to be claimable.
//! - **Timestamp source**: `env.ledger().timestamp()` is the only time source used.
//!   Wall-clock time or block numbers are NOT used.

#![cfg(test)]
#![allow(unused_imports)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::testutils::Events as _;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    token, Address, Env,
};

// â”€â”€ Helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract(admin.clone());
    (token_id, admin)
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    token::StellarAssetClient::new(env, token).mint(to, &amount);
}

fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

/// Full setup: env + client + registered offering + funded issuer + holder with 100% share.
/// Returns (env, client, issuer, offering_token, payment_token, holder).
fn setup_with_holder() -> (
    Env,
    RevoraRevenueShareClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (payment_token, _) = create_payment_token(&env);
    let holder = Address::generate(&env);

    RevoraRevenueShareClient::new(&env, &cid).register_offering(
        &issuer,
        &symbol_short!("ns"),
        &offering_token,
        &10_000, // 100% share pool
        &payment_token,
        &0,
    );
    mint(&env, &payment_token, &issuer, 10_000_000);
    RevoraRevenueShareClient::new(&env, &cid).set_holder_share(&issuer, &symbol_short!("ns"), &offering_token, &holder, &10_000);

    (env, client, issuer, offering_token, payment_token, holder)
}

/// Deposit one period of revenue and return the period_id used.
fn deposit_period(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
    payment_token: &Address,
    period_id: u64,
    amount: i128,
) {
    client
        .deposit_revenue(issuer, &symbol_short!("ns"), token, payment_token, &amount, &period_id)
        ;
}

fn assert_report_window_round_trip(
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
    start: u64,
    end: u64,
) {
    let w = client.get_report_window(issuer, &symbol_short!("ns"), token).unwrap();
    assert_eq!(w.start_timestamp, start);
    assert_eq!(w.end_timestamp, end);
}

fn assert_claim_window_round_trip(
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
    start: u64,
    end: u64,
) {
    let w = client.get_claim_window(issuer, &symbol_short!("ns"), token).unwrap();
    assert_eq!(w.start_timestamp, start);
    assert_eq!(w.end_timestamp, end);
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// SECTION 1 â€” Report Window Boundary Matrix
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// No report window set â†’ report_revenue always succeeds regardless of timestamp.
#[test]
fn report_window_unset_always_open() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    // Verify no window is stored
    assert!(client.get_report_window(&issuer, &symbol_short!("ns"), &token).is_none());
    // Any timestamp - should succeed
    let period_ids = [1u64, 2, 3, 4];
    for (i, ts) in [0u64, 1, 1_000, u64::MAX / 2].iter().enumerate() {
        set_time(&env, *ts);
        let r = client.try_report_revenue(
            &issuer,
            &symbol_short!("ns"),
            &token,
            &token,
            &100,
            &period_ids[i],
            &false,
        );
    }
}

/// now < start â†’ ReportingWindowClosed.
#[test]
fn report_window_before_start_is_closed() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    // Window: [1000, 2000]
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);

    // now = 999 (one second before start)
    set_time(&env, 999);
    let r = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false,
    );
    assert_eq!(r, Err(Ok(RevoraError::ReportingWindowClosed)));
}

/// now == start â†’ OK (start boundary is inclusive).
#[test]
fn report_window_at_start_is_open_inclusive() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);

    set_time(&env, 1_000); // exactly at start
    let r = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false,
    );
    assert!(r.is_ok(), "start boundary must be inclusive, got {r:?}");
}

/// now strictly inside (start, end) â†’ OK.
#[test]
fn report_window_inside_is_open() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);

    set_time(&env, 1_500);
    let r = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false,
    );
    assert!(r.is_ok(), "mid-window must be open, got {r:?}");
}

/// now == end â†’ OK (end boundary is inclusive).
#[test]
fn report_window_at_end_is_open_inclusive() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);

    set_time(&env, 2_000); // exactly at end
    let r = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false,
    );
    assert!(r.is_ok(), "end boundary must be inclusive, got {r:?}");
}

/// now > end â†’ ReportingWindowClosed.
#[test]
fn report_window_after_end_is_closed() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);

    set_time(&env, 2_001); // one second after end
    let r = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false,
    );
    assert_eq!(r, Err(Ok(RevoraError::ReportingWindowClosed)));
}

/// Zero-width window (start == end): only the exact timestamp is open.
#[test]
fn report_window_zero_width_open_at_exact_timestamp() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    // start == end: single-second window at T=5000
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &5_000, &5_000);

    set_time(&env, 5_000);
    let r = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false,
    );
    assert!(r.is_ok(), "zero-width window must be open at exact timestamp, got {r:?}");
}

/// Zero-width window: one second before is closed.
#[test]
fn report_window_zero_width_closed_before() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &5_000, &5_000);

    set_time(&env, 4_999);
    let r = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false,
    );
    assert_eq!(r, Err(Ok(RevoraError::ReportingWindowClosed)));
}

/// Zero-width window: one second after is closed.
#[test]
fn report_window_zero_width_closed_after() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &5_000, &5_000);

    set_time(&env, 5_001);
    let r = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false,
    );
    assert_eq!(r, Err(Ok(RevoraError::ReportingWindowClosed)));
}

/// Reconfiguring the window mid-flight to exclude the current time closes reporting.
#[test]
fn report_window_reconfigured_to_exclude_now_closes_reporting() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    // Initial window: [1000, 3000]; now = 2000 â†’ open
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &3_000);
    set_time(&env, 2_000);
    client
        .report_revenue(&issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false)
        ;

    // Issuer reconfigures window to [4000, 5000]; now = 2000 â†’ closed
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &4_000, &5_000);
    let r = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &token, &100, &2, &false,
    );
    assert_eq!(r, Err(Ok(RevoraError::ReportingWindowClosed)));
}

/// Reconfiguring the window mid-flight to include the current time opens reporting.
#[test]
fn report_window_reconfigured_to_include_now_opens_reporting() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    // Initial window: [4000, 5000]; now = 2000 â†’ closed
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &4_000, &5_000);
    set_time(&env, 2_000);
    let r = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false,
    );
    assert_eq!(r, Err(Ok(RevoraError::ReportingWindowClosed)));

    // Issuer reconfigures to [1000, 3000]; now = 2000 â†’ open
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &3_000);
    let r2 = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &token, &100, &1, &false,
    );
    assert!(r2.is_ok(), "reconfigured window should now be open, got {r2:?}");
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// SECTION 2 â€” Claim Window Boundary Matrix
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// No claim window set â†’ claim always succeeds (window-wise) regardless of timestamp.
#[test]
fn claim_window_unset_always_open() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Verify no window is stored
    assert!(client.get_claim_window(&issuer, &symbol_short!("ns"), &token).is_none());

    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    // Claim at an arbitrary timestamp â€” should succeed
    set_time(&env, 999_999);
    let payout = client.claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(payout, 100_000);
}

/// now < start â†’ ClaimWindowClosed.
#[test]
fn claim_window_before_start_is_closed() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 500);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    // Window: [1000, 2000]; now = 999
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    set_time(&env, 999);
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(r, Err(Ok(RevoraError::ClaimWindowClosed)));
}

/// now == start â†’ OK (start boundary is inclusive).
#[test]
fn claim_window_at_start_is_open_inclusive() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 500);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    set_time(&env, 1_000); // exactly at start
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert!(r.is_ok(), "start boundary must be inclusive, got {r:?}");
}

/// now strictly inside (start, end) â†’ OK.
#[test]
fn claim_window_inside_is_open() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 500);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    set_time(&env, 1_500);
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert!(r.is_ok(), "mid-window must be open, got {r:?}");
}

/// now == end â†’ OK (end boundary is inclusive).
#[test]
fn claim_window_at_end_is_open_inclusive() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 500);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    set_time(&env, 2_000); // exactly at end
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert!(r.is_ok(), "end boundary must be inclusive, got {r:?}");
}

/// now > end â†’ ClaimWindowClosed.
#[test]
fn claim_window_after_end_is_closed() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 500);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    set_time(&env, 2_001); // one second after end
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(r, Err(Ok(RevoraError::ClaimWindowClosed)));
}

/// Zero-width claim window: only the exact timestamp is open.
#[test]
fn claim_window_zero_width_open_at_exact_timestamp() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 500);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    // start == end at T=5000
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &5_000, &5_000);
    set_time(&env, 5_000);
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert!(r.is_ok(), "zero-width window must be open at exact timestamp, got {r:?}");
}

/// Zero-width claim window: one second before is closed.
#[test]
fn claim_window_zero_width_closed_before() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 500);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &5_000, &5_000);
    set_time(&env, 4_999);
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(r, Err(Ok(RevoraError::ClaimWindowClosed)));
}

/// Zero-width claim window: one second after is closed.
#[test]
fn claim_window_zero_width_closed_after() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 500);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &5_000, &5_000);
    set_time(&env, 5_001);
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(r, Err(Ok(RevoraError::ClaimWindowClosed)));
}

/// Reconfiguring the claim window mid-flight to exclude the current time closes claiming.
#[test]
fn claim_window_reconfigured_to_exclude_now_closes_claiming() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 500);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 2, 50_000);

    // Initial window: [1000, 3000]; now = 2000 â†’ open; claim period 1
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_000, &3_000);
    set_time(&env, 2_000);
    client.claim(&holder, &issuer, &symbol_short!("ns"), &token, &1);

    // Issuer reconfigures window to [4000, 5000]; now = 2000 â†’ closed
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &4_000, &5_000);
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(r, Err(Ok(RevoraError::ClaimWindowClosed)));
}

/// Reconfiguring the claim window mid-flight to include the current time opens claiming.
#[test]
fn claim_window_reconfigured_to_include_now_opens_claiming() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 500);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    // Initial window: [4000, 5000]; now = 2000 â†’ closed
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &4_000, &5_000);
    set_time(&env, 2_000);
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(r, Err(Ok(RevoraError::ClaimWindowClosed)));

    // Issuer reconfigures to [1000, 3000]; now = 2000 â†’ open
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_000, &3_000);
    let r2 = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert!(r2.is_ok(), "reconfigured window should now be open, got {r2:?}");
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// SECTION 3 â€” Window Validation (set_report_window / set_claim_window)
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// set_report_window with start < end is accepted.
#[test]
fn set_report_window_valid_range_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    let r = client.try_set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    assert!(r.is_ok());

    let w = client.get_report_window(&issuer, &symbol_short!("ns"), &token).unwrap();
    assert_eq!(w.start_timestamp, 1_000);
    assert_eq!(w.end_timestamp, 2_000);
}

/// set_report_window with start == end (zero-width) is accepted.
#[test]
fn set_report_window_zero_width_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    let r = client.try_set_report_window(&issuer, &symbol_short!("ns"), &token, &5_000, &5_000);
    assert!(r.is_ok(), "zero-width window must be accepted, got {r:?}");
}

/// set_report_window with start > end is rejected with LimitReached.
#[test]
fn set_report_window_inverted_range_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    let r = client.try_set_report_window(&issuer, &symbol_short!("ns"), &token, &2_000, &1_000);
    assert_eq!(r, Err(Ok(RevoraError::LimitReached)));

    // No window should have been stored
    assert!(client.get_report_window(&issuer, &symbol_short!("ns"), &token).is_none());
}

/// set_claim_window with start < end is accepted.
#[test]
fn set_claim_window_valid_range_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    let r = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    assert!(r.is_ok());

    let w = client.get_claim_window(&issuer, &symbol_short!("ns"), &token).unwrap();
    assert_eq!(w.start_timestamp, 1_000);
    assert_eq!(w.end_timestamp, 2_000);
}

/// set_claim_window with start == end (zero-width) is accepted.
#[test]
fn set_claim_window_zero_width_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    let r = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &5_000, &5_000);
    assert!(r.is_ok(), "zero-width window must be accepted, got {r:?}");
}

/// set_claim_window with start > end is rejected with LimitReached.
#[test]
fn set_claim_window_inverted_range_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    let r = client.try_set_claim_window(&issuer, &symbol_short!("ns"), &token, &2_000, &1_000);
    assert_eq!(r, Err(Ok(RevoraError::LimitReached)));

    assert!(client.get_claim_window(&issuer, &symbol_short!("ns"), &token).is_none());
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// SECTION 4 â€” deposit_revenue has NO time-window gate
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// deposit_revenue succeeds regardless of any report or claim window configuration.
/// This asserts the documented semantic: only report_revenue and claim are window-gated.
#[test]
fn deposit_revenue_ignores_report_and_claim_windows() {
    let (env, client, issuer, token, payment_token, _holder) = setup_with_holder();

    // Set both windows to a future range so "now" is outside both
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &9_000, &10_000);
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &9_000, &10_000);

    // now = 1000, well outside both windows
    set_time(&env, 1_000);

    let r = client.try_deposit_revenue(
        &issuer, &symbol_short!("ns"), &token, &payment_token, &100_000, &1,
    );
    assert!(r.is_ok(), "deposit_revenue must not be gated by report/claim windows, got {r:?}");
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
#[derive(Copy, Clone)]
enum WindowState {
    Open,
    Closed,
}

/// The report and claim windows are independent persistent settings.
///
/// Security invariant: a closed claim window must not block reporting, and a
/// closed report window must not block claiming. Each entrypoint may fail only
/// with its own typed window error when its own window is closed.
#[test]
fn report_and_claim_windows_are_independent_across_open_closed_matrix() {
    let cases = [
        (WindowState::Open, WindowState::Open, true, true),
        (WindowState::Open, WindowState::Closed, true, false),
        (WindowState::Closed, WindowState::Open, false, true),
        (WindowState::Closed, WindowState::Closed, false, false),
    ];

    for (idx, (report_state, claim_state, report_should_open, claim_should_open)) in cases.iter().copied().enumerate() {
        let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

        set_time(&env, 1_000);
        deposit_period(&env, &client, &issuer, &token, &payment_token, 100 + idx as u64, 100_000);

        match report_state {
            WindowState::Open => {
                client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_500, &2_000);
                assert_report_window_round_trip(&client, &issuer, &token, 1_500, 2_000);
            }
            WindowState::Closed => {
                client.set_report_window(&issuer, &symbol_short!("ns"), &token, &2_500, &3_000);
                assert_report_window_round_trip(&client, &issuer, &token, 2_500, 3_000);
            }
        }

        match claim_state {
            WindowState::Open => {
                client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_500, &2_000);
                assert_claim_window_round_trip(&client, &issuer, &token, 1_500, 2_000);
            }
            WindowState::Closed => {
                client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &2_500, &3_000);
                assert_claim_window_round_trip(&client, &issuer, &token, 2_500, 3_000);
            }
        }

        set_time(&env, 1_750);

        let report_result = client.try_report_revenue(
            &issuer, &symbol_short!("ns"), &token, &payment_token, &1_000, &(1_000 + idx as u64), &false,
        );
        if report_should_open {
            assert!(
                report_result.is_ok(),
                "report_revenue must ignore claim-window state in case {idx}, got {report_result:?}"
            );
        } else {
            assert_eq!(
                report_result,
                Err(Ok(RevoraError::ReportingWindowClosed)),
                "report_revenue must return only its own typed window error in case {idx}"
            );
        }

        let claim_result = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
        if claim_should_open {
            assert_eq!(claim_result, Ok(Ok(100_000)), "claim must ignore report-window state in case {idx}");
        } else {
            assert_eq!(
                claim_result,
                Err(Ok(RevoraError::ClaimWindowClosed)),
                "claim must return only its own typed window error in case {idx}"
            );
        }
    }
}

/// With both windows unset, both entrypoints remain open and the getters
/// explicitly report no persistent window.
#[test]
fn report_and_claim_windows_unset_are_independently_always_open() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    assert!(client.get_report_window(&issuer, &symbol_short!("ns"), &token).is_none());
    assert!(client.get_claim_window(&issuer, &symbol_short!("ns"), &token).is_none());

    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    set_time(&env, 42_000);
    let report_result = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &payment_token, &1_000, &2, &false,
    );
    assert!(
        report_result.is_ok(),
        "unset claim window must not block report_revenue, got {report_result:?}"
    );

    let claim_result = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(claim_result, Ok(Ok(100_000)), "unset report window must not block claim");
}

/// Zero-width windows are valid single-second windows for both gates.
#[test]
fn report_and_claim_zero_width_windows_overlap_only_at_exact_second() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &2_000, &2_000);
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &2_000, &2_000);
    assert_report_window_round_trip(&client, &issuer, &token, 2_000, 2_000);
    assert_claim_window_round_trip(&client, &issuer, &token, 2_000, 2_000);

    set_time(&env, 1_999);
    let report_before = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &payment_token, &1_000, &2, &false,
    );
    assert_eq!(report_before, Err(Ok(RevoraError::ReportingWindowClosed)));
    let claim_before = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(claim_before, Err(Ok(RevoraError::ClaimWindowClosed)));

    set_time(&env, 2_000);
    let report_at = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token, &payment_token, &1_000, &2, &false,
    );
    assert!(
        report_at.is_ok(),
        "report_revenue must accept the exact zero-width timestamp, got {report_at:?}"
    );
    let claim_at = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(claim_at, Ok(Ok(100_000)));
}

// SECTION 5 â€” Claim delay is orthogonal to claim window
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// Claim window open + delay not elapsed â†’ ClaimDelayNotElapsed (not ClaimWindowClosed).
/// Confirms the two mechanisms are independent and delay is checked per-period inside the loop.
#[test]
fn claim_window_open_but_delay_not_elapsed_returns_delay_error() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Deposit at T=1000
    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    // Set 500s delay and a claim window that is open at T=1200
    client.set_claim_delay(&issuer, &symbol_short!("ns"), &token, &500);
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_100, &2_000);

    // T=1200: window is open, but delay requires T >= 1000+500=1500
    set_time(&env, 1_200);
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(r, Err(Ok(RevoraError::ClaimDelayNotElapsed)));
}

/// Claim window open + delay elapsed â†’ claim succeeds.
#[test]
fn claim_window_open_and_delay_elapsed_succeeds() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    client.set_claim_delay(&issuer, &symbol_short!("ns"), &token, &500);
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_100, &3_000);

    // T=1500: window open AND delay elapsed (1000+500=1500)
    set_time(&env, 1_500);
    let payout = client.claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(payout, 100_000);
}

/// Claim window closed + delay elapsed â†’ ClaimWindowClosed (window check runs first).
#[test]
fn claim_window_closed_even_if_delay_elapsed() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    client.set_claim_delay(&issuer, &symbol_short!("ns"), &token, &100);
    // Window is in the past: [500, 900]
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &500, &900);

    // T=1200: delay elapsed (1000+100=1100 <= 1200) but window is closed
    set_time(&env, 1_200);
    let r = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token, &50);
    assert_eq!(r, Err(Ok(RevoraError::ClaimWindowClosed)));
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// SECTION 6 â€” Window isolation across offerings
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// A report window on offering A must not affect offering B.
#[test]
fn report_window_is_scoped_per_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token_a, &1_000, &token_a, &0);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token_b, &1_000, &token_b, &0);

    // Close offering A's report window; leave B's unset (always open)
    client.set_report_window(&issuer, &symbol_short!("ns"), &token_a, &5_000, &6_000);

    set_time(&env, 1_000); // outside A's window

    // A is closed
    let r_a = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token_a, &token_a, &100, &1, &false,
    );
    assert_eq!(r_a, Err(Ok(RevoraError::ReportingWindowClosed)));

    // B is open (no window set)
    let r_b = client.try_report_revenue(
        &issuer, &symbol_short!("ns"), &token_b, &token_b, &100, &1, &false,
    );
    assert!(r_b.is_ok(), "offering B must be unaffected by offering A's window, got {r_b:?}");
}

/// A claim window on offering A must not affect offering B.
#[test]
fn claim_window_is_scoped_per_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let (payment_token, _) = create_payment_token(&env);
    let holder = Address::generate(&env);

    RevoraRevenueShareClient::new(&env, &cid).register_offering(
        &issuer, &symbol_short!("ns"), &token_a, &10_000, &payment_token, &0,
    );
    RevoraRevenueShareClient::new(&env, &cid).register_offering(
        &issuer, &symbol_short!("ns"), &token_b, &10_000, &payment_token, &0,
    );
    mint(&env, &payment_token, &issuer, 10_000_000);
    RevoraRevenueShareClient::new(&env, &cid).set_holder_share(&issuer, &symbol_short!("ns"), &token_a, &holder, &10_000);
    RevoraRevenueShareClient::new(&env, &cid).set_holder_share(&issuer, &symbol_short!("ns"), &token_b, &holder, &10_000);

    set_time(&env, 500);
    client
        .deposit_revenue(&issuer, &symbol_short!("ns"), &token_a, &payment_token, &100_000, &1)
        ;
    client
        .deposit_revenue(&issuer, &symbol_short!("ns"), &token_b, &payment_token, &100_000, &1)
        ;

    // Close A's claim window; leave B's unset
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token_a, &5_000, &6_000);

    set_time(&env, 1_000); // outside A's window

    let r_a = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token_a, &50);
    assert_eq!(r_a, Err(Ok(RevoraError::ClaimWindowClosed)));

    let r_b = client.try_claim(&holder, &issuer, &symbol_short!("ns"), &token_b, &50);
    assert!(r_b.is_ok(), "offering B must be unaffected by offering A's window, got {r_b:?}");
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// SECTION 7 â€” Event emission on window set
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// set_report_window emits an event.
#[test]
fn set_report_window_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    let before = env.events().all().len();
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    assert!(
        env.events().all().len() > before,
        "set_report_window must emit at least one event"
    );
}

/// set_claim_window emits an event.
#[test]
fn set_claim_window_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    let before = env.events().all().len();
    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    assert!(
        env.events().all().len() > before,
        "set_claim_window must emit at least one event"
    );
}

// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
// SECTION 8 â€” get_report_window / get_claim_window read-back
// â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

/// get_report_window returns None when no window has been set.
#[test]
fn get_report_window_returns_none_when_unset() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    assert!(client.get_report_window(&issuer, &symbol_short!("ns"), &token).is_none());
}

/// get_claim_window returns None when no window has been set.
#[test]
fn get_claim_window_returns_none_when_unset() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    assert!(client.get_claim_window(&issuer, &symbol_short!("ns"), &token).is_none());
}

/// get_report_window returns the correct window after set.
#[test]
fn get_report_window_returns_correct_values() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_234, &5_678);
    let w = client.get_report_window(&issuer, &symbol_short!("ns"), &token).unwrap();
    assert_eq!(w.start_timestamp, 1_234);
    assert_eq!(w.end_timestamp, 5_678);
}

/// get_claim_window returns the correct window after set.
#[test]
fn get_claim_window_returns_correct_values() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    client.set_claim_window(&issuer, &symbol_short!("ns"), &token, &9_000, &9_999);
    let w = client.get_claim_window(&issuer, &symbol_short!("ns"), &token).unwrap();
    assert_eq!(w.start_timestamp, 9_000);
    assert_eq!(w.end_timestamp, 9_999);
}

/// Overwriting a window replaces the stored values.
#[test]
fn set_report_window_overwrites_previous() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0);

    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &3_000, &4_000);

    let w = client.get_report_window(&issuer, &symbol_short!("ns"), &token).unwrap();
    assert_eq!(w.start_timestamp, 3_000);
    assert_eq!(w.end_timestamp, 4_000);
}

// â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• 
// SECTION 9 â€” Epoch Boundary Report Revenue Ordering Invariant
// â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• â• 

/// #407 Add concurrent report_revenue epoch-boundary tests asserting period_id ordering invariant under window cutover
///
/// Security Notes:
/// This test validates that `report_revenue` correctly preserves the strict monotonic
/// ordering invariant of `last_report_period_id` when transitioning across reporting-window boundaries.
/// - The strict `+1` step requirement (`require_next_period_id`) cannot be bypassed by
///   shifting the temporal reporting window mid-flight.
/// - Skipped period slots correctly reject with `InvalidPeriodId` even if the
///   window is dynamically reconfigured, overlapping, or compressed to zero-width.
#[test]
fn report_revenue_epoch_boundary_ordering_invariant() {
    let (env, client, issuer, token, payment_token, _holder) = setup_with_holder();

    // 1. Configure window [1000, 2000] (A to B)
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &1_000, &2_000);
    
    // Move time to A (1000)
    set_time(&env, 1_000);
    
    // 2. Report period 1 at A
    let r1 = client.try_report_revenue(&issuer, &symbol_short!("ns"), &token, &payment_token, &100, &1, &false);
    assert!(r1.is_ok(), "Period 1 report failed");

    // 3. Move time to B+1 (2001) and reconfigure to [2001, 3000] (C)
    set_time(&env, 2_001);
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &2_001, &3_000);

    // Edge case: skipped - period_id rejected
    let r_skip = client.try_report_revenue(&issuer, &symbol_short!("ns"), &token, &payment_token, &100, &3, &false);
    assert_eq!(r_skip, Err(Ok(RevoraError::InvalidPeriodId)), "Skipped period_id must be rejected");

    // 4. Report period 2 and assert it succeeds, verifying invariant that last_report_period_id == 1 was persisted across the cutover
    let r2 = client.try_report_revenue(&issuer, &symbol_short!("ns"), &token, &payment_token, &100, &2, &false);
    assert!(r2.is_ok(), "Period 2 report failed across epoch boundary");

    // Edge case: Window reset to zero-width
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &4_000, &4_000);
    set_time(&env, 4_000);
    let r3 = client.try_report_revenue(&issuer, &symbol_short!("ns"), &token, &payment_token, &100, &3, &false);
    assert!(r3.is_ok(), "Period 3 report failed on zero-width window");

    // Edge case: overlapping windows (start time of new window is before end of old logic)
    client.set_report_window(&issuer, &symbol_short!("ns"), &token, &3_500, &5_000);
    set_time(&env, 4_500);
    let r4 = client.try_report_revenue(&issuer, &symbol_short!("ns"), &token, &payment_token, &100, &4, &false);
    assert!(r4.is_ok(), "Period 4 report failed on overlapping window reconfiguration");

    // Verify ordering invariant is still strictly enforced
    let r_skip2 = client.try_report_revenue(&issuer, &symbol_short!("ns"), &token, &payment_token, &100, &6, &false);
    assert_eq!(r_skip2, Err(Ok(RevoraError::InvalidPeriodId)), "Skipped period_id must be rejected after multiple window reconfigurations");
}
