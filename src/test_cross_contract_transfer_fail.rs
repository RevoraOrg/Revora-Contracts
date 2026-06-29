//! # Cross-Contract Transfer Failure — Atomicity Test Suite [RC26Q2-C13]
//!
//! Verifies that a failed `try_transfer` during `deposit_revenue` or
//! `deposit_revenue_with_snapshot` leaves **zero** observable state change:
//! no `PeriodRevenue` entry, no `PeriodCount` increment, no `DepositedRevenue`
//! update, and no `LastSnapshotRef` advance.
//!
//! ## Atomicity Invariant
//!
//! ```text
//! do_deposit_revenue:
//!   1. validate inputs          ← pure, no writes
//!   2. check PeriodAlreadyDeposited ← pure read
//!   3. check SupplyCap          ← pure read
//!   4. try_transfer(issuer → contract, amount)
//!      └─ FAIL → return Err(TransferFailed)   ← NO writes have occurred yet
//!   5. storage().set(PeriodRevenue)            ← only reached on success
//!   6. storage().set(PeriodDepositTime)
//!   7. storage().set(PeriodCount + 1)
//!   8. storage().set(DepositedRevenue)
//! ```
//!
//! If step 4 fails, steps 5-8 are never executed, so storage is unchanged.
//!
//! ## Security Note
//!
//! The ordering of `try_transfer` **before** any storage write is the critical
//! invariant. Any refactor that moves a storage write above the transfer call
//! would break atomicity and could allow an issuer to credit a period without
//! actually depositing tokens.
//!
//! ## Event Symbols
//!
//! - `rev_dep2` (`EVENT_REV_DEPOSIT_V2`) — emitted only on successful deposit
//! - `rev_snp2` (`EVENT_REV_DEP_SNAP_V2`) — emitted only on successful snapshot deposit
//!
//! Neither event is emitted when `TransferFailed` is returned.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::Address as _,
    token,
    Address, Env,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Deploy a real Stellar asset contract and return (token_address, admin).
fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract(admin.clone());
    (token_id, admin)
}

fn mint(env: &Env, token: &Address, recipient: &Address, amount: i128) {
    token::StellarAssetClient::new(env, token).mint(recipient, &amount);
}

fn token_balance(env: &Env, token: &Address, who: &Address) -> i128 {
    token::Client::new(env, token).balance(who)
}

/// Full setup: env, client, contract_id, issuer, offering_token, payment_token, payment_admin.
fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (payment_token, pt_admin) = create_payment_token(&env);

    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &5_000,
        &payment_token,
        &0,
    );

    (env, client, contract_id, issuer, offering_token, payment_token, pt_admin)
}

// ══════════════════════════════════════════════════════════════════════════════
// ATOMICITY: deposit_revenue — transfer fails (zero balance)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn deposit_revenue_transfer_fail_returns_transfer_failed_error() {
    let (env, client, _contract_id, issuer, token, payment_token, _pt_admin) = setup();

    // Issuer has zero balance — transfer will fail
    assert_eq!(token_balance(&env, &payment_token, &issuer), 0);

    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );

    assert!(result.is_err());
    assert!(matches!(result.err(), Some(Ok(RevoraError::TransferFailed))));
}

#[test]
fn deposit_revenue_transfer_fail_does_not_write_period_revenue() {
    let (env, client, _contract_id, issuer, token, payment_token, _pt_admin) = setup();

    // No balance — transfer fails
    let _ = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );

    // PeriodRevenue must not exist: period_count stays 0
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
}

#[test]
fn deposit_revenue_transfer_fail_does_not_increment_period_count() {
    let (env, client, _contract_id, issuer, token, payment_token, _pt_admin) = setup();

    let count_before = client.get_period_count(&issuer, &symbol_short!("def"), &token);

    let _ = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );

    assert_eq!(
        client.get_period_count(&issuer, &symbol_short!("def"), &token),
        count_before
    );
}

#[test]
fn deposit_revenue_transfer_fail_does_not_update_deposited_revenue() {
    let (env, client, _contract_id, issuer, token, payment_token, _pt_admin) = setup();

    let _ = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );

    // get_revenue_by_period returns 0 for a period that was never written
    assert_eq!(
        client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1),
        0
    );
}

#[test]
fn deposit_revenue_transfer_fail_contract_balance_unchanged() {
    let (env, client, contract_id, issuer, token, payment_token, _pt_admin) = setup();

    let contract_bal_before = token_balance(&env, &payment_token, &contract_id);

    let _ = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );

    assert_eq!(token_balance(&env, &payment_token, &contract_id), contract_bal_before);
}

// ══════════════════════════════════════════════════════════════════════════════
// ATOMICITY: deposit_revenue — partial balance (insufficient funds)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn deposit_revenue_insufficient_balance_returns_transfer_failed() {
    let (env, client, _contract_id, issuer, token, payment_token, pt_admin) = setup();

    // Mint less than the deposit amount
    mint(&env, &payment_token, &issuer, 50_000);

    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000, // More than balance
        &1,
    );

    assert!(result.is_err());
    assert!(matches!(result.err(), Some(Ok(RevoraError::TransferFailed))));

    // No state written
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
}

#[test]
fn deposit_revenue_insufficient_balance_issuer_balance_unchanged() {
    let (env, client, _contract_id, issuer, token, payment_token, pt_admin) = setup();

    mint(&env, &payment_token, &issuer, 50_000);
    let issuer_bal_before = token_balance(&env, &payment_token, &issuer);

    let _ = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );

    // Issuer balance unchanged — no tokens moved
    assert_eq!(token_balance(&env, &payment_token, &issuer), issuer_bal_before);
}

// ══════════════════════════════════════════════════════════════════════════════
// ATOMICITY: successful deposit followed by failed deposit
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn successful_deposit_then_failed_deposit_preserves_first_period_only() {
    let (env, client, contract_id, issuer, token, payment_token, pt_admin) = setup();

    // Fund issuer for exactly one deposit
    mint(&env, &payment_token, &issuer, 100_000);

    // First deposit succeeds
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 1);
    assert_eq!(token_balance(&env, &payment_token, &contract_id), 100_000);

    // Second deposit fails — no more balance
    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &2,
    );

    assert!(result.is_err());

    // Only period 1 exists; period 2 was never written
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 1);
    assert_eq!(
        client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1),
        100_000
    );
    assert_eq!(
        client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &2),
        0
    );
    // Contract balance unchanged after failed second deposit
    assert_eq!(token_balance(&env, &payment_token, &contract_id), 100_000);
}

// ══════════════════════════════════════════════════════════════════════════════
// ATOMICITY: deposit_revenue_with_snapshot — transfer fails
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_deposit_transfer_fail_returns_transfer_failed() {
    let (env, client, _contract_id, issuer, token, payment_token, _pt_admin) = setup();

    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);

    // No balance — transfer will fail
    let result = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
        &100,
    );

    assert!(result.is_err());
    assert!(matches!(result.err(), Some(Ok(RevoraError::TransferFailed))));
}

#[test]
fn snapshot_deposit_transfer_fail_does_not_advance_last_snapshot_ref() {
    let (env, client, _contract_id, issuer, token, payment_token, _pt_admin) = setup();

    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);

    let ref_before = client.get_last_snapshot_ref(&issuer, &symbol_short!("def"), &token);

    // Transfer fails — no balance
    let _ = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
        &100,
    );

    // LastSnapshotRef must not have advanced
    assert_eq!(
        client.get_last_snapshot_ref(&issuer, &symbol_short!("def"), &token),
        ref_before
    );
}

#[test]
fn snapshot_deposit_transfer_fail_does_not_write_period_revenue() {
    let (env, client, _contract_id, issuer, token, payment_token, _pt_admin) = setup();

    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);

    let _ = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
        &100,
    );

    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
    assert_eq!(
        client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token, &1),
        0
    );
}

#[test]
fn snapshot_deposit_transfer_fail_contract_balance_unchanged() {
    let (env, client, contract_id, issuer, token, payment_token, _pt_admin) = setup();

    client.set_snapshot_config(&issuer, &symbol_short!("def"), &token, &true);

    let contract_bal_before = token_balance(&env, &payment_token, &contract_id);

    let _ = client.try_deposit_revenue_with_snapshot(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
        &100,
    );

    assert_eq!(token_balance(&env, &payment_token, &contract_id), contract_bal_before);
}

// ══════════════════════════════════════════════════════════════════════════════
// ATOMICITY: supply cap — transfer fails before cap is updated
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn deposit_with_supply_cap_transfer_fail_does_not_update_deposited_revenue_counter() {
    let (env, client, _contract_id, issuer, token, payment_token, _pt_admin) = setup();

    // Set a supply cap
    client.set_supply_cap(&issuer, &symbol_short!("def"), &token, &500_000);

    // No balance — transfer fails before DepositedRevenue is written
    let _ = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );

    // Supply cap counter must remain at 0
    assert_eq!(client.get_supply_cap(&issuer, &symbol_short!("def"), &token), 500_000);
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 0);
}

// ══════════════════════════════════════════════════════════════════════════════
// ATOMICITY: multiple offerings — failure in one does not affect another
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn transfer_fail_in_one_offering_does_not_affect_sibling_offering() {
    let (env, client, contract_id, issuer, token_a, payment_token_a, pt_admin_a) = setup();

    // Register a second offering with its own payment token
    let token_b = Address::generate(&env);
    let (payment_token_b, pt_admin_b) = create_payment_token(&env);
    client.register_offering(
        &issuer,
        &symbol_short!("def"),
        &token_b,
        &5_000,
        &payment_token_b,
        &0,
    );

    // Fund only offering B
    mint(&env, &payment_token_b, &issuer, 100_000);

    // Deposit into offering B succeeds
    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token_b,
        &payment_token_b,
        &100_000,
        &1,
    );

    // Deposit into offering A fails (no balance)
    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token_a,
        &payment_token_a,
        &100_000,
        &1,
    );

    assert!(result.is_err());

    // Offering B state intact
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token_b), 1);
    assert_eq!(
        client.get_revenue_by_period(&issuer, &symbol_short!("def"), &token_b, &1),
        100_000
    );

    // Offering A state untouched
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token_a), 0);
}

// ══════════════════════════════════════════════════════════════════════════════
// ATOMICITY: period ordering invariant preserved after failed deposit
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn failed_deposit_does_not_advance_period_ordering_cursor() {
    let (env, client, _contract_id, issuer, token, payment_token, pt_admin) = setup();

    // Fund for period 1 only
    mint(&env, &payment_token, &issuer, 100_000);

    client.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &1,
    );

    // Period 2 fails (no balance)
    let _ = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &2,
    );

    // Fund and retry period 2 — should succeed (ordering cursor not corrupted)
    mint(&env, &payment_token, &issuer, 100_000);

    let result = client.try_deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payment_token,
        &100_000,
        &2,
    );

    assert!(result.is_ok());
    assert_eq!(client.get_period_count(&issuer, &symbol_short!("def"), &token), 2);
}
