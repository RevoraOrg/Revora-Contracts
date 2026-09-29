//! Adversarial and boundary test coverage for `register_offering` in `lib.rs` (#1072).
//!
//! # Coverage Matrix
//!
//! | Scenario / Property                                                    | Expected Behavior                        |
//! |------------------------------------------------------------------------|------------------------------------------|
//! | Valid registration with minimal parameters (quorum=1, co_issuers=[])   | Ok(()), offering stored                  |
//! | Valid registration with co-issuers and quorum=2                        | Ok(()), offering stored                  |
//! | quorum=0 rejected                                                      | Err(LimitReached)                        |
//! | quorum > total_issuers rejected                                        | Err(LimitReached)                        |
//! | quorum exactly equals total_issuers accepted                           | Ok(())                                   |
//! | revenue_share_bps = 0 (zero percent) accepted                          | Ok(())                                   |
//! | revenue_share_bps = 10_000 (100%) accepted                             | Ok(())                                   |
//! | revenue_share_bps = 10_001 rejected                                    | Err(InvalidRevenueShareBps)              |
//! | revenue_share_bps = u32::MAX rejected                                  | Err(InvalidRevenueShareBps)              |
//! | supply_cap = 0 accepted (no cap)                                       | Ok(())                                   |
//! | supply_cap = i128::MAX accepted                                        | Ok(())                                   |
//! | supply_cap < 0 rejected                                                | Err(InvalidAmount)                       |
//! | supply_cap = i128::MIN rejected                                        | Err(InvalidAmount)                       |
//! | display_decimals = 18 (MAX) accepted                                   | Ok(())                                   |
//! | display_decimals > 18 rejected                                         | Err(DisplayDecimalsOutOfRange)           |
//! | display_decimals = u32::MAX rejected                                   | Err(DisplayDecimalsOutOfRange)           |
//! | Unauthorized caller (no mock_all_auths) panics                         | Auth required                            |
//! | Duplicate registration is idempotent                                   | Ok(()), count stays 1                    |
//! | State unchanged after rejected quorum=0 call                           | Offering count remains 0                 |
//! | State unchanged after rejected bps > 10_000 call                       | Offering count remains 0                 |
//! | State unchanged after rejected negative supply_cap call                | Offering count remains 0                 |
//! | State unchanged after rejected display_decimals > 18 call              | Offering count remains 0                 |
//! | Distinct issuers can register same namespace+token independently        | Both registrations succeed               |
//! | Same issuer can register different tokens in same namespace             | Both registrations succeed               |

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Vec};

// ── Test Setup Helpers ────────────────────────────────────────────────────────

fn setup_env() -> (Env, RevoraRevenueShareClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    (env, client)
}

/// Call register_offering with all valid defaults.  Returns (issuer, token).
fn register_default(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Address) {
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let payout_asset = Address::generate(env);
    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        &symbol_short!("def"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    (issuer, token)
}

// ── Happy-path tests ──────────────────────────────────────────────────────────

#[test]
fn register_offering_valid_minimal_succeeds() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &0,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_ok(), "valid minimal registration must succeed");
}

#[test]
fn register_offering_with_co_issuers_and_quorum_two_succeeds() {
    let (env, client) = setup_env();
    let primary = Address::generate(&env);
    let co1 = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let mut co_issuers = Vec::new(&env);
    co_issuers.push_back(co1);

    let result = client.try_register_offering(
        &primary,
        &co_issuers,
        &2u32, // quorum == total_issuers (1 primary + 1 co) — boundary
        &symbol_short!("ns"),
        &token,
        &500,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_ok(), "quorum equal to total_issuers must be accepted");
}

#[test]
fn register_offering_bps_zero_accepted() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &0, // 0% revenue share
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_ok(), "0 bps (0% share) must be accepted");
}

#[test]
fn register_offering_bps_exactly_10000_accepted() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &10_000, // 100% — upper boundary, must be accepted
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_ok(), "10_000 bps (100%) must be accepted");
}

#[test]
fn register_offering_supply_cap_zero_accepted() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &payout_asset,
        &0, // 0 = no cap
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_ok(), "supply_cap of 0 (no cap) must be accepted");
}

#[test]
fn register_offering_supply_cap_large_value_accepted() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &payout_asset,
        &i128::MAX, // maximum positive cap
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_ok(), "large positive supply_cap must be accepted");
}

#[test]
fn register_offering_display_decimals_18_accepted() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!("WEI"),
        &18, // maximum allowed decimals
    );
    assert!(result.is_ok(), "display_decimals = 18 (MAX) must be accepted");
}

// ── Failure-path (adversarial) tests ─────────────────────────────────────────

#[test]
fn register_offering_quorum_zero_rejected() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &0u32, // quorum=0 — invalid
        &symbol_short!("ns"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_err(), "quorum=0 must be rejected");
    assert_eq!(
        result.err().unwrap().unwrap(),
        RevoraError::LimitReached,
        "quorum=0 must return LimitReached"
    );
}

#[test]
fn register_offering_quorum_exceeds_total_issuers_rejected() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    // No co-issuers: total = 1. Quorum=2 exceeds total.
    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &2u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_err(), "quorum > total_issuers must be rejected");
    assert_eq!(
        result.err().unwrap().unwrap(),
        RevoraError::LimitReached,
        "quorum > total_issuers must return LimitReached"
    );
}

#[test]
fn register_offering_bps_over_10000_rejected() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &10_001, // just over the 10_000 limit
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_err(), "revenue_share_bps > 10_000 must be rejected");
    assert_eq!(
        result.err().unwrap().unwrap(),
        RevoraError::InvalidRevenueShareBps,
        "bps > 10_000 must return InvalidRevenueShareBps"
    );
}

#[test]
fn register_offering_bps_max_u32_rejected() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &u32::MAX,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_err(), "u32::MAX bps must be rejected");
    assert_eq!(
        result.err().unwrap().unwrap(),
        RevoraError::InvalidRevenueShareBps,
        "u32::MAX bps must return InvalidRevenueShareBps"
    );
}

#[test]
fn register_offering_negative_supply_cap_rejected() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &payout_asset,
        &-1, // negative supply cap — invalid
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_err(), "negative supply_cap must be rejected");
    assert_eq!(
        result.err().unwrap().unwrap(),
        RevoraError::InvalidAmount,
        "negative supply_cap must return InvalidAmount"
    );
}

#[test]
fn register_offering_min_i128_supply_cap_rejected() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &payout_asset,
        &i128::MIN,
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_err(), "i128::MIN supply_cap must be rejected");
    assert_eq!(
        result.err().unwrap().unwrap(),
        RevoraError::InvalidAmount,
        "i128::MIN supply_cap must return InvalidAmount"
    );
}

#[test]
fn register_offering_display_decimals_19_rejected() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!("TOK"),
        &19, // one over the max of 18
    );
    assert!(result.is_err(), "display_decimals = 19 must be rejected");
    assert_eq!(
        result.err().unwrap().unwrap(),
        RevoraError::DisplayDecimalsOutOfRange,
        "display_decimals > 18 must return DisplayDecimalsOutOfRange"
    );
}

#[test]
fn register_offering_display_decimals_max_u32_rejected() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!("TOK"),
        &u32::MAX,
    );
    assert!(result.is_err(), "u32::MAX display_decimals must be rejected");
    assert_eq!(
        result.err().unwrap().unwrap(),
        RevoraError::DisplayDecimalsOutOfRange,
        "u32::MAX display_decimals must return DisplayDecimalsOutOfRange"
    );
}

// ── State-unchanged-after-rejection tests ────────────────────────────────────

/// State must be unchanged after a rejected call: offering count stays 0.
#[test]
fn state_unchanged_after_quorum_zero_rejection() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let namespace = symbol_short!("ns");

    let _ = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &0u32, // invalid
        &namespace,
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    let count = client.get_offering_count(&issuer, &namespace);
    assert_eq!(count, 0, "offering count must remain 0 after quorum=0 rejection");
}

#[test]
fn state_unchanged_after_bps_rejection() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let namespace = symbol_short!("ns");

    let _ = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &99_999, // invalid bps
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    let count = client.get_offering_count(&issuer, &namespace);
    assert_eq!(count, 0, "offering count must remain 0 after invalid bps rejection");
}

#[test]
fn state_unchanged_after_negative_supply_cap_rejection() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let namespace = symbol_short!("ns");

    let _ = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &1_000,
        &payout_asset,
        &-100, // invalid
        &symbol_short!(""),
        &0,
    );

    let count = client.get_offering_count(&issuer, &namespace);
    assert_eq!(count, 0, "offering count must remain 0 after negative supply_cap rejection");
}

#[test]
fn state_unchanged_after_display_decimals_rejection() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let namespace = symbol_short!("ns");

    let _ = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!("TOK"),
        &255, // invalid
    );

    let count = client.get_offering_count(&issuer, &namespace);
    assert_eq!(count, 0, "offering count must remain 0 after invalid display_decimals rejection");
}

// ── Idempotency tests ─────────────────────────────────────────────────────────

#[test]
fn register_offering_duplicate_is_idempotent() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let namespace = symbol_short!("ns");

    // Register once
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert_eq!(client.get_offering_count(&issuer, &namespace), 1);

    // Register again with same (issuer, namespace, token) — must be idempotent
    let result = client.try_register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &5_000, // different bps, but same identity key — still ignored
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    assert!(result.is_ok(), "duplicate registration must return Ok (idempotent)");

    // Count must not increase
    assert_eq!(
        client.get_offering_count(&issuer, &namespace),
        1,
        "duplicate registration must not increment offering count"
    );
}

// ── Isolation tests ───────────────────────────────────────────────────────────

#[test]
fn distinct_issuers_can_register_same_namespace_and_token() {
    let (env, client) = setup_env();
    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let namespace = symbol_short!("shared");

    client.register_offering(
        &issuer_a,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    client.register_offering(
        &issuer_b,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token,
        &2_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    assert_eq!(
        client.get_offering_count(&issuer_a, &namespace),
        1,
        "issuer_a should have 1 offering"
    );
    assert_eq!(
        client.get_offering_count(&issuer_b, &namespace),
        1,
        "issuer_b should have 1 offering independently"
    );
}

#[test]
fn same_issuer_different_tokens_in_same_namespace_both_registered() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let namespace = symbol_short!("ns");

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token_a,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace,
        &token_b,
        &2_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    assert_eq!(
        client.get_offering_count(&issuer, &namespace),
        2,
        "two distinct tokens under the same issuer+namespace must each be counted"
    );
}

#[test]
fn same_issuer_and_token_different_namespaces_both_registered() {
    let (env, client) = setup_env();
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);
    let ns_a = symbol_short!("alpha");
    let ns_b = symbol_short!("beta");

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns_a,
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &ns_b,
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );

    assert_eq!(
        client.get_offering_count(&issuer, &ns_a),
        1,
        "namespace alpha must have its own count"
    );
    assert_eq!(
        client.get_offering_count(&issuer, &ns_b),
        1,
        "namespace beta must have its own count"
    );
}

// ── Authorization tests ───────────────────────────────────────────────────────

#[test]
#[should_panic]
fn register_offering_without_auth_panics() {
    // Deliberately do NOT call env.mock_all_auths() — auth should be enforced.
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    // This must panic because primary_issuer.require_auth() is called
    // and no auth mock is in place.
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &token,
        &1_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
}

// ── Error code stability tests ────────────────────────────────────────────────

/// These tests verify that the numeric error codes exposed to integrators
/// remain stable.  Changing these values is a breaking change.
#[test]
fn error_code_invalid_revenue_share_bps_is_1() {
    assert_eq!(RevoraError::InvalidRevenueShareBps as u32, 1);
}

#[test]
fn error_code_limit_reached_is_2() {
    assert_eq!(RevoraError::LimitReached as u32, 2);
}

#[test]
fn error_code_invalid_amount_is_21() {
    assert_eq!(RevoraError::InvalidAmount as u32, 21);
}

#[test]
fn error_code_display_decimals_out_of_range_is_51() {
    assert_eq!(RevoraError::DisplayDecimalsOutOfRange as u32, 51);
}
