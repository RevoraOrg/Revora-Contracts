//! Adversarial coverage for `deposit_revenue_with_snapshot` (issue #1133).
//!
//! `deposit_revenue_with_snapshot` folds snapshot-reference validation, issuer
//! authorization, replay protection (strictly increasing references) and the
//! core revenue-deposit path into a single entry point. These tests pin the
//! success path and every documented rejection path, and assert that a rejected
//! call leaves the persisted snapshot reference untouched.
//!
//! | Case                                        | Expected outcome                    |
//! |---------------------------------------------|-------------------------------------|
//! | valid deposit with a fresh reference        | `Ok`, reference persisted           |
//! | second deposit with a higher reference      | `Ok`, reference advances            |
//! | snapshot reference `0`                      | `InvalidAmount`, no state change    |
//! | reference equal to the last one             | `OutdatedSnapshot`, no state change |
//! | reference below the last one                | `OutdatedSnapshot`, no state change |
//! | duplicate period id                         | `PeriodAlreadyDeposited`, ref kept  |
//! | zero period id                              | `InvalidPeriodId`, no state change  |
//! | zero / negative amount                      | `InvalidAmount`, no state change    |
//! | snapshots never enabled                     | `SnapshotNotEnabled`, no state      |
//! | snapshots disabled after a deposit          | `SnapshotNotEnabled`, ref kept      |
//! | caller is not the offering issuer           | `SnapshotNotEnabled`, no state      |

#![cfg(test)]

use super::*;
use soroban_sdk::testutils::Address as _;

/// Namespace shared by every offering registered in this module.
fn namespace() -> Symbol {
    symbol_short!("ns")
}

/// Register a single-issuer offering backed by a real Stellar asset contract so
/// that the token transfer inside `do_deposit_revenue` can succeed.
fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payment_token = env.register_stellar_asset_contract_v2(issuer.clone()).address();

    client.initialize(&issuer, &None::<Address>, &None::<bool>);
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &namespace(),
        &offering_token,
        &10_000u32,
        &payment_token,
        &0i128,
        &symbol_short!(""),
        // Must match the Stellar Asset Contract's `decimals()` (7), otherwise
        // `register_offering` rejects the offering with `DecimalsMismatch`.
        &7u32,
    );
    token::StellarAssetClient::new(&env, &payment_token).mint(&issuer, &1_000_000);

    (env, client, issuer, offering_token, payment_token)
}

#[test]
fn deposit_with_snapshot_records_the_reference() {
    let (_env, client, issuer, token, payment_token) = setup();
    client.set_snapshot_config(&issuer, &namespace(), &token, &true);

    client.deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &1,
        &7,
    );

    assert!(client.get_snapshot_config(&issuer, &namespace(), &token));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 7);
}

#[test]
fn deposit_with_snapshot_advances_across_increasing_references() {
    let (_env, client, issuer, token, payment_token) = setup();
    client.set_snapshot_config(&issuer, &namespace(), &token, &true);

    client.deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &1,
        &1,
    );
    client.deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &2_000,
        &2,
        &2,
    );

    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 2);
}

#[test]
fn deposit_with_snapshot_rejects_zero_reference_without_state_change() {
    let (_env, client, issuer, token, payment_token) = setup();
    client.set_snapshot_config(&issuer, &namespace(), &token, &true);

    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &1,
        &0,
    );

    assert!(matches!(result.err(), Some(Ok(RevoraError::InvalidAmount))));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 0);
}

#[test]
fn deposit_with_snapshot_rejects_replayed_reference() {
    let (_env, client, issuer, token, payment_token) = setup();
    client.set_snapshot_config(&issuer, &namespace(), &token, &true);
    client.deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &1,
        &5,
    );

    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &2,
        &5,
    );

    assert!(matches!(result.err(), Some(Ok(RevoraError::OutdatedSnapshot))));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 5);

    // The rejected call must not have consumed period 2: a higher reference works.
    client.deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &2,
        &6,
    );
    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 6);
}

#[test]
fn deposit_with_snapshot_rejects_decreasing_reference() {
    let (_env, client, issuer, token, payment_token) = setup();
    client.set_snapshot_config(&issuer, &namespace(), &token, &true);
    client.deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &1,
        &9,
    );

    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &2,
        &4,
    );

    assert!(matches!(result.err(), Some(Ok(RevoraError::OutdatedSnapshot))));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 9);
}

#[test]
fn deposit_with_snapshot_rejects_duplicate_period() {
    let (_env, client, issuer, token, payment_token) = setup();
    client.set_snapshot_config(&issuer, &namespace(), &token, &true);
    client.deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &1,
        &1,
    );

    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &1,
        &2,
    );

    assert!(matches!(result.err(), Some(Ok(RevoraError::PeriodAlreadyDeposited))));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 1);
}

#[test]
fn deposit_with_snapshot_rejects_zero_period_id() {
    let (_env, client, issuer, token, payment_token) = setup();
    client.set_snapshot_config(&issuer, &namespace(), &token, &true);

    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &0,
        &1,
    );

    assert!(matches!(result.err(), Some(Ok(RevoraError::InvalidPeriodId))));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 0);
}

#[test]
fn deposit_with_snapshot_rejects_non_positive_amounts() {
    let (_env, client, issuer, token, payment_token) = setup();
    client.set_snapshot_config(&issuer, &namespace(), &token, &true);

    let zero = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &0,
        &1,
        &1,
    );
    assert!(matches!(zero.err(), Some(Ok(RevoraError::InvalidAmount))));

    let negative = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &-1,
        &1,
        &1,
    );
    assert!(matches!(negative.err(), Some(Ok(RevoraError::InvalidAmount))));

    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 0);
}

#[test]
fn deposit_with_snapshot_rejects_when_snapshots_are_disabled() {
    let (_env, client, issuer, token, payment_token) = setup();

    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &1,
        &1,
    );

    assert!(matches!(result.err(), Some(Ok(RevoraError::SnapshotNotEnabled))));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 0);
}

#[test]
fn deposit_with_snapshot_rejects_after_snapshots_are_disabled() {
    let (_env, client, issuer, token, payment_token) = setup();
    client.set_snapshot_config(&issuer, &namespace(), &token, &true);
    client.deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &1,
        &1,
    );
    client.set_snapshot_config(&issuer, &namespace(), &token, &false);

    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &2,
        &2,
    );

    assert!(matches!(result.err(), Some(Ok(RevoraError::SnapshotNotEnabled))));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 1);
}

#[test]
fn deposit_with_snapshot_rejects_non_issuer_caller() {
    let (env, client, issuer, token, payment_token) = setup();
    client.set_snapshot_config(&issuer, &namespace(), &token, &true);
    let stranger = Address::generate(&env);

    let result = client.try_deposit_revenue_with_snapshot(
        &stranger,
        &namespace(),
        &token,
        &payment_token,
        &1_000,
        &1,
        &1,
    );

    // The snapshot config is keyed by the calling issuer, so a non-issuer never
    // reaches the deposit path and cannot advance any offering state.
    assert!(matches!(result.err(), Some(Ok(RevoraError::SnapshotNotEnabled))));
    assert_eq!(client.get_last_snapshot_ref(&issuer, &namespace(), &token), 0);
}
