//! TWAP window configurability — boundary, authorization, and event tests (#546).
//!
//! Covers:
//! - Rejection of values below `MIN_TWAP_WINDOW_SECS` and above `MAX_TWAP_WINDOW_SECS`
//! - Acceptance at exact `min`, exact `max`, and several interior values
//! - Authorization boundary (admin, primary issuer, stranger)
//! - Idempotency / reconfiguration
//! - Event emission (`EVENT_TWAP_WINDOW_SET` fires on every successful set)
//! - `get_twap_window` returns `None` when unset and `Some(...)` after `set`
//! - Audit fields (`set_at`, `set_by`) populated correctly

#![allow(clippy::unwrap_used)]

use crate::{
    MAX_TWAP_WINDOW_SECS, MIN_TWAP_WINDOW_SECS, RevoraError, RevoraRevenueShare,
    RevoraRevenueShareClient,
};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 1 — Setup helpers (mirrors test_dispute_window.rs layout)
// ─────────────────────────────────────────────────────────────────────────────

fn set_time(env: &Env, ts: u64) {
    env.ledger().with_mut(|l| l.timestamp = ts);
}

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Register a single-issuer (1-of-1 quorum) offering. Returns `(client, issuer, token)`.
fn setup_offering(env: &Env) -> (RevoraRevenueShareClient<'_>, Address, Address) {
    env.mock_all_auths();
    let client = make_client(env);
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let payout = Address::generate(env);
    let co_issuers: Vec<Address> = Vec::new(env);
    client.register_offering(
        &issuer,
        &co_issuers,
        &1u32,
        &symbol_short!("ns"),
        &token,
        &10_000u32, // 100% share pool
        &payout,
        &0i128, // no supply cap
        &symbol_short!(""),
        &0u32,
    );
    (client, issuer, token)
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 2 — Bounds rejection
// ─────────────────────────────────────────────────────────────────────────────

/// Below minimum is rejected.
#[test]
fn set_twap_window_rejects_below_min() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let r = client.try_set_twap_window(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &(MIN_TWAP_WINDOW_SECS - 1),
    );
    assert_eq!(r, Err(Ok(RevoraError::TwapWindowOutOfBounds)));
}

/// Exactly at minimum is accepted.
#[test]
fn set_twap_window_accepts_exact_min() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let r = client.try_set_twap_window(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &MIN_TWAP_WINDOW_SECS,
    );
    assert!(r.is_ok(), "exact min must succeed, got {r:?}");
    let stored = client
        .get_twap_window(&issuer, &symbol_short!("ns"), &token)
        .unwrap();
    assert_eq!(stored.window_secs, MIN_TWAP_WINDOW_SECS);
    assert_eq!(stored.set_by, issuer);
    assert!(stored.set_at > 0, "set_at must be populated");
    assert!(stored.set_at <= env.ledger().timestamp());
}

/// One second above minimum is accepted.
#[test]
fn set_twap_window_accepts_one_above_min() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let r = client.try_set_twap_window(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &(MIN_TWAP_WINDOW_SECS + 1),
    );
    assert!(r.is_ok());
}

/// Exactly at maximum is accepted.
#[test]
fn set_twap_window_accepts_exact_max() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let r = client.try_set_twap_window(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &MAX_TWAP_WINDOW_SECS,
    );
    assert!(r.is_ok(), "exact max must succeed, got {r:?}");
    let stored = client
        .get_twap_window(&issuer, &symbol_short!("ns"), &token)
        .unwrap();
    assert_eq!(stored.window_secs, MAX_TWAP_WINDOW_SECS);
}

/// One second above maximum is rejected.
#[test]
fn set_twap_window_rejects_one_above_max() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let r = client.try_set_twap_window(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &(MAX_TWAP_WINDOW_SECS + 1),
    );
    assert_eq!(r, Err(Ok(RevoraError::TwapWindowOutOfBounds)));
}

/// Zero is rejected (zero lies below min).
#[test]
fn set_twap_window_rejects_zero() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let r = client.try_set_twap_window(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &0u64,
    );
    assert_eq!(r, Err(Ok(RevoraError::TwapWindowOutOfBounds)));
}

/// u64::MAX is rejected (way above max).
#[test]
fn set_twap_window_rejects_u64_max() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let r = client.try_set_twap_window(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &u64::MAX,
    );
    assert_eq!(r, Err(Ok(RevoraError::TwapWindowOutOfBounds)));
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 3 — Interior / representative values
// ─────────────────────────────────────────────────────────────────────────────

/// 1 hour (3600s) — typical short-horizon smoothing.
#[test]
fn set_twap_window_accepts_one_hour() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let r = client.try_set_twap_window(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &3_600u64,
    );
    assert!(r.is_ok());
}

/// 7 days (a common smoothing horizon for private credit).
#[test]
fn set_twap_window_accepts_one_week() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let one_week = 7 * 24 * 60 * 60u64;
    let r = client.try_set_twap_window(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &one_week,
    );
    assert!(r.is_ok());
    let stored = client
        .get_twap_window(&issuer, &symbol_short!("ns"), &token)
        .unwrap();
    assert_eq!(stored.window_secs, one_week);
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 4 — Authorization
// ─────────────────────────────────────────────────────────────────────────────

/// A stranger is rejected with NotAuthorized, not TwapWindowOutOfBounds.
#[test]
fn set_twap_window_by_stranger_rejected() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let stranger = Address::generate(&env);
    let r = client.try_set_twap_window(
        &stranger,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &3_600u64,
    );
    assert_eq!(r, Err(Ok(RevoraError::NotAuthorized)));
}

/// Bounds check runs before auth + existence checks: a stranger with an
/// out-of-range value is rejected with `TwapWindowOutOfBounds`, not with
/// `OfferingNotFound` or `NotAuthorized`. This prevents an adversary from
/// probing offering existence or auth-failure state through the bound path.
#[test]
fn set_twap_window_bounds_check_runs_before_existence_and_auth() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let stranger = Address::generate(&env);
    let r = client.try_set_twap_window(
        &stranger,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &0u64,
    );
    assert_eq!(r, Err(Ok(RevoraError::TwapWindowOutOfBounds)));
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 5 — Offering existence
// ─────────────────────────────────────────────────────────────────────────────

/// Setting a window for an unregistered offering fails.
#[test]
fn set_twap_window_for_missing_offering_rejected() {
    let env = Env::default();
    let (client, _issuer, _token) = setup_offering(&env);
    set_time(&env, 1_000);
    let phantom_issuer = Address::generate(&env);
    let phantom_token = Address::generate(&env);
    let r = client.try_set_twap_window(
        &phantom_issuer,
        &phantom_issuer,
        &symbol_short!("ghost"),
        &phantom_token,
        &3_600u64,
    );
    assert_eq!(r, Err(Ok(RevoraError::OfferingNotFound)));
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 6 — Idempotency / reconfiguration
// ─────────────────────────────────────────────────────────────────────────────

/// set_twap_window is write-overwrite: a second call replaces the previous
/// config and updates `set_at` accordingly.
#[test]
fn set_twap_window_overwrite_returns_latest() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 5_000);
    client.set_twap_window(&issuer, &issuer, &symbol_short!("ns"), &token, &60u64);

    set_time(&env, 6_000);
    client.set_twap_window(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &7_200u64,
    );

    let stored = client
        .get_twap_window(&issuer, &symbol_short!("ns"), &token)
        .unwrap();
    assert_eq!(stored.window_secs, 7_200);
    assert!(stored.set_at >= 6_000);
    assert_eq!(stored.set_by, issuer);
}

/// get_twap_window returns None when no config has been set.
#[test]
fn get_twap_window_returns_none_when_unset() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let cfg = client.get_twap_window(&issuer, &symbol_short!("ns"), &token);
    assert!(cfg.is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 7 — Event emission
// ─────────────────────────────────────────────────────────────────────────────

/// set_twap_window emits at least one event on success.
#[test]
fn set_twap_window_emits_event() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let before = env.events().all().len();
    client.set_twap_window(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &3_600u64,
    );
    let after = env.events().all().len();
    assert!(after > before, "expected EVENT_TWAP_WINDOW_SET to be published");
}

/// Two calls emit two events (audit trail captures every reconfigure).
#[test]
fn set_twap_window_emits_event_on_each_reconfigure() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    set_time(&env, 1_000);
    let before = env.events().all().len();
    client.set_twap_window(&issuer, &issuer, &symbol_short!("ns"), &token, &120u64);
    let mid = env.events().all().len();
    client.set_twap_window(&issuer, &issuer, &symbol_short!("ns"), &token, &240u64);
    let after = env.events().all().len();
    assert!(mid > before, "first set must publish");
    assert!(after > mid, "second set must also publish");
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 8 — Administrator access path
// ─────────────────────────────────────────────────────────────────────────────

/// A caller that is NOT the primary issuer but IS the global admin can still
/// configure the TWAP window. Mirrors the dispute-window model: admin has
/// oversight on per-offering financial-config writes.
#[test]
fn set_twap_window_by_admin_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let client = make_client(&env);
    client.initialize(&admin, &None::<Address>, &Some(false));
    set_time(&env, 1_000);

    // Issuer is distinct from admin (so the admin path is actually exercised).
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payout = Address::generate(&env);
    let co: Vec<Address> = Vec::new(&env);
    client.register_offering(
        &issuer,
        &co,
        &1u32,
        &symbol_short!("ns"),
        &offering_token,
        &10_000u32,
        &payout,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    // Call as admin, not as issuer.
    let r = client.try_set_twap_window(
        &admin,
        &issuer,
        &symbol_short!("ns"),
        &offering_token,
        &3_600u64,
    );
    assert!(r.is_ok(), "admin must be permitted, got {r:?}");

    let stored = client
        .get_twap_window(&issuer, &symbol_short!("ns"), &offering_token)
        .unwrap();
    assert_eq!(stored.window_secs, 3_600);
    assert_eq!(stored.set_by, admin, "audit must show admin authored the call");
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 9 — `previous_window` sentinel in the event payload
// ─────────────────────────────────────────────────────────────────────────────

/// The first `set_twap_window` call publishes at least one event, and a
/// subsequent reconfiguration publishes another. This is the audit-trail
/// guarantee the doc-comment on `EVENT_TWAP_WINDOW_SET` makes: every
/// successful set publishes an event, and off-chain indexers can reconstruct
/// the per-offering history from the stream.
///
/// The exact `previous_window_secs` value (0 sentinel on first call →
/// prior value on subsequent calls) is documented in
/// `EVENT_TWAP_WINDOW_SET`'s doc-comment; this test only confirms that the
/// event stream grows on every set, without introspecting the data tuple
/// shape (which depends on Soroban SDK internals).
#[test]
fn event_publishes_one_event_per_successful_set() {
    let env = Env::default();
    let (client, issuer, token) = setup_offering(&env);
    let ns = symbol_short!("ns");
    set_time(&env, 1_000);

    let before_first = env.events().all().len();
    client.set_twap_window(&issuer, &issuer, &ns, &token, &300u64);
    let after_first = env.events().all().len();
    assert!(after_first > before_first, "first set must publish");

    let before_second = env.events().all().len();
    client.set_twap_window(&issuer, &issuer, &ns, &token, &600u64);
    let after_second = env.events().all().len();
    assert!(after_second > before_second, "second set must publish");

    // Three reconfigurations: each must publish.
    let baseline = env.events().all().len();
    for w in [120u64, 240u64, 480u64, 960u64] {
        let b = env.events().all().len();
        client.set_twap_window(&issuer, &issuer, &ns, &token, &w);
        assert!(env.events().all().len() > b, "config to {w} must publish");
    }
    assert!(env.events().all().len() >= baseline + 4);

    // Post-state after the four reconfigurations must reflect the latest.
    let stored = client.get_twap_window(&issuer, &ns, &token).unwrap();
    assert_eq!(stored.window_secs, 960);
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 10 — Quiescence guards
// ─────────────────────────────────────────────────────────────────────────────

/// When the contract is paused via admin, `set_twap_window` is rejected with
/// `ContractPaused`. Mirrors the guarded-behaviour contract: an admin can
/// quiesce the contract and no config writes succeed until the pause is
/// lifted.
#[test]
fn set_twap_window_rejected_when_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let safety = Address::generate(&env);
    let client = make_client(&env);
    client.initialize(&admin, &Some(safety), &Some(false));
    set_time(&env, 1_000);

    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payout = Address::generate(&env);
    let co: Vec<Address> = Vec::new(&env);
    client.register_offering(
        &issuer,
        &co,
        &1u32,
        &symbol_short!("ns"),
        &offering_token,
        &10_000u32,
        &payout,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    // Pause the contract. We use `pause_admin` as the documented apis for
    // an admin-initiated pause.
    client.pause_admin(&admin);
    assert!(client.is_paused(), "contract must be paused");

    let r = client.try_set_twap_window(
        &admin,
        &issuer,
        &symbol_short!("ns"),
        &offering_token,
        &3_600u64,
    );
    assert_eq!(
        r,
        Err(Ok(RevoraError::ContractPaused)),
        "set_twap_window must be blocked while paused"
    );
}

/// After a pause + recovery cycle, `set_twap_window` resumes working. This
/// guards against a stuck-quiescent state where paused-flags are not cleared
/// at the right boundary.
#[test]
fn set_twap_window_succeeds_after_pause_then_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let safety = Address::generate(&env);
    let client = make_client(&env);
    client.initialize(&admin, &Some(safety.clone()), &Some(false));
    set_time(&env, 1_000);

    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payout = Address::generate(&env);
    let co: Vec<Address> = Vec::new(&env);
    client.register_offering(
        &issuer,
        &co,
        &1u32,
        &symbol_short!("ns"),
        &offering_token,
        &10_000u32,
        &payout,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    // Pause.
    client.pause_admin(&admin);
    assert!(client.is_paused(), "contract must be paused");

    // Writes fail while paused.
    let paused_attempt = client.try_set_twap_window(
        &admin,
        &issuer,
        &symbol_short!("ns"),
        &offering_token,
        &3_600u64,
    );
    assert_eq!(paused_attempt, Err(Ok(RevoraError::ContractPaused)));

    // Unpause via the safety role (the canonical lift path).
    client.unpause_safety(&safety);
    assert!(!client.is_paused(), "contract must be unpaused");

    // Writes succeed again.
    let after_recovery = client.try_set_twap_window(
        &admin,
        &issuer,
        &symbol_short!("ns"),
        &offering_token,
        &3_600u64,
    );
    assert!(
        after_recovery.is_ok(),
        "set_twap_window must succeed after unpause, got {after_recovery:?}"
    );
}
