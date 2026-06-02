//! # Two-Tier Pause State — Test Suite
//!
//! Covers the `SoftPaused` / `HardPaused` escalation matrix introduced to
//! replace the single binary pause flag.
//!
//! ## Tier semantics
//!
//! | State       | reports/deposits | claim |
//! |-------------|-----------------|-------|
//! | NotPaused   | ✓               | ✓     |
//! | SoftPaused  | ✗               | ✓     |
//! | HardPaused  | ✗               | ✗     |
//!
//! ## Security notes
//!
//! - Only the **admin** can reach `HardPaused` (via `hard_pause_admin`).
//! - The **safety** role is capped at `SoftPaused` — it cannot strand holder funds.
//! - `is_paused()` returns `true` for both tiers (backward-compatible binary signal).
//! - `get_pause_state()` returns the exact `PauseState` discriminant.
//! - Every pause/unpause call emits both the legacy `paused`/`unpaused` event and
//!   the new versioned `paused2` event carrying the tier.
//! - Escalation from `SoftPaused` → `HardPaused` is a single admin call; no
//!   intermediate unpause is required.

#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, token, Address, Env};

use crate::{PauseState, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Initialize with both admin and safety roles; mock all auths for the test.
fn setup(env: &Env) -> (RevoraRevenueShareClient<'_>, Address, Address) {
    env.mock_all_auths();
    let client = make_client(env);
    let admin = Address::generate(env);
    let safety = Address::generate(env);
    client.initialize(&admin, &Some(safety.clone()), &None::<bool>);
    (client, admin, safety)
}

/// Full offering + holder setup with a real Stellar asset token so `claim` can
/// actually transfer tokens.
///
/// Returns `(client, admin, safety, issuer, offering_token, payment_token, holder)`.
fn setup_with_offering(
    env: &Env,
) -> (RevoraRevenueShareClient<'_>, Address, Address, Address, Address, Address, Address) {
    env.mock_all_auths();
    let client = make_client(env);
    let admin = Address::generate(env);
    let safety = Address::generate(env);
    client.initialize(&admin, &Some(safety.clone()), &None::<bool>);

    let issuer = Address::generate(env);
    let offering_token = Address::generate(env);
    let payment_admin = Address::generate(env);
    let payment_token = env.register_stellar_asset_contract_v2(payment_admin.clone());
    let holder = Address::generate(env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &10_000,
        &payment_token.address(),
        &0,
    );
    client.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder, &10_000);

    // Mint to issuer and deposit period 1
    token::StellarAssetClient::new(env, &payment_token.address()).mint(&issuer, &500_000);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token.address(),
        &100_000,
        &1,
    );

    (client, admin, safety, issuer, offering_token, payment_token.address(), holder)
}

// ── Section A: get_pause_state / is_paused ───────────────────────────────────

/// Fresh contract reports NotPaused.
#[test]
fn get_pause_state_default_is_not_paused() {
    let env = Env::default();
    let client = make_client(&env);
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
}

/// After `pause_admin`, state is SoftPaused and `is_paused` returns true.
#[test]
fn pause_admin_sets_soft_paused() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    client.pause_admin(&admin);

    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
    assert!(client.is_paused());
}

/// After `pause_safety`, state is SoftPaused and `is_paused` returns true.
#[test]
fn pause_safety_sets_soft_paused() {
    let env = Env::default();
    let (client, _admin, safety) = setup(&env);

    client.pause_safety(&safety);

    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
    assert!(client.is_paused());
}

/// After `hard_pause_admin`, state is HardPaused and `is_paused` returns true.
#[test]
fn hard_pause_admin_sets_hard_paused() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    client.hard_pause_admin(&admin);

    assert_eq!(client.get_pause_state(), PauseState::HardPaused);
    assert!(client.is_paused());
}

/// `unpause_admin` from SoftPaused restores NotPaused.
#[test]
fn unpause_admin_from_soft_restores_not_paused() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    client.pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    client.unpause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
}

/// `unpause_admin` from HardPaused restores NotPaused.
#[test]
fn unpause_admin_from_hard_restores_not_paused() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    client.hard_pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);

    client.unpause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
}

/// `unpause_safety` from SoftPaused restores NotPaused.
#[test]
fn unpause_safety_from_soft_restores_not_paused() {
    let env = Env::default();
    let (client, _admin, safety) = setup(&env);

    client.pause_safety(&safety);
    client.unpause_safety(&safety);

    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
}

// ── Section B: SoftPaused — claim allowed, mutations blocked ─────────────────

/// Under SoftPaused, `claim` succeeds and the holder receives their payout.
#[test]
fn soft_pause_claim_succeeds() {
    let env = Env::default();
    let (client, admin, _safety, _issuer, offering_token, _payment_token, holder) =
        setup_with_offering(&env);

    client.pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    let result = client.try_claim(&holder, &_issuer, &symbol_short!("def"), &offering_token, &50);
    assert!(result.is_ok(), "claim must succeed under SoftPaused, got {result:?}");
    assert_eq!(result.unwrap().unwrap(), 100_000, "holder should receive full payout");
}

/// Under SoftPaused, `deposit_revenue` is blocked with ContractPaused.
#[test]
fn soft_pause_deposit_blocked() {
    let env = Env::default();
    let (client, admin, _safety, issuer, offering_token, payment_token, _holder) =
        setup_with_offering(&env);

    client.pause_admin(&admin);

    token::StellarAssetClient::new(&env, &payment_token).mint(&issuer, &100_000);
    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token,
        &100_000,
        &2,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::ContractPaused)),
        "deposit must be blocked under SoftPaused"
    );
}

/// Under SoftPaused, `register_offering` is blocked with ContractPaused.
#[test]
fn soft_pause_register_offering_blocked() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    client.pause_admin(&admin);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let result =
        client.try_register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));
}

/// Under SoftPaused, `set_holder_share` is blocked with ContractPaused.
#[test]
fn soft_pause_set_holder_share_blocked() {
    let env = Env::default();
    let (client, admin, _safety, issuer, offering_token, _payment_token, holder) =
        setup_with_offering(&env);

    client.pause_admin(&admin);

    let result = client.try_set_holder_share(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &holder,
        &5_000u32,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));
}

// ── Section C: HardPaused — everything blocked including claim ────────────────

/// Under HardPaused, `claim` is blocked with ContractPaused.
#[test]
fn hard_pause_claim_blocked() {
    let env = Env::default();
    let (client, admin, _safety, issuer, offering_token, _payment_token, holder) =
        setup_with_offering(&env);

    client.hard_pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);

    let result = client.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert_eq!(
        result,
        Err(Ok(RevoraError::ContractPaused)),
        "claim must be blocked under HardPaused"
    );
}

/// Under HardPaused, `deposit_revenue` is blocked with ContractPaused.
#[test]
fn hard_pause_deposit_blocked() {
    let env = Env::default();
    let (client, admin, _safety, issuer, offering_token, payment_token, _holder) =
        setup_with_offering(&env);

    client.hard_pause_admin(&admin);

    token::StellarAssetClient::new(&env, &payment_token).mint(&issuer, &100_000);
    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &payment_token,
        &100_000,
        &2,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));
}

/// Under HardPaused, `register_offering` is blocked with ContractPaused.
#[test]
fn hard_pause_register_offering_blocked() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    client.hard_pause_admin(&admin);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let result =
        client.try_register_offering(&issuer, &symbol_short!("def"), &token, &1_000, &token, &0);
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));
}

// ── Section D: Escalation soft → hard ────────────────────────────────────────

/// Admin can escalate from SoftPaused to HardPaused without unpausing first.
#[test]
fn escalation_soft_to_hard() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    // Start with soft pause (e.g. safety triggered it)
    client.pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    // Admin escalates directly to hard pause
    client.hard_pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);
    assert!(client.is_paused());
}

/// After escalation soft → hard, claim is now blocked (was allowed under soft).
#[test]
fn escalation_soft_to_hard_blocks_claim() {
    let env = Env::default();
    let (client, admin, _safety, issuer, offering_token, _payment_token, holder) =
        setup_with_offering(&env);

    // Soft pause: claim still works
    client.pause_admin(&admin);
    let r1 = client.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert!(r1.is_ok(), "claim must succeed under SoftPaused");

    // Re-deposit for a second period so there's something to claim
    token::StellarAssetClient::new(&env, &_payment_token).mint(&issuer, &100_000);
    // Must unpause briefly to deposit (deposit is blocked under soft pause)
    client.unpause_admin(&admin);
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &_payment_token,
        &100_000,
        &2,
    );

    // Escalate to hard pause
    client.hard_pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);

    // Claim is now blocked
    let r2 = client.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert_eq!(
        r2,
        Err(Ok(RevoraError::ContractPaused)),
        "claim must be blocked after escalation to HardPaused"
    );
}

/// Safety role cannot escalate to HardPaused — `hard_pause_admin` is admin-only.
#[test]
fn safety_cannot_hard_pause() {
    let env = Env::default();
    let (client, _admin, safety) = setup(&env);

    // Safety can soft-pause
    client.pause_safety(&safety);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    // Safety cannot call hard_pause_admin (wrong identity — returns NotAuthorized)
    let result = client.try_hard_pause_admin(&safety);
    assert_eq!(
        result,
        Err(Ok(RevoraError::NotAuthorized)),
        "safety role must not be able to hard-pause"
    );
    // State must remain SoftPaused, not escalated
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
}

// ── Section E: De-escalation hard → soft ─────────────────────────────────────

/// Admin can de-escalate from HardPaused to SoftPaused by calling `pause_admin`.
#[test]
fn de_escalation_hard_to_soft() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    client.hard_pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);

    // Calling pause_admin (soft) overwrites HardPaused with SoftPaused
    client.pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
}

/// After de-escalation hard → soft, claim is allowed again.
#[test]
fn de_escalation_hard_to_soft_allows_claim() {
    let env = Env::default();
    let (client, admin, _safety, issuer, offering_token, _payment_token, holder) =
        setup_with_offering(&env);

    // Hard pause: claim blocked
    client.hard_pause_admin(&admin);
    let r1 = client.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert_eq!(r1, Err(Ok(RevoraError::ContractPaused)));

    // De-escalate to soft pause
    client.pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    // Claim is now allowed again
    let r2 = client.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert!(r2.is_ok(), "claim must succeed after de-escalation to SoftPaused, got {r2:?}");
    assert_eq!(r2.unwrap().unwrap(), 100_000);
}

// ── Section F: Idempotency ────────────────────────────────────────────────────

/// Calling `pause_admin` twice is idempotent — stays SoftPaused.
#[test]
fn pause_admin_idempotent() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    client.pause_admin(&admin);
    client.pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
}

/// Calling `hard_pause_admin` twice is idempotent — stays HardPaused.
#[test]
fn hard_pause_admin_idempotent() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    client.hard_pause_admin(&admin);
    client.hard_pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);
}

/// Calling `unpause_admin` on an already-unpaused contract is idempotent.
#[test]
fn unpause_admin_idempotent() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    client.unpause_admin(&admin);
    client.unpause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    assert!(!client.is_paused());
}

// ── Section G: Full round-trip ────────────────────────────────────────────────

/// NotPaused → SoftPaused → NotPaused round-trip via admin.
#[test]
fn round_trip_soft_pause_admin() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    client.pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
    client.unpause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
}

/// NotPaused → SoftPaused → NotPaused round-trip via safety.
#[test]
fn round_trip_soft_pause_safety() {
    let env = Env::default();
    let (client, _admin, safety) = setup(&env);

    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    client.pause_safety(&safety);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);
    client.unpause_safety(&safety);
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
}

/// NotPaused → HardPaused → NotPaused round-trip via admin.
#[test]
fn round_trip_hard_pause_admin() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
    client.hard_pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);
    client.unpause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
}

/// Full escalation matrix: NotPaused → Soft → Hard → Soft → NotPaused.
#[test]
fn full_escalation_matrix() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);

    assert_eq!(client.get_pause_state(), PauseState::NotPaused);

    client.pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    client.hard_pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::HardPaused);

    client.pause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::SoftPaused);

    client.unpause_admin(&admin);
    assert_eq!(client.get_pause_state(), PauseState::NotPaused);
}

// ── Section H: Backward-compat — existing tests still hold ───────────────────

/// `is_paused` returns true for SoftPaused (backward compat).
#[test]
fn is_paused_true_for_soft_paused() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);
    client.pause_admin(&admin);
    assert!(client.is_paused());
}

/// `is_paused` returns true for HardPaused (backward compat).
#[test]
fn is_paused_true_for_hard_paused() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);
    client.hard_pause_admin(&admin);
    assert!(client.is_paused());
}

/// `is_paused` returns false for NotPaused (backward compat).
#[test]
fn is_paused_false_for_not_paused() {
    let env = Env::default();
    let (client, admin, _safety) = setup(&env);
    client.pause_admin(&admin);
    client.unpause_admin(&admin);
    assert!(!client.is_paused());
}
