//! # Claim Transfer Failure — Atomicity Test Suite (#378)
//!
//! Verifies that a failed `try_transfer` during `claim` leaves **zero** observable
//! state change: `LastClaimedIdx` is NOT advanced, no tokens move, and the holder
//! can retry after the underlying issue is resolved.
//!
//! ## Atomicity Invariant
//!
//! ```text
//! claim:
//!   1. require_auth(holder)                    ← auth check
//!   2. blacklist / share / window checks       ← pure reads
//!   3. iterate periods, accumulate total_payout ← pure reads + accumulation
//!   4. try_transfer(contract → holder, payout)
//!      └─ FAIL → return Err(TransferFailed)    ← LastClaimedIdx NOT written
//!   5. storage().set(LastClaimedIdx)            ← only reached on success
//!   6. emit claim event                         ← only reached on success
//! ```
//!
//! If step 4 fails, step 5 is never executed, so `LastClaimedIdx` is unchanged
//! and the holder can retry the claim once the token issue is resolved.
//!
//! ## Security Note
//!
//! The ordering of `try_transfer` **before** `LastClaimedIdx` write is the critical
//! invariant. Any refactor that moves the index write above the transfer call would
//! allow a holder to mark periods as claimed without actually receiving tokens —
//! permanently losing their payout.
//!
//! ## Mock Token Design
//!
//! `FailingTransferToken` is a minimal Soroban contract implementing the standard
//! token interface. It stores a `fail_from` address; when `transfer` is called with
//! `from == fail_from`, it panics (simulating a reverting token). This lets us:
//! - Succeed on deposit (issuer → contract, `from == issuer`)
//! - Fail on claim   (contract → holder, `from == contract`)
//!
//! The token tracks balances in storage so deposit/claim balance assertions work.

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, testutils::Address as _, token, Address,
    Env, String,
};

// ══════════════════════════════════════════════════════════════════════════════
// FailingTransferToken — mock token that panics when `from == fail_from`
// ══════════════════════════════════════════════════════════════════════════════

#[contracttype]
enum TokenKey {
    Balance(Address),
    FailFrom,
}

/// Minimal token contract: supports `transfer` and `balance`.
/// Panics when `from == fail_from` (set via `set_fail_from`).
/// Implements the full Soroban token interface so `token::Client` can call it.
#[contract]
pub struct FailingTransferToken;

#[contractimpl]
impl FailingTransferToken {
    /// Configure which `from` address causes `transfer` to panic.
    pub fn set_fail_from(env: Env, fail_from: Address) {
        env.storage().persistent().set(&TokenKey::FailFrom, &fail_from);
    }

    /// Mint tokens to `to` (test helper, no auth).
    pub fn mint(env: Env, to: Address, amount: i128) {
        let bal: i128 = env.storage().persistent().get(&TokenKey::Balance(to.clone())).unwrap_or(0);
        env.storage().persistent().set(&TokenKey::Balance(to), &(bal + amount));
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage().persistent().get(&TokenKey::Balance(id)).unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        let fail_from: Option<Address> = env.storage().persistent().get(&TokenKey::FailFrom);
        if let Some(ref f) = fail_from {
            if &from == f {
                panic!("transfer intentionally failed for test");
            }
        }
        let from_bal: i128 =
            env.storage().persistent().get(&TokenKey::Balance(from.clone())).unwrap_or(0);
        env.storage().persistent().set(&TokenKey::Balance(from), &(from_bal - amount));
        let to_bal: i128 =
            env.storage().persistent().get(&TokenKey::Balance(to.clone())).unwrap_or(0);
        env.storage().persistent().set(&TokenKey::Balance(to), &(to_bal + amount));
    }

    pub fn transfer_from(
        _env: Env,
        _spender: Address,
        _from: Address,
        _to: Address,
        _amount: i128,
    ) {
        panic!("not implemented");
    }

    pub fn approve(
        _env: Env,
        _from: Address,
        _spender: Address,
        _amount: i128,
        _expiration_ledger: u32,
    ) {
    }

    pub fn allowance(_env: Env, _from: Address, _spender: Address) -> i128 {
        0
    }

    pub fn decimals(_env: Env) -> u32 {
        7
    }

    pub fn name(env: Env) -> String {
        String::from_str(&env, "FailToken")
    }

    pub fn symbol(env: Env) -> String {
        String::from_str(&env, "FAIL")
    }

    pub fn total_supply(_env: Env) -> i128 {
        0
    }

    pub fn burn(_env: Env, _from: Address, _amount: i128) {
        panic!("not implemented");
    }

    pub fn burn_from(_env: Env, _spender: Address, _from: Address, _amount: i128) {
        panic!("not implemented");
    }

    pub fn set_authorized(_env: Env, _id: Address, _authorize: bool) {}

    pub fn authorized(_env: Env, _id: Address) -> bool {
        true
    }

    pub fn clawback(_env: Env, _from: Address, _amount: i128) {
        panic!("not implemented");
    }

    pub fn set_admin(_env: Env, _new_admin: Address) {}

    pub fn admin(_env: Env) -> Address {
        panic!("not implemented");
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Test helpers
// ══════════════════════════════════════════════════════════════════════════════

fn make_revora(env: &Env) -> (Address, RevoraRevenueShareClient<'static>) {
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(env, &id);
    (id, client)
}

fn deploy_failing_token(env: &Env) -> (Address, FailingTransferTokenClient<'static>) {
    let id = env.register_contract(None, FailingTransferToken);
    let client = FailingTransferTokenClient::new(env, &id);
    (id, client)
}

fn pending_periods(
    env: &Env,
    revora_id: &Address,
    issuer: &Address,
    offering_token: &Address,
    holder: &Address,
) -> soroban_sdk::Vec<u64> {
    env.as_contract(revora_id, || {
        RevoraRevenueShare::get_pending_periods(
            env.clone(),
            issuer.clone(),
            symbol_short!("def"),
            offering_token.clone(),
            holder.clone(),
        )
    })
}

/// Full setup for claim-failure tests.
///
/// - Registers an offering with `FailingTransferToken` as payment token.
/// - Gives holder 100% share (10_000 bps).
/// - Deposits period 1 (100_000) — succeeds because fail_from is not yet set.
/// - Configures the token to fail when `from == revora_id` (claim direction).
///
/// Returns `(env, revora_id, revora, fail_token_id, fail_token, issuer, offering_token, holder)`.
fn setup_claim_fail() -> (
    Env,
    Address,
    RevoraRevenueShareClient<'static>,
    Address,
    FailingTransferTokenClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let (revora_id, revora) = make_revora(&env);
    let (fail_token_id, fail_token) = deploy_failing_token(&env);

    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let holder = Address::generate(&env);

    revora.register_offering(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &10_000,
        &fail_token_id,
        &0,
    );
    revora.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder, &10_000);

    // Mint to issuer and deposit — transfer direction is issuer→contract, not yet failing
    fail_token.mint(&issuer, &1_000_000);
    revora.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &fail_token_id,
        &100_000,
        &1,
    );

    // Now arm the token to fail when `from == revora_id` (claim: contract→holder)
    fail_token.set_fail_from(&revora_id);

    (env, revora_id, revora, fail_token_id, fail_token, issuer, offering_token, holder)
}

// ══════════════════════════════════════════════════════════════════════════════
// CLAIM TRANSFER FAILURE TESTS
// ══════════════════════════════════════════════════════════════════════════════

/// Claim transfer failure returns `TransferFailed`.
#[test]
fn claim_transfer_fail_returns_transfer_failed() {
    let (_env, _revora_id, revora, _fail_token_id, _fail_token, issuer, offering_token, holder) =
        setup_claim_fail();

    let result = revora.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);

    assert!(result.is_err(), "expected Err, got {result:?}");
    assert!(
        matches!(result.err(), Some(Ok(RevoraError::TransferFailed))),
        "expected TransferFailed"
    );
}

/// `LastClaimedIdx` is NOT advanced when claim transfer fails.
#[test]
fn claim_transfer_fail_does_not_advance_last_claimed_idx() {
    let (env, revora_id, revora, _fail_token_id, _fail_token, issuer, offering_token, holder) =
        setup_claim_fail();

    let pending_before = pending_periods(&env, &revora_id, &issuer, &offering_token, &holder);
    assert_eq!(pending_before.len(), 1, "should have 1 pending period before failed claim");

    let _ = revora.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);

    let pending_after = pending_periods(&env, &revora_id, &issuer, &offering_token, &holder);
    assert_eq!(
        pending_after.len(),
        pending_before.len(),
        "LastClaimedIdx must not advance on transfer failure"
    );
    assert_eq!(pending_after.get(0), pending_before.get(0), "pending period IDs must be unchanged");
}

/// Holder balance is unchanged when claim transfer fails.
#[test]
fn claim_transfer_fail_holder_balance_unchanged() {
    let (_env, _revora_id, revora, _fail_token_id, fail_token, issuer, offering_token, holder) =
        setup_claim_fail();

    let holder_bal_before = fail_token.balance(&holder);

    let _ = revora.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);

    assert_eq!(
        fail_token.balance(&holder),
        holder_bal_before,
        "holder balance must not change on failed claim transfer"
    );
}

/// Contract balance is unchanged when claim transfer fails.
#[test]
fn claim_transfer_fail_contract_balance_unchanged() {
    let (_env, revora_id, revora, _fail_token_id, fail_token, issuer, offering_token, holder) =
        setup_claim_fail();

    let contract_bal_before = fail_token.balance(&revora_id);

    let _ = revora.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);

    assert_eq!(
        fail_token.balance(&revora_id),
        contract_bal_before,
        "contract balance must not change on failed claim transfer"
    );
}

/// After a failed claim, the holder can retry and succeed once the token issue is resolved.
#[test]
fn claim_transfer_fail_then_retry_succeeds() {
    let (env, revora_id, revora, _fail_token_id, fail_token, issuer, offering_token, holder) =
        setup_claim_fail();

    // First attempt fails
    let r1 = revora.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert!(matches!(r1.err(), Some(Ok(RevoraError::TransferFailed))));

    // Fix the token: point fail_from at a dummy address so claim direction no longer fails
    let dummy = Address::generate(&env);
    fail_token.set_fail_from(&dummy);

    // Retry — should now succeed
    let r2 = revora.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert!(r2.is_ok(), "retry after fixing token should succeed, got {r2:?}");

    // Pending periods now empty
    let pending = pending_periods(&env, &revora_id, &issuer, &offering_token, &holder);
    assert_eq!(pending.len(), 0, "all periods should be claimed after successful retry");
}

/// Multi-period claim: all periods fail atomically — none are marked claimed.
#[test]
fn claim_transfer_fail_multi_period_no_partial_state() {
    let (env, revora_id, revora, fail_token_id, fail_token, issuer, offering_token, holder) =
        setup_claim_fail();

    // Temporarily disable fail mode to deposit two more periods
    let dummy = Address::generate(&env);
    fail_token.set_fail_from(&dummy);

    fail_token.mint(&issuer, &200_000);
    revora.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &fail_token_id,
        &100_000,
        &2,
    );
    revora.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &fail_token_id,
        &100_000,
        &3,
    );

    // Re-arm fail mode for claim direction
    fail_token.set_fail_from(&revora_id);

    let pending_before = pending_periods(&env, &revora_id, &issuer, &offering_token, &holder);
    assert_eq!(pending_before.len(), 3);

    // Attempt to claim all 3 — transfer fails
    let result = revora.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert!(matches!(result.err(), Some(Ok(RevoraError::TransferFailed))));

    // All 3 periods still pending — no partial state
    let pending_after = pending_periods(&env, &revora_id, &issuer, &offering_token, &holder);
    assert_eq!(
        pending_after.len(),
        3,
        "all 3 periods must remain pending after failed multi-period claim"
    );
}

/// A failed claim does not affect a different holder's pending state.
#[test]
fn claim_transfer_fail_does_not_affect_other_holder_state() {
    let (env, revora_id, revora, fail_token_id, fail_token, issuer, offering_token, holder) =
        setup_claim_fail();

    let holder2 = Address::generate(&env);
    // Give holder2 a share (adjust holder1 to 50% too)
    revora.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder, &5_000);
    revora.set_holder_share(&issuer, &symbol_short!("def"), &offering_token, &holder2, &5_000);

    // Deposit period 2 while fail mode is temporarily off
    let dummy = Address::generate(&env);
    fail_token.set_fail_from(&dummy);
    fail_token.mint(&issuer, &100_000);
    revora.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token,
        &fail_token_id,
        &100_000,
        &2,
    );

    // Re-arm fail mode
    fail_token.set_fail_from(&revora_id);

    // holder1 claim fails
    let r1 = revora.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token, &50);
    assert!(matches!(r1.err(), Some(Ok(RevoraError::TransferFailed))));

    // holder2 pending state is independent and unchanged
    let pending_h2 = pending_periods(&env, &revora_id, &issuer, &offering_token, &holder2);
    assert_eq!(pending_h2.len(), 2, "holder2 should still have 2 pending periods");

    // holder1 pending state also unchanged
    let pending_h1 = pending_periods(&env, &revora_id, &issuer, &offering_token, &holder);
    assert_eq!(pending_h1.len(), 2, "holder1 should still have 2 pending periods");
}

/// A failed claim on one offering does not affect a sibling offering's state.
#[test]
fn claim_transfer_fail_does_not_affect_sibling_offering() {
    let (env, revora_id, revora, _fail_token_id, _fail_token, issuer, offering_token_a, holder) =
        setup_claim_fail();

    // Register a second offering with a normal Stellar asset token
    let offering_token_b = Address::generate(&env);
    let admin_b = Address::generate(&env);


    revora.register_offering(
        &issuer,
        &symbol_short!("def"),
        &offering_token_b,
        &10_000,

        &0,
    );
    revora.set_holder_share(&issuer, &symbol_short!("def"), &offering_token_b, &holder, &10_000);
    revora.deposit_revenue(
        &issuer,
        &symbol_short!("def"),
        &offering_token_b,

        &100_000,
        &1,
    );

    // Claim on offering A fails (failing token)
    let r_a = revora.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token_a, &50);
    assert!(matches!(r_a.err(), Some(Ok(RevoraError::TransferFailed))));

    // Claim on offering B succeeds (normal token)
    let r_b = revora.try_claim(&holder, &issuer, &symbol_short!("def"), &offering_token_b, &50);
    assert!(r_b.is_ok(), "sibling offering claim must succeed, got {r_b:?}");

    // Offering A: period 1 still pending
    let pending_a = pending_periods(&env, &revora_id, &issuer, &offering_token_a, &holder);
    assert_eq!(pending_a.len(), 1, "offering A period must remain pending");

    // Offering B: no pending periods
    let pending_b = pending_periods(&env, &revora_id, &issuer, &offering_token_b, &holder);
    assert_eq!(pending_b.len(), 0, "offering B must be fully claimed");
}
