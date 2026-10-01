#![cfg(test)]
//! Adversarial coverage for the admin-only `hard_pause_admin` escalation path.
//!
//! Complements `test_pause_tiers` by pinning the authorization boundary and the
//! "state unchanged after rejection" contract:
//!   - `NotInitialized` before `initialize`;
//!   - `NotAuthorized` for a non-admin caller, with pause state left untouched;
//!   - the safety role cannot escalate to `HardPaused`;
//!   - idempotency and the round-trip back to `NotPaused` via `unpause_admin`;
//!   - the behavioural consequence: a hard pause blocks a state-mutating
//!     entrypoint with `ContractPaused`.

extern crate std;

use super::*;
use crate::{PauseState, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol, Vec};

fn initialized() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let c = RevoraRevenueShareClient::new(&env, &id);
    let admin = Address::generate(&env);
    let safety = Address::generate(&env);
    c.initialize(&admin, &Some(safety.clone()), &Some(false));
    (env, c, admin, safety)
}

// ── Pre-conditions ───────────────────────────────────────────────────────────

/// Before `initialize` there is no admin to match against.
#[test]
fn rejects_before_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let c = RevoraRevenueShareClient::new(&env, &id);
    let caller = Address::generate(&env);

    let result = c.try_hard_pause_admin(&caller);
    assert_eq!(result.unwrap_err().unwrap(), RevoraError::NotInitialized);
}

// ── Authorization boundary ───────────────────────────────────────────────────

/// A non-admin caller is rejected and the pause state is left untouched.
#[test]
fn non_admin_is_rejected_and_state_is_unchanged() {
    let (env, c, _admin, _safety) = initialized();
    let stranger = Address::generate(&env);

    let before = c.get_pause_state();
    let result = c.try_hard_pause_admin(&stranger);
    let after = c.get_pause_state();

    assert_eq!(result.unwrap_err().unwrap(), RevoraError::NotAuthorized);
    assert_eq!(before, PauseState::NotPaused);
    assert_eq!(after, before, "a rejected escalation must not change pause state");
    assert!(!c.is_paused());
}

/// The safety role is limited to `SoftPaused` and cannot escalate.
#[test]
fn safety_role_cannot_hard_pause() {
    let (_env, c, _admin, safety) = initialized();

    let result = c.try_hard_pause_admin(&safety);
    assert_eq!(result.unwrap_err().unwrap(), RevoraError::NotAuthorized);
    assert_eq!(c.get_pause_state(), PauseState::NotPaused);
}

// ── Happy path & idempotency ─────────────────────────────────────────────────

/// The admin escalates to `HardPaused`.
#[test]
fn admin_escalates_to_hard_paused() {
    let (_env, c, admin, _safety) = initialized();

    c.hard_pause_admin(&admin).unwrap();

    assert_eq!(c.get_pause_state(), PauseState::HardPaused);
    assert!(c.is_paused());
}

/// Calling it twice stays `HardPaused` and does not error.
#[test]
fn hard_pause_is_idempotent() {
    let (_env, c, admin, _safety) = initialized();

    c.hard_pause_admin(&admin).unwrap();
    c.hard_pause_admin(&admin).unwrap();

    assert_eq!(c.get_pause_state(), PauseState::HardPaused);
    assert!(c.is_paused());
}

/// `unpause_admin` restores the open state.
#[test]
fn unpause_admin_round_trips_to_not_paused() {
    let (_env, c, admin, _safety) = initialized();

    c.hard_pause_admin(&admin).unwrap();
    assert_eq!(c.get_pause_state(), PauseState::HardPaused);

    c.unpause_admin(&admin).unwrap();
    assert_eq!(c.get_pause_state(), PauseState::NotPaused);
    assert!(!c.is_paused());
}

// ── Behavioural consequence ──────────────────────────────────────────────────

/// A hard pause blocks a state-mutating entrypoint with `ContractPaused`.
#[test]
fn hard_pause_blocks_state_mutating_entrypoint() {
    let (env, c, admin, _safety) = initialized();
    c.hard_pause_admin(&admin).unwrap();

    let issuer = Address::generate(&env);
    let namespace = Symbol::new(&env, "public");
    let token = Address::generate(&env);
    let asset = Address::generate(&env);

    let result = c.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1_u32,
        &namespace,
        &token,
        &10_000_u32,
        &asset,
        &0_i128,
        &Symbol::new(&env, "XLM"),
        &7_u32,
    );

    assert_eq!(result.unwrap_err().unwrap(), RevoraError::ContractPaused);
}
