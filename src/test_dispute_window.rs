//! # Dispute Window Time Boundary Matrix
//!
//! Hardens the dispute window enforcement for IssuerDispute freezes based on ledger time.
//!
//! ## Soroban Time Model (for integrators)
//!
//! Soroban uses `env.ledger().timestamp()` which returns the Unix timestamp (seconds
//! since epoch) of the **current ledger's close time**. This value is:
//! - Set by the Stellar network consensus; not manipulable by individual transactions.
//! - Monotonically non-decreasing across ledgers (guaranteed by the protocol).
//! - Available in tests via `env.ledger().with_mut(|l| l.timestamp = T)`.
//!
//! The dispute window is stored as `DisputeWindowSecs(OfferingId)` in seconds.
//! The check computes the deadline as: `period_close_timestamp + dispute_window_secs`.
//! The check is **inclusive on the deadline boundary**:
//!   `now <= period_close_timestamp + dispute_window_secs`
//!
//! ## Coverage Matrix
//!
//! ### Dispute Window Configuration
//! | Scenario | Expected |
//! |----------|----------|
//! | No window set | Returns DEFAULT_DISPUTE_WINDOW_SECS (30 days) |
//! | Window set to custom value | Returns configured value |
//! | Set by issuer | OK |
//! | Set by admin | OK |
//! | Set by unauthorized | NotAuthorized |
//! | Set on non-existent offering | OfferingNotFound |
//!
//! ### IssuerDispute Freeze Window Enforcement
//! | Scenario | now vs deadline | Expected |
//! |----------|----------------|----------|
//! | No period closed | any | OK (no deadline) |
//! | Period not closed | any | OK (no deadline) |
//! | now < deadline | before deadline | OK |
//! | now == deadline | at deadline | OK (inclusive) |
//! | now > deadline | after deadline | DisputeWindowClosed |
//! | Window = 0 (all rejected) | now > close_time | DisputeWindowClosed |
//! | Window = 0 (all rejected) | now == close_time | OK (inclusive) |
//! | Other freeze reasons | any | OK (window only applies to IssuerDispute) |
//!
//! ## Security / Risk Notes
//!
//! - **Reconfiguration race**: An issuer can change the dispute window while a freeze
//!   transaction is in-flight. The contract applies the window that is active at the
//!   ledger that closes the transaction — there is no "snapshot" of the window at
//!   submission time. Integrators must account for this.
//! - **Zero window**: A window of 0 seconds means disputes are only allowed at the
//!   exact second the period is closed. This is intentional but operationally fragile;
//!   issuers should prefer windows with meaningful duration.
//! - **No period closed**: If no period has been closed via `close_period`, there is
//!   no deadline to enforce, so disputes are always allowed. This is by design.
//! - **Timestamp source**: `env.ledger().timestamp()` is the only time source used.
//!   Wall-clock time or block numbers are NOT used.
//! - **Other freeze reasons**: The dispute window only applies to `IssuerDispute`.
//!   Other freeze reasons (Sanctions, CourtOrder, Manual) are not subject to this check.

#![cfg(test)]
#![allow(unused_imports)]

use crate::{FreezeReason, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::testutils::Events as _;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    token, Address, Env,
};

// ── Helpers ─────────────────────────────────────────────────────────────────────

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
fn setup_with_holder(
) -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address, Address) {
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
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &offering_token,
        &10_000, // 100% share pool
        &payment_token,
        &0,
        &symbol_short!(""),
        &0);
    mint(&env, &payment_token, &issuer, 10_000_000);
    RevoraRevenueShareClient::new(&env, &cid).set_holder_share(
        &issuer,
        &symbol_short!("ns"),
        &offering_token,
        &holder,
        &10_000,
    );

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
    client.deposit_revenue(issuer, &symbol_short!("ns"), token, payment_token, &amount, &period_id);
}

// ─── Constants ───────────────────────────────────────────────────────────────────

/// Default dispute window: 30 days in seconds.
const DEFAULT_DISPUTE_WINDOW_SECS: u64 = 2_592_000;

// ────────────────────────────────────────────────────────────────────────────────
// SECTION 1 — Dispute Window Configuration Tests
// ────────────────────────────────────────────────────────────────────────────────

/// get_dispute_window returns default when no window has been set.
#[test]
fn get_dispute_window_returns_default_when_unset() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0, &symbol_short!(""), &0);

    let window = client.get_dispute_window(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(window, DEFAULT_DISPUTE_WINDOW_SECS);
}

/// get_dispute_window returns configured value after set.
#[test]
fn get_dispute_window_returns_configured_value() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0, &symbol_short!(""), &0);

    client.set_dispute_window(&issuer, &symbol_short!("ns"), &token, &5_000_000);
    let window = client.get_dispute_window(&issuer, &symbol_short!("ns"), &token);
    assert_eq!(window, 5_000_000);
}

/// set_dispute_window by issuer succeeds.
#[test]
fn set_dispute_window_by_issuer_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0, &symbol_short!(""), &0);

    let r = client.try_set_dispute_window(&issuer, &symbol_short!("ns"), &token, &10_000);
    assert!(r.is_ok());
    assert_eq!(client.get_dispute_window(&issuer, &symbol_short!("ns"), &token), 10_000);
}

/// set_dispute_window by unauthorized caller fails.
#[test]
fn set_dispute_window_by_unauthorized_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0, &symbol_short!(""), &0);

    let unauthorized = Address::generate(&env);
    let r = client.try_set_dispute_window(&unauthorized, &symbol_short!("ns"), &token, &10_000);
    assert_eq!(r, Err(Ok(RevoraError::NotAuthorized)));
}

/// set_dispute_window on non-existent offering fails.
#[test]
fn set_dispute_window_on_nonexistent_offering_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let r = client.try_set_dispute_window(&issuer, &symbol_short!("ns"), &token, &10_000);
    assert_eq!(r, Err(Ok(RevoraError::OfferingNotFound)));
}

/// set_dispute_window emits an event.
#[test]
fn set_dispute_window_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    RevoraRevenueShareClient::new(&env, &cid).register_offering(&issuer, &symbol_short!("ns"), &token, &1_000, &token, &0, &symbol_short!(""), &0);

    let before = env.events().all().len();
    client.set_dispute_window(&issuer, &symbol_short!("ns"), &token, &12345);
    assert!(env.events().all().len() > before, "expected event to be emitted");
}

// ────────────────────────────────────────────────────────────────────────────────
// SECTION 2 — IssuerDispute Freeze Window Enforcement
// ────────────────────────────────────────────────────────────────────────────────

/// IssuerDispute freeze succeeds when no period is closed (no deadline).
#[test]
fn issuer_dispute_succeeds_when_no_period_closed() {
    let (env, client, issuer, token, _payment, holder) = setup_with_holder();

    // No period has been closed, so dispute should be allowed
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert!(r.is_ok(), "dispute should be allowed when no period is closed");
}

/// IssuerDispute freeze succeeds when period is deposited but not closed.
#[test]
fn issuer_dispute_succeeds_when_period_not_closed() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Deposit a period but don't close it
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);

    // Period is not closed, so dispute should be allowed
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert!(r.is_ok(), "dispute should be allowed when period is not closed");
}

/// IssuerDispute freeze succeeds before deadline.
#[test]
fn issuer_dispute_succeeds_before_deadline() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Set custom dispute window: 100 seconds
    client.set_dispute_window(&issuer, &symbol_short!("ns"), &token, &100);

    // Deposit and close period at T=1000
    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);
    client.close_period(&issuer, &symbol_short!("ns"), &token, &1);

    // Try dispute at T=1050 (50 seconds after close, before 100-second deadline)
    set_time(&env, 1_050);
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert!(r.is_ok(), "dispute should succeed before deadline");
}

/// IssuerDispute freeze succeeds at exact deadline (inclusive boundary).
#[test]
fn issuer_dispute_succeeds_at_deadline_inclusive() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Set custom dispute window: 100 seconds
    client.set_dispute_window(&issuer, &symbol_short!("ns"), &token, &100);

    // Deposit and close period at T=1000
    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);
    client.close_period(&issuer, &symbol_short!("ns"), &token, &1);

    // Try dispute at T=1100 (exactly at deadline)
    set_time(&env, 1_100);
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert!(r.is_ok(), "dispute should succeed at exact deadline (inclusive)");
}

/// IssuerDispute freeze fails after deadline.
#[test]
fn issuer_dispute_fails_after_deadline() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Set custom dispute window: 100 seconds
    client.set_dispute_window(&issuer, &symbol_short!("ns"), &token, &100);

    // Deposit and close period at T=1000
    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);
    client.close_period(&issuer, &symbol_short!("ns"), &token, &1);

    // Try dispute at T=1101 (1 second after deadline)
    set_time(&env, 1_101);
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert_eq!(r, Err(Ok(RevoraError::DisputeWindowClosed)));
}

/// IssuerDispute freeze with zero window only succeeds at exact close time.
#[test]
fn issuer_dispute_zero_window_succeeds_at_exact_close_time() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Set zero dispute window
    client.set_dispute_window(&issuer, &symbol_short!("ns"), &token, &0);

    // Deposit and close period at T=1000
    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);
    client.close_period(&issuer, &symbol_short!("ns"), &token, &1);

    // Try dispute at T=1000 (exact close time)
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert!(r.is_ok(), "zero window should allow dispute at exact close time");
}

/// IssuerDispute freeze with zero window fails 1 second after close.
#[test]
fn issuer_dispute_zero_window_fails_after_close() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Set zero dispute window
    client.set_dispute_window(&issuer, &symbol_short!("ns"), &token, &0);

    // Deposit and close period at T=1000
    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);
    client.close_period(&issuer, &symbol_short!("ns"), &token, &1);

    // Try dispute at T=1001 (1 second after close)
    set_time(&env, 1_001);
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert_eq!(r, Err(Ok(RevoraError::DisputeWindowClosed)));
}

/// Other freeze reasons are not subject to dispute window.
#[test]
fn other_freeze_reasons_bypass_dispute_window() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Set zero dispute window
    client.set_dispute_window(&issuer, &symbol_short!("ns"), &token, &0);

    // Deposit and close period at T=1000
    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);
    client.close_period(&issuer, &symbol_short!("ns"), &token, &1);

    // Try freeze with Sanctions reason at T=2000 (way past deadline)
    set_time(&env, 2_000);
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::Sanctions,
    );
    assert!(r.is_ok(), "Sanctions freeze should bypass dispute window");

    // Try freeze with CourtOrder reason
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::CourtOrder,
    );
    assert!(r.is_ok(), "CourtOrder freeze should bypass dispute window");

    // Try freeze with Manual reason
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::Manual,
    );
    assert!(r.is_ok(), "Manual freeze should bypass dispute window");
}

/// Dispute window uses default when not configured.
#[test]
fn dispute_window_uses_default_when_not_configured() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Don't set custom window - should use default (30 days = 2,592,000 seconds)

    // Deposit and close period at T=1000
    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);
    client.close_period(&issuer, &symbol_short!("ns"), &token, &1);

    // Try dispute at T=1000 + 2,592,000 - 1 = 2,591,999 (1 second before default deadline)
    set_time(&env, 1_000 + DEFAULT_DISPUTE_WINDOW_SECS - 1);
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert!(r.is_ok(), "dispute should succeed before default deadline");

    // Try dispute at T=1000 + 2,592,000 + 1 = 2,592,001 (1 second after default deadline)
    set_time(&env, 1_000 + DEFAULT_DISPUTE_WINDOW_SECS + 1);
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert_eq!(r, Err(Ok(RevoraError::DisputeWindowClosed)));
}

/// Dispute window can be reconfigured after being set.
#[test]
fn dispute_window_can_be_reconfigured() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Set initial window: 100 seconds
    client.set_dispute_window(&issuer, &symbol_short!("ns"), &token, &100);
    assert_eq!(client.get_dispute_window(&issuer, &symbol_short!("ns"), &token), 100);

    // Reconfigure to 500 seconds
    client.set_dispute_window(&issuer, &symbol_short!("ns"), &token, &500);
    assert_eq!(client.get_dispute_window(&issuer, &symbol_short!("ns"), &token), 500);

    // Deposit and close period at T=1000
    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);
    client.close_period(&issuer, &symbol_short!("ns"), &token, &1);

    // Try dispute at T=1200 (within new 500-second window, would have been past old 100-second window)
    set_time(&env, 1_200);
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert!(r.is_ok(), "dispute should succeed with reconfigured window");
}

/// Dispute window check only applies to the most recently closed period.
#[test]
fn dispute_window_checks_most_recent_closed_period() {
    let (env, client, issuer, token, payment_token, holder) = setup_with_holder();

    // Set dispute window: 100 seconds
    client.set_dispute_window(&issuer, &symbol_short!("ns"), &token, &100);

    // Deposit and close period 1 at T=1000
    set_time(&env, 1_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 1, 100_000);
    client.close_period(&issuer, &symbol_short!("ns"), &token, &1);

    // Deposit and close period 2 at T=2000
    set_time(&env, 2_000);
    deposit_period(&env, &client, &issuer, &token, &payment_token, 2, 100_000);
    client.close_period(&issuer, &symbol_short!("ns"), &token, &2);

    // Try dispute at T=2050 (within period 2's window, past period 1's window)
    set_time(&env, 2_050);
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert!(r.is_ok(), "dispute should succeed based on most recent period");

    // Try dispute at T=2150 (past period 2's window)
    set_time(&env, 2_150);
    let r = client.try_emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );
    assert_eq!(r, Err(Ok(RevoraError::DisputeWindowClosed)));
}
