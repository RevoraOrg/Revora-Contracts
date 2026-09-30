//! Adversarial and boundary test coverage for `set_concentration_limit` in `src/lib.rs` (#1110).
//!
//! # Coverage Matrix
//!
//! | Scenario / Property                                          | Assertions / Expected Behavior |
//! |--------------------------------------------------------------|--------------------------------|
//! | Happy path (valid issuer, offering exists)                   | `Ok`, config stored exactly     |
//! | Overwrite existing config                                    | State updated to new values     |
//! | `max_bps = 0` (disabled)                                     | Accepted, stored as 0           |
//! | `max_bps = 1` (lower valid)                                  | Accepted, stored as 1           |
//! | `max_bps = 10_000` (upper valid boundary)                    | Accepted, stored as 10_000      |
//! | `max_bps = 10_001` (first invalid)                           | `InvalidShareBps`, state held   |
//! | `max_bps = u32::MAX`                                         | `InvalidShareBps`, state held   |
//! | `max_staleness_secs = 0` (guard disabled)                    | Accepted, stored as 0           |
//! | `max_staleness_secs = u64::MAX`                              | Accepted, stored exactly        |
//! | `enforce` true / false                                       | Stored flag matches input       |
//! | Unregistered offering                                        | `LimitReached`, state held      |
//! | Wrong namespace                                              | `LimitReached`, state held      |
//! | Wrong token                                                  | `LimitReached`, state held      |
//! | Attacker address as `issuer`                                 | `LimitReached`, state held      |
//! | Unauthorized caller (auth cleared, correct issuer)           | Rejected, state held            |
//! | Contract frozen                                              | `ContractFrozen`, state held    |
//! | Contract paused (soft)                                       | `ContractPaused`, state held    |
//! | Invalid `max_bps` before offering lookup                     | `InvalidShareBps` even if missing |
//! | Frozen check precedes `max_bps` check                        | `ContractFrozen` wins           |
//! | Rejected ops emit no config-set events                       | Event count unchanged           |
//! | Event-only mode: `Ok` but no persistent write                | `get_concentration_limit` none  |
//! | Multi-issuer offering (quorum auth mocked)                   | Accepted, config stored         |

#![cfg(test)]

use crate::{ConcentrationLimitConfig, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, Symbol, Vec,
};

// ── Test Setup Helpers ────────────────────────────────────────────────────────

/// Deploy contract + register one offering under `(issuer, ns, token)`.
/// Auth mocking is enabled for the remainder of the test.
fn setup_offering(
) -> (Env, RevoraRevenueShareClient<'static>, Address, Symbol, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("def");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    (env, client, issuer, ns, token, payout_asset)
}

fn get_cfg(
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    ns: &Symbol,
    token: &Address,
) -> Option<ConcentrationLimitConfig> {
    client.get_concentration_limit(issuer, ns, token)
}

fn assert_cfg_eq(
    cfg: Option<ConcentrationLimitConfig>,
    max_bps: u32,
    enforce: bool,
    max_staleness_secs: u64,
) {
    let cfg = cfg.expect("expected concentration config to be present");
    assert_eq!(cfg.max_bps, max_bps, "max_bps mismatch");
    assert_eq!(cfg.enforce, enforce, "enforce mismatch");
    assert_eq!(cfg.max_staleness_secs, max_staleness_secs, "max_staleness_secs mismatch");
}

// ── Happy path ────────────────────────────────────────────────────────────────

#[test]
fn set_concentration_limit_happy_path_stores_config() {
    let (_env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);
}

#[test]
fn set_concentration_limit_stores_enforce_and_staleness() {
    let (_env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &3_000, &true, &3_600u64);

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 3_000, true, 3_600);
}

#[test]
fn set_concentration_limit_overwrite_updates_state() {
    let (_env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);
    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);

    client.set_concentration_limit(&issuer, &ns, &token, &2_500, &true, &1_800u64);
    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 2_500, true, 1_800);
}

#[test]
fn set_concentration_limit_max_bps_zero_stored_as_disabled() {
    let (_env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &0, &true, &0u64);

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 0, true, 0);
}

#[test]
fn set_concentration_limit_max_bps_one_accepted() {
    let (_env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &1, &false, &0u64);

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 1, false, 0);
}

#[test]
fn set_concentration_limit_max_bps_upper_boundary_10000_accepted() {
    let (_env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &10_000, &false, &0u64);

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 10_000, false, 0);
}

#[test]
fn set_concentration_limit_max_staleness_zero_accepted() {
    let (_env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &true, &0u64);

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, true, 0);
}

#[test]
fn set_concentration_limit_max_staleness_u64_max_accepted() {
    let (_env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &4_000, &true, &u64::MAX);

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 4_000, true, u64::MAX);
}

#[test]
fn set_concentration_limit_emits_event_on_success() {
    let (env, client, issuer, ns, token, _pa) = setup_offering();

    let before = env.events().all().len();
    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);
    let after = env.events().all().len();
    assert!(after > before, "successful set must emit EVENT_CONC_LIMIT_SET");
}

// ── Boundary rejections: max_bps ──────────────────────────────────────────────

#[test]
fn set_concentration_limit_max_bps_10001_rejected_state_unchanged() {
    let (_env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);

    let r = client.try_set_concentration_limit(&issuer, &ns, &token, &10_001, &false, &0u64);
    assert_eq!(r, Err(Ok(RevoraError::InvalidShareBps)));

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);
}

#[test]
fn set_concentration_limit_max_bps_u32_max_rejected_state_unchanged() {
    let (_env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &7_500, &true, &60u64);

    let r = client.try_set_concentration_limit(&issuer, &ns, &token, &u32::MAX, &true, &1u64);
    assert_eq!(r, Err(Ok(RevoraError::InvalidShareBps)));

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 7_500, true, 60);
}

#[test]
fn set_concentration_limit_invalid_max_bps_before_offering_lookup() {
    // Validation order: max_bps is checked BEFORE offering existence.
    // A missing offering must still surface InvalidShareBps for bad max_bps.
    let (env, client, issuer, ns, _token, _pa) = setup_offering();
    let missing_token = Address::generate(&env);

    let r =
        client.try_set_concentration_limit(&issuer, &ns, &missing_token, &10_001, &false, &0u64);
    assert_eq!(r, Err(Ok(RevoraError::InvalidShareBps)));

    // Nothing was written for the missing token.
    assert!(get_cfg(&client, &issuer, &ns, &missing_token).is_none());
}

// ── Missing / mismatched identity ─────────────────────────────────────────────

#[test]
fn set_concentration_limit_unregistered_offering_rejected_state_unchanged() {
    let (env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);

    let missing_token = Address::generate(&env);
    let r =
        client.try_set_concentration_limit(&issuer, &ns, &missing_token, &2_000, &true, &10u64);
    assert_eq!(r, Err(Ok(RevoraError::LimitReached)));

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);
    assert!(get_cfg(&client, &issuer, &ns, &missing_token).is_none());
}

#[test]
fn set_concentration_limit_wrong_namespace_rejected_state_unchanged() {
    let (_env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);

    let bad_ns = symbol_short!("badns");
    let r = client.try_set_concentration_limit(&issuer, &bad_ns, &token, &2_000, &true, &10u64);
    assert_eq!(r, Err(Ok(RevoraError::LimitReached)));

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);
    assert!(get_cfg(&client, &issuer, &bad_ns, &token).is_none());
}

#[test]
fn set_concentration_limit_wrong_token_rejected_state_unchanged() {
    let (env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);

    let other_token = Address::generate(&env);
    let r = client.try_set_concentration_limit(&issuer, &ns, &other_token, &1_000, &false, &0u64);
    assert_eq!(r, Err(Ok(RevoraError::LimitReached)));

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);
    assert!(get_cfg(&client, &issuer, &ns, &other_token).is_none());
}

#[test]
fn set_concentration_limit_attacker_issuer_address_rejected_state_unchanged() {
    let (env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);

    let attacker = Address::generate(&env);
    // Attacker uses their own address as `issuer` — OfferingId key misses.
    let r = client.try_set_concentration_limit(&attacker, &ns, &token, &9_999, &true, &1u64);
    assert_eq!(r, Err(Ok(RevoraError::LimitReached)));

    // Issuer's config untouched; attacker has no config.
    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);
    assert!(get_cfg(&client, &attacker, &ns, &token).is_none());
}

// ── Unauthorized callers ──────────────────────────────────────────────────────

#[test]
fn set_concentration_limit_unauthorized_no_auth_rejected_state_unchanged() {
    let (env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);
    let events_before = env.events().all().len();

    // Clear auth mocks: subsequent mutations require real issuer auth.
    env.set_auths(&[]);

    let r = client.try_set_concentration_limit(&issuer, &ns, &token, &1_000, &true, &99u64);
    assert!(r.is_err(), "unauthenticated set_concentration_limit must be rejected");

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);
    assert_eq!(
        env.events().all().len(),
        events_before,
        "rejected call must not emit concentration-set events"
    );
}

#[test]
fn set_concentration_limit_unauth_with_invalid_max_bps_fails_validation_first() {
    // max_bps validation runs BEFORE issuer quorum auth.
    // With auth cleared, invalid max_bps still surfaces InvalidShareBps
    // (deterministic, does not depend on auth availability).
    let (env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);
    env.set_auths(&[]);

    let r = client.try_set_concentration_limit(&issuer, &ns, &token, &10_001, &false, &0u64);
    assert_eq!(r, Err(Ok(RevoraError::InvalidShareBps)));

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);
}

#[test]
fn set_concentration_limit_multi_issuer_quorum_mocked_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let co_issuer = Address::generate(&env);
    let ns = symbol_short!("def");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(
        &issuer,
        &soroban_sdk::vec![&env, co_issuer.clone()],
        &2u32,
        &ns,
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    // mock_all_auths satisfies primary + co-issuer quorum.
    client.set_concentration_limit(&issuer, &ns, &token, &6_000, &true, &720u64);
    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 6_000, true, 720);
}

// ── Global freeze / pause ─────────────────────────────────────────────────────

#[test]
fn set_concentration_limit_contract_frozen_rejected_state_unchanged() {
    let (env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.freeze();

    let r = client.try_set_concentration_limit(&issuer, &ns, &token, &2_000, &true, &10u64);
    assert_eq!(r, Err(Ok(RevoraError::ContractFrozen)));

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);
}

#[test]
fn set_concentration_limit_contract_paused_rejected_state_unchanged() {
    let (env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.pause_admin(&admin);

    let r = client.try_set_concentration_limit(&issuer, &ns, &token, &2_000, &true, &10u64);
    assert_eq!(r, Err(Ok(RevoraError::ContractPaused)));

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);
}

#[test]
fn set_concentration_limit_frozen_takes_precedence_over_invalid_max_bps() {
    // Check order: require_not_frozen runs before max_bps validation.
    let (env, client, issuer, ns, token, _pa) = setup_offering();

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &false, &0u64);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.freeze();

    let r = client.try_set_concentration_limit(&issuer, &ns, &token, &u32::MAX, &false, &0u64);
    assert_eq!(r, Err(Ok(RevoraError::ContractFrozen)));

    assert_cfg_eq(get_cfg(&client, &issuer, &ns, &token), 5_000, false, 0);
}

// ── Event-only mode ───────────────────────────────────────────────────────────

#[test]
fn set_concentration_limit_event_only_mode_ok_but_no_storage_write() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("def");
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.initialize(&admin, &None::<Address>, &Some(true));
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns,
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    client.set_concentration_limit(&issuer, &ns, &token, &5_000, &true, &3_600u64);

    // Auth succeeds and entrypoint returns Ok, but persistent write is skipped.
    assert!(
        get_cfg(&client, &issuer, &ns, &token).is_none(),
        "event-only mode must not persist concentration config"
    );
}
