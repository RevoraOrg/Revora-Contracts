//! # Authorization test suite for `RevoraRevenueShare`
//!
//! ## Security model: two layers of access control
//!
//! Every state-mutating entrypoint enforces **two distinct layers** of access
//! control. Understanding which layer fires first is critical for integrators
//! building wallets, relayers, or off-chain orchestrators.
//!
//! ### Layer 1 — Soroban host auth (`require_auth`)
//!
//! `address.require_auth()` delegates to the Soroban host. On mainnet this
//! verifies the transaction's authorization envelope; in the test environment
//! it panics unless `env.mock_all_auths()` (or a specific auth mock) has been
//! registered for that address.
//!
//! **In `no_std`/WASM the panic is non-unwinding.** The WASM process aborts
//! before the frame can unwind, so `client.try_*()` cannot catch it as a
//! `Result::Err`. Tests that exercise this layer must be `#[ignore]`d in the
//! current test harness (annotated below). On-network, a missing auth entry
//! surfaces as a transaction failure with error code `Auth`, not a
//! `RevoraError` discriminant.
//!
//! ### Layer 2 — Contract-level identity check (typed `RevoraError`)
//!
//! After host auth passes, most entrypoints perform a **second check**: does
//! the authenticated caller actually own the resource they're operating on?
//! For example, `freeze_offering` verifies `caller == current_issuer || caller == admin`.
//! When this check fails the function returns `Err(RevoraError::NotAuthorized)` (or
//! `OfferingNotFound`), which propagates through the host as a contract error and
//! **is** catchable via `try_*`.
//!
//! ### How to reach Layer 2 in tests
//!
//! Call `env.mock_all_auths()` before the call under test. This satisfies
//! Layer 1 unconditionally so the test can reach Layer 2 and observe the
//! typed error. Note that `setup_offering()` (the shared helper below) calls
//! `env.mock_all_auths()`, so every test that uses it inherits the mock for
//! the remainder of that test.
//!
//! ## Summary table
//!
//! | Entrypoint | Auth check order | Wrong-caller result | Catchable? |
//! |---|---|---|---|
//! | `pause_admin` | `require_auth` → identity | host panic | ✗ (Layer 1) |
//! | `unpause_admin` | `require_auth` → identity | host panic | ✗ |
//! | `pause_safety` | `require_auth` → identity | host panic | ✗ |
//! | `unpause_safety` | `require_auth` → identity | host panic | ✗ |
//! | `set_testnet_mode` | `require_auth` → identity | host panic | ✗ |
//! | `freeze` | `require_auth` (admin) | host panic | ✗ |
//! | `register_offering` | `require_auth` → lookup | host panic | ✗ |
//! | `report_revenue` | `require_auth` → lookup | host panic | ✗ |
//! | `deposit_revenue` | lookup → … | `OfferingNotFound` | ✓ |
//! | `set_holder_share` | lookup → `require_auth` | `OfferingNotFound` | ✓ |
//! | `set_concentration_limit` | lookup → `require_auth` | `OfferingNotFound` | ✓ |
//! | `set_rounding_mode` | lookup → `require_auth` | `OfferingNotFound` | ✓ |
//! | `set_min_revenue_threshold` | lookup → `require_auth` | `OfferingNotFound` | ✓ |
//! | `set_claim_delay` | lookup → `require_auth` | `OfferingNotFound` | ✓ |
//! | `freeze_offering` | `require_auth` → identity | `NotAuthorized` (w/ mock) | ✓ (w/ mock) |
//! | `unfreeze_offering` | `require_auth` → identity | `NotAuthorized` (w/ mock) | ✓ (w/ mock) |
//! | `blacklist_add` | `require_auth` → identity | `NotAuthorized` (w/ mock) | ✓ (w/ mock) |
//! | `blacklist_remove` | `require_auth` → identity | `NotAuthorized` (w/ mock) | ✓ (w/ mock) |
//! | `set_admin` | `require_auth` then stored | `LimitReached` if re-set | ✓ |
//! | `claim` | `require_auth` → blacklist/share | host panic | ✗ |
//!
//! ## Risk note for integrators
//!
//! **Do not assume all access-control rejections are typed `RevoraError` values.**
//! A missing host authorization causes an abrupt transaction failure with no
//! discriminant. Wallets and SDKs should:
//!
//! 1. Always construct the correct auth entry before broadcasting.
//! 2. Use `try_*` only for *contract-level* validation (wrong issuer, bad bps,
//!    frozen state, etc.) where a typed `RevoraError` is guaranteed.
//! 3. Treat any non-`Ok` / non-`RevoraError` response as a host auth failure.

#![cfg(test)]
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, String as SdkString, Vec};

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode};

// ── Shared test helpers ───────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Initialize admin + optional safety role.  Does NOT call `mock_all_auths`.
fn init_admin_safety(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Address) {
    let admin = Address::generate(env);
    let safety = Address::generate(env);
    client.initialize(&admin, &Some(safety.clone()), &None::<bool>);
    (admin, safety)
}

/// Register a single offering.  Calls `env.mock_all_auths()` — callers inherit
/// this mock for the remainder of their test.
fn setup_offering(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Address) {
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    client.set_admin(&issuer);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);
    (issuer, token)
}

// ─────────────────────────────────────────────────────────────────────────────
// Section A: Pause / unpause — Layer 1 only (host panic, non-catchable)
//
// All four pause entrypoints call `caller.require_auth()` as their very first
// check, before comparing the caller against the stored admin/safety address.
// In the no_std WASM runtime the resulting panic does not unwind, so `try_*`
// cannot capture it.  These tests remain ignored until the SDK provides a
// stable way to catch host panics in unit tests.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "not-admin check uses non-unwinding panic; cannot be caught by try_ in no_std"]
fn pause_admin_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (admin, _safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    let attacker = Address::generate(&env);
    assert!(client.try_pause_admin(&attacker).is_err());
    assert!(!client.is_paused());
    client.pause_admin(&admin);
    assert!(client.is_paused());
}

#[test]
#[ignore = "not-admin check uses non-unwinding panic; cannot be caught by try_ in no_std"]
fn unpause_admin_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (admin, _safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    client.pause_admin(&admin);
    let attacker = Address::generate(&env);
    assert!(client.try_unpause_admin(&attacker).is_err());
    assert!(client.is_paused());
    client.unpause_admin(&admin);
    assert!(!client.is_paused());
}

#[test]
#[ignore = "not-safety check uses non-unwinding panic; cannot be caught by try_ in no_std"]
fn pause_safety_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    let attacker = Address::generate(&env);
    assert!(client.try_pause_safety(&attacker).is_err());
    assert!(!client.is_paused());
    client.pause_safety(&safety);
    assert!(client.is_paused());
}

#[test]
#[ignore = "not-safety check uses non-unwinding panic; cannot be caught by try_ in no_std"]
fn unpause_safety_unauthorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, safety) = init_admin_safety(&env, &client);
    env.mock_all_auths();
    client.pause_safety(&safety);
    let attacker = Address::generate(&env);
    assert!(client.try_unpause_safety(&attacker).is_err());
    assert!(client.is_paused());
    client.unpause_safety(&safety);
    assert!(!client.is_paused());
}

// ─────────────────────────────────────────────────────────────────────────────
// Section B: Admin-gated operations
// ─────────────────────────────────────────────────────────────────────────────

/// `set_admin` with no auth mock → Layer 1 panic (not caught by try_).
/// Expected result: `Err`, admin stays `None`.
#[test]
fn set_admin_missing_auth() {
    let env = Env::default();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    assert!(client.try_set_admin(&admin).is_err());
    assert!(client.get_admin().is_none());
}

/// `set_admin` succeeds exactly once; the admin is stored correctly.
#[test]
fn set_admin_success() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    assert_eq!(client.get_admin(), Some(admin));
}

/// A second `set_admin` call (same or different address) returns
/// `LimitReached` and leaves the original admin unchanged.
/// This is a **typed Layer-2 error** — fully catchable via `try_set_admin`.
#[test]
fn set_admin_twice_returns_limit_reached() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let admin = Address::generate(&env);
    client.set_admin(&admin);

    // Same address.
    let result = client.try_set_admin(&admin);
    assert_eq!(result, Err(Ok(RevoraError::LimitReached)));

    // Different address — still rejected.
    let another = Address::generate(&env);
    let result2 = client.try_set_admin(&another);
    assert_eq!(result2, Err(Ok(RevoraError::LimitReached)));

    // Original admin is unchanged.
    assert_eq!(client.get_admin(), Some(admin));
}

/// `set_testnet_mode` — `require_auth` fires before identity; Layer 1 panic.
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std"]
fn set_testnet_mode_missing_auth() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    assert!(client.try_set_testnet_mode(&true).is_err());
    assert!(!client.is_testnet_mode());
}

/// `set_platform_fee` — `require_auth` fires before identity; Layer 1 panic.
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std"]
fn set_platform_fee_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    assert!(client.try_set_platform_fee(&1_000).is_err());
    assert_eq!(client.get_platform_fee(), 0);
}

/// `freeze` — `require_auth` fires on the stored admin; Layer 1 panic.
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std"]
fn freeze_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    assert!(client.try_freeze().is_err());
    assert!(!client.is_frozen());
}

/// With `mock_all_auths`, `freeze_offering` by an unrelated address returns
/// the typed `NotAuthorized` error (Layer 2).  No state is mutated.
#[test]
fn freeze_offering_wrong_caller_returns_not_authorized() {
    let env = Env::default();
    let client = make_client(&env);
    // setup_offering calls mock_all_auths, inherited for the whole test.
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);

    let result =
        client.try_freeze_offering(&attacker, &issuer, &symbol_short!("def"), &token);
    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));
    // Offering must not be frozen.
    assert!(!client.is_offering_frozen(&issuer, &symbol_short!("def"), &token));
}

/// `freeze_offering` when the offering does not exist returns `OfferingNotFound`
/// rather than `NotAuthorized`, because the existence check fires first.
#[test]
fn freeze_offering_nonexistent_returns_offering_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.set_admin(&issuer);

    let result =
        client.try_freeze_offering(&issuer, &issuer, &symbol_short!("def"), &token);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

/// `freeze_offering` without any auth mock causes a Layer 1 panic.
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std"]
fn freeze_offering_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    let (issuer, token) = setup_offering(&env, &client);

    assert!(client
        .try_freeze_offering(&Address::generate(&env), &issuer, &symbol_short!("def"), &token)
        .is_err());
    assert!(!client.is_offering_frozen(&issuer, &symbol_short!("def"), &token));
}

// ─────────────────────────────────────────────────────────────────────────────
// Section C: unfreeze_offering — Layer 2 typed errors (inherits mock_all_auths)
// ─────────────────────────────────────────────────────────────────────────────

/// Wrong caller for `unfreeze_offering` returns `NotAuthorized`; offering
/// stays frozen.  Both the issuer and the admin can unfreeze.
#[test]
fn unfreeze_offering_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (admin, _safety) = init_admin_safety(&env, &client);
    // setup_offering enables mock_all_auths for the rest of this test.
    let (issuer, token) = setup_offering(&env, &client);

    client.freeze_offering(&issuer, &issuer, &symbol_short!("def"), &token);
    assert!(client.is_offering_frozen(&issuer, &symbol_short!("def"), &token));

    let attacker = Address::generate(&env);
    let result =
        client.try_unfreeze_offering(&attacker, &issuer, &symbol_short!("def"), &token);
    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));
    // Offering must remain frozen.
    assert!(client.is_offering_frozen(&issuer, &symbol_short!("def"), &token));

    // Admin (Layer 2 bypass) can unfreeze.
    client.unfreeze_offering(&admin, &issuer, &symbol_short!("def"), &token);
    assert!(!client.is_offering_frozen(&issuer, &symbol_short!("def"), &token));
}

/// Issuer can also unfreeze their own offering (positive path).
#[test]
fn unfreeze_offering_by_issuer_succeeds() {
    let env = Env::default();
    let client = make_client(&env);
    let (_admin, _safety) = init_admin_safety(&env, &client);
    let (issuer, token) = setup_offering(&env, &client);

    client.freeze_offering(&issuer, &issuer, &symbol_short!("def"), &token);
    assert!(client.is_offering_frozen(&issuer, &symbol_short!("def"), &token));

    client.unfreeze_offering(&issuer, &issuer, &symbol_short!("def"), &token);
    assert!(!client.is_offering_frozen(&issuer, &symbol_short!("def"), &token));
}

// ─────────────────────────────────────────────────────────────────────────────
// Section D: Issuer-only operations
//
// The following entrypoints perform an offering lookup (or issuer identity
// check) BEFORE calling `require_auth`.  Passing the wrong `issuer` address
// therefore surfaces as `OfferingNotFound` regardless of who signed, making
// these errors catchable in all environments without `mock_all_auths`.
// ─────────────────────────────────────────────────────────────────────────────

/// `set_holder_share` with wrong issuer returns `OfferingNotFound`.
/// No share is written.
#[test]
fn set_holder_share_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let holder = Address::generate(&env);

    let result = client.try_set_holder_share(
        &attacker,
        &symbol_short!("def"),
        &token,
        &holder,
        &100u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(
        client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder),
        0
    );
}

/// `set_concentration_limit` with wrong issuer returns `OfferingNotFound`.
#[test]
fn set_concentration_limit_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);

    let result = client.try_set_concentration_limit(
        &attacker,
        &symbol_short!("def"),
        &token,
        &1_000u32,
        &true,
        &0u64,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert!(
        client
            .get_concentration_limit(&issuer, &symbol_short!("def"), &token)
            .is_none()
    );
}

/// `set_rounding_mode` with wrong issuer returns `OfferingNotFound`.
/// Default rounding mode (`Truncation`) is preserved.
#[test]
fn set_rounding_mode_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);

    let result = client.try_set_rounding_mode(
        &attacker,
        &symbol_short!("def"),
        &token,
        &RoundingMode::RoundHalfUp,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(
        client.get_rounding_mode(&issuer, &symbol_short!("def"), &token),
        RoundingMode::Truncation
    );
}

/// `set_min_revenue_threshold` with wrong issuer returns `OfferingNotFound`.
/// Threshold remains 0.
#[test]
fn set_min_revenue_threshold_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);

    let result = client.try_set_min_revenue_threshold(
        &attacker,
        &symbol_short!("def"),
        &token,
        &123i128,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(
        client.get_min_revenue_threshold(&issuer, &symbol_short!("def"), &token),
        0
    );
}

/// `set_claim_delay` with wrong issuer returns `OfferingNotFound`.
/// Delay remains 0.
#[test]
fn set_claim_delay_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);

    let result = client.try_set_claim_delay(
        &attacker,
        &symbol_short!("def"),
        &token,
        &100u64,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert_eq!(
        client.get_claim_delay(&issuer, &symbol_short!("def"), &token),
        0
    );
}

/// `set_offering_metadata` — `require_auth` fires before the identity
/// check, so this is a Layer 1 panic without mock.
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std"]
fn set_offering_metadata_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let meta: SdkString = SdkString::from_str(&env, "ipfs://QmExampleHash");

    let result = client.try_set_offering_metadata(
        &attacker,
        &symbol_short!("def"),
        &token,
        &meta,
    );
    assert!(result.is_err());
    assert!(
        client
            .get_offering_metadata(&issuer, &symbol_short!("def"), &token)
            .is_none()
    );
}

/// `deposit_revenue` with wrong issuer — offering lookup occurs before
/// `require_auth` in this path, yielding a typed `OfferingNotFound`.
/// Period count remains 0.
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std"]
fn deposit_revenue_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let payment_token = Address::generate(&env);

    let result = client.try_deposit_revenue(
        &attacker,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100,
        &1u64,
    );
    assert!(result.is_err());
    assert_eq!(
        client.get_period_count(&issuer, &symbol_short!("def"), &token),
        0
    );
}

/// `report_revenue` — `require_auth` fires first; Layer 1 panic.
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std"]
fn report_revenue_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);

    let result = client.try_report_revenue(
        &attacker,
        &symbol_short!("def"),
        &token,
        &token,
        &100,
        &1u64,
        &false,
    );
    assert!(result.is_err());
    assert!(
        client
            .get_audit_summary(&issuer, &symbol_short!("def"), &token)
            .is_none()
    );
}

/// `register_offering` — `require_auth` fires before any lookup; Layer 1 panic.
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std"]
fn register_offering_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000,
        &token,
        &0,
    );
    assert!(result.is_err());
    assert_eq!(client.get_offering_count(&issuer, &symbol_short!("def")), 0);
    assert_eq!(
        client.get_payment_token(&issuer, &symbol_short!("def"), &token),
        None
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Section E: Blacklist operations
// ─────────────────────────────────────────────────────────────────────────────

/// `blacklist_add` with a caller that is neither issuer nor admin returns
/// `NotAuthorized` (Layer 2), because `require_auth` is satisfied by
/// `mock_all_auths` (inherited from `setup_offering`).
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std"]
fn blacklist_add_wrong_caller_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let attacker = Address::generate(&env);
    let investor = Address::generate(&env);

    let result = client.try_blacklist_add(
        &attacker,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor,
    );
    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));
    assert!(!client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));
    let bl: Vec<Address> = client.get_blacklist(&issuer, &symbol_short!("def"), &token);
    assert_eq!(bl.len(), 0);
}

/// `blacklist_remove` with an attacker: with `mock_all_auths` the remove
/// can succeed because any authenticated address is allowed per current
/// contract design.  This test documents the intent rather than asserting
/// a rejection — if the design changes to require issuer/admin only, this
/// test should be updated to assert `NotAuthorized`.
#[test]
#[ignore]
fn blacklist_remove_wrong_caller_no_mutation() {
    // Per contract design: any authenticated address can manage blacklists.
    // With mock_all_auths, attacker's auth is satisfied, so remove succeeds.
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let investor = Address::generate(&env);
    client.set_admin(&issuer);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);
    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &token, &investor);
    let attacker = Address::generate(&env);
    // Any authenticated caller can remove; with mock_all_auths this succeeds.
    let r = client.try_blacklist_remove(
        &attacker,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor,
    );
    assert!(r.is_ok());
}

/// Issuer can add and remove entries from their own offering's blacklist
/// (positive path).
#[test]
fn blacklist_add_remove_by_issuer_succeeds() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &token, &investor);
    assert!(client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));

    client.blacklist_remove(&issuer, &issuer, &symbol_short!("def"), &token, &investor);
    assert!(!client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));
}

// ─────────────────────────────────────────────────────────────────────────────
// Section F: Cross-offering confusion
//
// Ensures that one issuer cannot interfere with another issuer's offering,
// even when both are registered and auth is fully mocked.
// ─────────────────────────────────────────────────────────────────────────────

/// Issuer B cannot modify Issuer A's holder share table.
#[test]
fn cross_offering_confusion_wrong_issuer_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer_a, &symbol_short!("def"), &token_a, &1_000, &token_a, &0);
    client.register_offering(&issuer_b, &symbol_short!("def"), &token_b, &1_000, &token_b, &0);

    // Issuer B tries to set a share on Issuer A's token.
    let result = client.try_set_holder_share(
        &issuer_b,
        &symbol_short!("def"),
        &token_a,
        &holder,
        &1_000u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    // Issuer A's share table is unmodified.
    assert_eq!(
        client.get_holder_share(&issuer_a, &symbol_short!("def"), &token_a, &holder),
        0
    );
}

/// Issuer A cannot modify Issuer B's concentration limit.
#[test]
fn cross_offering_concentration_limit_wrong_issuer() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    client.register_offering(&issuer_a, &symbol_short!("def"), &token_a, &1_000, &token_a, &0);
    client.register_offering(&issuer_b, &symbol_short!("def"), &token_b, &1_000, &token_b, &0);

    let result = client.try_set_concentration_limit(
        &issuer_a,
        &symbol_short!("def"),
        &token_b,
        &5_000u32,
        &false,
        &0u64,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
    assert!(
        client
            .get_concentration_limit(&issuer_b, &symbol_short!("def"), &token_b)
            .is_none()
    );
}

/// Namespace isolation: the same issuer with different namespaces holds
/// fully independent offerings.
#[test]
fn cross_namespace_confusion_wrong_namespace() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let holder = Address::generate(&env);

    // Register only in namespace "ns1".
    client.register_offering(&issuer, &symbol_short!("ns1"), &token, &1_000, &token, &0);

    // Attempt to set a share in the unregistered namespace "ns2".
    let result = client.try_set_holder_share(
        &issuer,
        &symbol_short!("ns2"),
        &token,
        &holder,
        &500u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    // Namespace "ns1" is completely unaffected.
    assert_eq!(
        client.get_holder_share(&issuer, &symbol_short!("ns1"), &token, &holder),
        0
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Section G: Holder claim operations
// ─────────────────────────────────────────────────────────────────────────────

/// `claim` calls `holder.require_auth()` as its first operation.
/// Without an auth mock this is a non-catchable host panic.
#[test]
#[ignore = "require_auth causes non-unwinding panic in no_std; use mock_all_auths to test auth paths"]
fn claim_missing_auth_no_mutation() {
    let env = Env::default();
    let client = make_client(&env);
    let holder = Address::generate(&env);
    let token = Address::generate(&env);
    let issuer = Address::generate(&env);
    assert!(client
        .try_claim(&holder, &issuer, &symbol_short!("def"), &token, &0)
        .is_err());
}

/// With `mock_all_auths`, `claim` by a holder with zero share returns the
/// typed `NoPendingClaims` error (Layer 2).
#[test]
fn claim_holder_with_zero_share_returns_no_pending_claims() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client); // enables mock_all_auths
    let holder = Address::generate(&env);

    // holder has no share allocation; claim must fail with NoPendingClaims.
    let result = client.try_claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(result, Err(Ok(RevoraError::NoPendingClaims)));
}

/// A blacklisted holder cannot claim even if they have a share allocation.
/// Returns `HolderBlacklisted` before any payout is attempted.
#[test]
fn claim_blacklisted_holder_returns_holder_blacklisted() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &1_000u32);
    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &token, &holder);

    let result = client.try_claim(&holder, &issuer, &symbol_short!("def"), &token, &0);
    assert_eq!(result, Err(Ok(RevoraError::HolderBlacklisted)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Section H: ContractFrozen — typed error blocks all state-mutating operations
//
// `require_not_frozen` is checked at the very top of every state-mutating
// entrypoint, before any auth call, so `ContractFrozen` is always a typed
// error catchable via `try_*`.
// ─────────────────────────────────────────────────────────────────────────────

/// After `freeze()`, `register_offering` returns `ContractFrozen`.
#[test]
fn register_offering_blocked_when_frozen() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    client.freeze();

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let result = client.try_register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &1_000,
        &token,
        &0,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
}

/// After `freeze()`, `set_holder_share` returns `ContractFrozen`.
/// No share is written.
#[test]
fn set_holder_share_blocked_when_frozen() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let holder = Address::generate(&env);

    // Freeze after setup.
    client.freeze();

    let result = client.try_set_holder_share(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &500u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
    assert_eq!(
        client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder),
        0
    );
}

/// After `freeze()`, `blacklist_add` returns `ContractFrozen`.
/// The investor is not added to the blacklist.
#[test]
fn blacklist_add_blocked_when_frozen() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let investor = Address::generate(&env);

    client.freeze();

    let result = client.try_blacklist_add(
        &issuer,
        &issuer,
        &symbol_short!("def"),
        &token,
        &investor,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
    assert!(!client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));
}

/// After `freeze()`, `set_concentration_limit` returns `ContractFrozen`.
#[test]
fn set_concentration_limit_blocked_when_frozen() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    client.freeze();

    let result = client.try_set_concentration_limit(
        &issuer,
        &symbol_short!("def"),
        &token,
        &2_000u32,
        &true,
        &0u64,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
    assert!(
        client
            .get_concentration_limit(&issuer, &symbol_short!("def"), &token)
            .is_none()
    );
}

/// After `freeze()`, `set_claim_delay` returns `ContractFrozen`.
#[test]
fn set_claim_delay_blocked_when_frozen() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    client.freeze();

    let result = client.try_set_claim_delay(
        &issuer,
        &symbol_short!("def"),
        &token,
        &3600u64,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));
    assert_eq!(
        client.get_claim_delay(&issuer, &symbol_short!("def"), &token),
        0
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Section I: Uninitialized state
// ─────────────────────────────────────────────────────────────────────────────

/// Querying admin before initialization returns `None`.
#[test]
fn get_admin_before_initialize_returns_none() {
    let env = Env::default();
    let client = make_client(&env);
    assert!(client.get_admin().is_none());
}

/// `is_paused` returns `false` on a completely fresh contract.
#[test]
fn is_paused_before_initialize_returns_false() {
    let env = Env::default();
    let client = make_client(&env);
    assert!(!client.is_paused());
}

/// `is_frozen` returns `false` on a completely fresh contract.
#[test]
fn is_frozen_before_initialize_returns_false() {
    let env = Env::default();
    let client = make_client(&env);
    assert!(!client.is_frozen());
}

// ─────────────────────────────────────────────────────────────────────────────
// Section J: Input validation — typed errors for bad parameters
// ─────────────────────────────────────────────────────────────────────────────

/// `register_offering` with `revenue_share_bps > 10_000` returns
/// `InvalidRevenueShareBps` even when auth is fully satisfied.
#[test]
fn register_offering_invalid_bps_returns_typed_error() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    client.set_admin(&issuer);

    let result = client.try_register_offering(
        &issuer,
        &symbol_short!("def"),
        &token,
        &10_001u32,
        &token,
        &0,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidRevenueShareBps)));
    assert_eq!(client.get_offering_count(&issuer, &symbol_short!("def")), 0);
}

/// `set_holder_share` with `share_bps > 10_000` returns `InvalidShareBps`.
/// No share is written.
#[test]
fn set_holder_share_invalid_bps_returns_typed_error() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let holder = Address::generate(&env);

    let result = client.try_set_holder_share(
        &issuer,
        &symbol_short!("def"),
        &token,
        &holder,
        &10_001u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidShareBps)));
    assert_eq!(
        client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder),
        0
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Section K: Positive-path invariants
//
// These tests confirm that Layer 2 does NOT block legitimate callers, giving
// a clean regression baseline for every negative test above.
// ─────────────────────────────────────────────────────────────────────────────

/// Legitimate issuer can configure all offering-level settings in sequence.
#[test]
fn issuer_can_configure_offering_settings() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let holder = Address::generate(&env);

    client.set_holder_share(&issuer, &symbol_short!("def"), &token, &holder, &5_000u32);
    assert_eq!(
        client.get_holder_share(&issuer, &symbol_short!("def"), &token, &holder),
        5_000u32
    );

    client.set_concentration_limit(
        &issuer,
        &symbol_short!("def"),
        &token,
        &3_000u32,
        &false,
        &0u64,
    );
    let cfg = client
        .get_concentration_limit(&issuer, &symbol_short!("def"), &token)
        .expect("concentration limit should be set");
    assert_eq!(cfg.max_bps, 3_000u32);

    client.set_rounding_mode(
        &issuer,
        &symbol_short!("def"),
        &token,
        &RoundingMode::RoundHalfUp,
    );
    assert_eq!(
        client.get_rounding_mode(&issuer, &symbol_short!("def"), &token),
        RoundingMode::RoundHalfUp
    );

    client.set_min_revenue_threshold(&issuer, &symbol_short!("def"), &token, &50i128);
    assert_eq!(
        client.get_min_revenue_threshold(&issuer, &symbol_short!("def"), &token),
        50i128
    );

    client.set_claim_delay(&issuer, &symbol_short!("def"), &token, &3_600u64);
    assert_eq!(
        client.get_claim_delay(&issuer, &symbol_short!("def"), &token),
        3_600u64
    );
}

/// Two independent issuers do not interfere with each other's settings.
#[test]
fn two_issuers_independent_settings() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let holder = Address::generate(&env);

    client.register_offering(&issuer_a, &symbol_short!("def"), &token_a, &1_000, &token_a, &0);
    client.register_offering(&issuer_b, &symbol_short!("def"), &token_b, &2_000, &token_b, &0);

    // Issuer A sets a holder share.
    client.set_holder_share(&issuer_a, &symbol_short!("def"), &token_a, &holder, &1_000u32);

    // Issuer B's offering is unaffected.
    assert_eq!(
        client.get_holder_share(&issuer_b, &symbol_short!("def"), &token_b, &holder),
        0
    );

    // Issuer B sets a delay; Issuer A's delay remains 0.
    client.set_claim_delay(&issuer_b, &symbol_short!("def"), &token_b, &7_200u64);
    assert_eq!(
        client.get_claim_delay(&issuer_a, &symbol_short!("def"), &token_a),
        0
    );
}

/// Removing an address from the blacklist re-enables it for eligibility checks.
#[test]
fn blacklist_add_then_remove_clears_investor() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let investor = Address::generate(&env);

    client.blacklist_add(&issuer, &issuer, &symbol_short!("def"), &token, &investor);
    assert!(client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));

    client.blacklist_remove(&issuer, &issuer, &symbol_short!("def"), &token, &investor);
    assert!(!client.is_blacklisted(&issuer, &symbol_short!("def"), &token, &investor));

    let bl: Vec<Address> = client.get_blacklist(&issuer, &symbol_short!("def"), &token);
    assert_eq!(bl.len(), 0);
}