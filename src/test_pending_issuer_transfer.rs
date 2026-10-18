//! # Adversarial coverage for `get_pending_issuer_transfer`
//!
//! `get_pending_issuer_transfer(env, issuer, namespace, token) -> Option<Address>`
//! is the public read-only projection of the pending issuer-transfer record keyed
//! by `OfferingId { issuer, namespace, token }`. It is the oracle every issuer
//! transfer UI and off-chain indexer relies on, so the wrong value here means a
//! wrong accept/cancel decision downstream.
//!
//! ## Why adversarial coverage is needed
//!
//! The function has no auth gate and no explicit error path, but its *output*
//! encodes authorization and lifecycle state:
//!
//! - It must return `None` for unknown/invalid `OfferingId`s (wrong issuer,
//!   wrong namespace, wrong token, unknown tenants) rather than leaking any
//!   pending record from a neighboring tenant.
//! - It must observe the full proposal lifecycle: `None` before proposal,
//!   `Some(new_issuer)` while pending, `None` again after accept / cancel /
//!   reject, and the *replacement* value after `replace_issuer_transfer`.
//! - It must not mutate state: repeated calls, calls through fresh client
//!   handles (Soroban reads have no caller), and calls around re-proposals
//!   must be stable and side-effect free.
//! - After expiry (ledger time advanced past the effective deadline) the
//!   record stays visible until an accept/cancel/reject clears it — the
//!   deadline is enforced by `accept_issuer_transfer`, not by the reader. This
//!   is deterministic because Soroban tests control `env.ledger().timestamp()`.
//! - It must read the same underlying record that
//!   `get_pending_transfer_details` exposes in full, i.e. the projected
//!   `new_issuer` always equals `details.unwrap().new_issuer`.
//!
//! ## Coverage matrix
//!
//! | # | Scenario | Expected |
//! |---|----------|----------|
//! | 1 | Uninitialized contract, arbitrary ids | `None` |
//! | 2 | Registered offering, no transfer proposed | `None` |
//! | 3 | After `propose_issuer_transfer` | `Some(new_issuer)` |
//! | 4 | Idempotent repeats / fresh client handles | stable, state unchanged |
//! | 5 | Unknown issuer / namespace / token variants | `None`, pending untouched |
//! | 6 | Two offerings, same namespace, different issuers/tokens | per-tenant isolation |
//! | 7 | Two namespaces, same issuer+token | per-namespace isolation |
//! | 8 | After `replace_issuer_transfer` | `Some(new_new_issuer)` |
//! | 9 | After `cancel_issuer_transfer` | `None` |
//! | 10 | After `accept_issuer_transfer` | `None` |
//! | 11 | After `reject_issuer_transfer` (new-issuer side) | `None` |
//! | 12 | Failed replace/cancel/accept (no pending) | `None`, state unchanged |
//! | 13 | Failed proposal (double → IssuerTransferPending) | value unchanged |
//! | 14 | Failed proposal (unknown offering → OfferingNotFound) | no state |
//! | 15 | Expired pending (custom 1h + default 7d) | still visible; accept fails |
//! | 16 | Consistency with `get_pending_transfer_details` | `new_issuer` projection equal |
//!
//! ## Security / risk notes
//!
//! - The reader is unauthenticated **by design** (AUTH_MATRIX.md: "no auth"):
//!   pending-transfer data is public. These tests pin that contract: any future
//!   change that makes the reader auth-gated or value-filtered is a breaking
//!   change and must update this module deliberately.
//! - All failure paths assert both the returned error (via `try_*`) and that
//!   the pending value is unchanged afterwards, so a rejected operation can
//!   never silently mutate observed state.

#![cfg(test)]

use crate::{DataKey2, PendingTransfer, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger},
    Address, Env, Symbol, Vec,
};

const NS: Symbol = symbol_short!("def");

/// One registered offering with its own token + payout asset.
struct Tenant {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    issuer: Address,
    token: Address,
}

/// Read helper mirroring the function under test.
fn pending(t: &Tenant) -> Option<Address> {
    t.client.get_pending_issuer_transfer(&t.issuer, &NS, &t.token)
}

/// Create client + one registered offering under `NS`.
fn setup() -> Tenant {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout_asset = Address::generate(&env);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &NS,
        &token,
        &5_000u32,
        &payout_asset,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );

    // Seed the global issuer/namespace registries so `accept_issuer_transfer` /
    // `reject_issuer_transfer` (which scan via `find_pending_transfer_for_new_issuer`)
    // can resolve the pending transfer. `register_offering` on master does not
    // populate these indices; this mirrors `issue_370_*` test setup in src/lib.rs.
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey2::IssuerCount, &1_u32);
        env.storage().persistent().set(&DataKey2::IssuerItem(0), &issuer);
        env.storage().persistent().set(&DataKey2::IssuerRegistered(issuer.clone()), &true);
        env.storage().persistent().set(&DataKey2::NamespaceCount(issuer.clone()), &1_u32);
        env.storage().persistent().set(&DataKey2::NamespaceItem(issuer.clone(), 0), &NS);
        env.storage().persistent().set(&DataKey2::NamespaceRegistered(issuer.clone(), NS), &true);
    });

    Tenant { env, client, issuer, token }
}

// ─── Success paths ───────────────────────────────────────────────────────────

#[test]
fn pending_returns_none_before_any_proposal() {
    let t = setup();
    assert_eq!(pending(&t), None);
}

#[test]
fn pending_returns_proposed_new_issuer() {
    let t = setup();
    let new_issuer = Address::generate(&t.env);

    t.client.propose_issuer_transfer(&t.issuer, &NS, &t.token, &new_issuer);

    assert_eq!(pending(&t), Some(new_issuer));
}

#[test]
fn pending_is_read_only_and_idempotent() {
    let t = setup();
    let new_issuer = Address::generate(&t.env);
    t.client.propose_issuer_transfer(&t.issuer, &NS, &t.token, &new_issuer);

    // Repeated reads return the same value...
    assert_eq!(pending(&t), Some(new_issuer.clone()));
    assert_eq!(pending(&t), Some(new_issuer.clone()));

    // ...including through a freshly constructed client handle over the same
    // contract address, i.e. the read has no hidden per-handle state.
    let client2 = RevoraRevenueShareClient::new(&t.env, &t.client.address);
    assert_eq!(client2.get_pending_issuer_transfer(&t.issuer, &NS, &t.token), Some(new_issuer));
}

#[test]
fn pending_details_projection_matches_pending_issuer() {
    let t = setup();
    let new_issuer = Address::generate(&t.env);
    let before = t.env.ledger().timestamp();

    t.client.propose_issuer_transfer(&t.issuer, &NS, &t.token, &new_issuer);

    let details: PendingTransfer =
        t.client.get_pending_transfer_details(&t.issuer, &NS, &t.token).unwrap();
    assert_eq!(details.new_issuer, new_issuer);
    assert_eq!(details.timestamp, before);
    // Default proposal (expiry_secs = 0) means "use the 7-day default".
    assert_eq!(details.expiry_secs, 0);

    // The projection under test is exactly `details.map(|d| d.new_issuer)`.
    assert_eq!(pending(&t), Some(details.new_issuer));
}

// ─── Invalid / boundary identifiers ──────────────────────────────────────────

#[test]
fn pending_unknown_ids_return_none_and_leave_state_unchanged() {
    let t = setup();
    let new_issuer = Address::generate(&t.env);
    t.client.propose_issuer_transfer(&t.issuer, &NS, &t.token, &new_issuer);

    // Unknown issuer — read-only miss must not disturb the real record.
    let stranger = Address::generate(&t.env);
    assert_eq!(t.client.get_pending_issuer_transfer(&stranger, &NS, &t.token), None);
    assert_eq!(pending(&t), Some(new_issuer.clone()));

    // Unknown namespace.
    assert_eq!(
        t.client.get_pending_issuer_transfer(&t.issuer, &symbol_short!("nope"), &t.token),
        None
    );
    assert_eq!(pending(&t), Some(new_issuer.clone()));

    // Unknown token.
    let other_token = Address::generate(&t.env);
    assert_eq!(t.client.get_pending_issuer_transfer(&t.issuer, &NS, &other_token), None);
    assert_eq!(pending(&t), Some(new_issuer.clone()));

    // Fully fabricated (issuer, namespace, token) triple.
    assert_eq!(
        t.client.get_pending_issuer_transfer(
            &Address::generate(&t.env),
            &symbol_short!("zz"),
            &Address::generate(&t.env)
        ),
        None
    );
    assert_eq!(pending(&t), Some(new_issuer));
}

#[test]
fn pending_is_isolated_per_offering_identity() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout = Address::generate(&env);

    for (issuer, token) in [(&issuer_a, &token_a), (&issuer_b, &token_b)] {
        client.register_offering(
            issuer,
            &Vec::new(&env),
            &1u32,
            &NS,
            token,
            &5_000u32,
            &payout,
            &0i128,
            &symbol_short!(""),
            &0u32,
        );
    }

    let new_a = Address::generate(&env);
    client.propose_issuer_transfer(&issuer_a, &NS, &token_a, &new_a);

    // Tenant B must not see tenant A's pending transfer.
    assert_eq!(client.get_pending_issuer_transfer(&issuer_a, &NS, &token_a), Some(new_a.clone()));
    assert_eq!(client.get_pending_issuer_transfer(&issuer_b, &NS, &token_a), None);
    assert_eq!(client.get_pending_issuer_transfer(&issuer_a, &NS, &token_b), None);
    assert_eq!(client.get_pending_issuer_transfer(&issuer_b, &NS, &token_b), None);

    // A second, independent namespace for the same issuer+token.
    client.register_offering(
        &issuer_a,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("alt"),
        &token_a,
        &5_000u32,
        &payout,
        &0i128,
        &symbol_short!(""),
        &0u32,
    );
    let new_alt = Address::generate(&env);
    client.propose_issuer_transfer(&issuer_a, &symbol_short!("alt"), &token_a, &new_alt);

    assert_eq!(client.get_pending_issuer_transfer(&issuer_a, &NS, &token_a), Some(new_a));
    assert_eq!(
        client.get_pending_issuer_transfer(&issuer_a, &symbol_short!("alt"), &token_a),
        Some(new_alt)
    );
}

// ─── Lifecycle transitions observed through the reader ───────────────────────

#[test]
fn pending_reflects_replace_then_none_after_cancel() {
    let t = setup();
    let first = Address::generate(&t.env);
    let second = Address::generate(&t.env);

    t.client.propose_issuer_transfer(&t.issuer, &NS, &t.token, &first);
    assert_eq!(pending(&t), Some(first));

    t.client.replace_issuer_transfer(&t.issuer, &NS, &t.token, &second);
    assert_eq!(pending(&t), Some(second));

    t.client.cancel_issuer_transfer(&t.issuer, &NS, &t.token);
    assert_eq!(pending(&t), None);
}

#[test]
fn pending_clears_after_accept() {
    let t = setup();
    let new_issuer = Address::generate(&t.env);
    t.client.propose_issuer_transfer(&t.issuer, &NS, &t.token, &new_issuer);
    assert_eq!(pending(&t), Some(new_issuer.clone()));

    t.client.accept_issuer_transfer(&new_issuer, &NS, &t.token);
    assert_eq!(pending(&t), None);

    // And the offering now lives under the new issuer — no pending anywhere.
    assert!(t.client.get_offering(&new_issuer, &NS, &t.token).is_some());
}

#[test]
fn pending_clears_after_reject() {
    let t = setup();
    let new_issuer = Address::generate(&t.env);
    t.client.propose_issuer_transfer(&t.issuer, &NS, &t.token, &new_issuer);
    assert_eq!(pending(&t), Some(new_issuer.clone()));

    t.client.reject_issuer_transfer(&new_issuer, &NS, &t.token);
    assert_eq!(pending(&t), None);
}

// ─── Rejected operations leave observed state unchanged ──────────────────────

#[test]
fn failed_replace_without_pending_leaves_state_unchanged() {
    let t = setup();
    let err = t
        .client
        .try_replace_issuer_transfer(&t.issuer, &NS, &t.token, &Address::generate(&t.env))
        .unwrap_err();
    assert_eq!(err, Ok(RevoraError::NoTransferPending));
    assert_eq!(pending(&t), None);
}

#[test]
fn failed_cancel_without_pending_leaves_state_unchanged() {
    let t = setup();
    let err = t.client.try_cancel_issuer_transfer(&t.issuer, &NS, &t.token).unwrap_err();
    assert_eq!(err, Ok(RevoraError::NoTransferPending));
    assert_eq!(pending(&t), None);
}

#[test]
fn failed_accept_without_pending_leaves_state_unchanged() {
    let t = setup();
    let stranger = Address::generate(&t.env);
    let err = t.client.try_accept_issuer_transfer(&stranger, &NS, &t.token).unwrap_err();
    assert_eq!(err, Ok(RevoraError::NoTransferPending));
    assert_eq!(pending(&t), None);
}

#[test]
fn failed_double_proposal_keeps_original_value() {
    let t = setup();
    let first = Address::generate(&t.env);
    let second = Address::generate(&t.env);

    t.client.propose_issuer_transfer(&t.issuer, &NS, &t.token, &first);
    assert_eq!(pending(&t), Some(first.clone()));

    let err = t.client.try_propose_issuer_transfer(&t.issuer, &NS, &t.token, &second).unwrap_err();
    assert_eq!(err, Ok(RevoraError::IssuerTransferPending));

    // The rejected second proposal must not overwrite the observed value.
    assert_eq!(pending(&t), Some(first.clone()));
    let details: PendingTransfer =
        t.client.get_pending_transfer_details(&t.issuer, &NS, &t.token).unwrap();
    assert_eq!(details.new_issuer, first);
}

#[test]
fn failed_proposal_for_unknown_offering_leaves_no_state() {
    let t = setup();
    let stranger = Address::generate(&t.env);
    let other_token = Address::generate(&t.env);

    // Unknown (issuer, namespace, token): OfferingNotFound, and the reader
    // must still see nothing for both the fake id and the real one.
    let err = t
        .client
        .try_propose_issuer_transfer(&stranger, &NS, &other_token, &Address::generate(&t.env))
        .unwrap_err();
    assert_eq!(err, Ok(RevoraError::OfferingNotFound));
    assert_eq!(t.client.get_pending_issuer_transfer(&stranger, &NS, &other_token), None);
    assert_eq!(pending(&t), None);
}

// ─── Expiry boundary behavior ────────────────────────────────────────────────

#[test]
fn expired_pending_stays_readable_until_cleared() {
    let t = setup();
    let new_issuer = Address::generate(&t.env);
    let t0 = t.env.ledger().timestamp();

    // Custom expiry: proposal with expiry_secs = 3600 (1h).
    t.client.propose_transfer_with_expiry(&t.issuer, &NS, &t.token, &new_issuer, &3_600u64);

    let details: PendingTransfer =
        t.client.get_pending_transfer_details(&t.issuer, &NS, &t.token).unwrap();
    assert_eq!(details.expiry_secs, 3_600);
    assert_eq!(pending(&t), Some(new_issuer.clone()));

    // Advance past the effective deadline — the reader is time-transparent
    // and keeps returning the record (expiry is enforced by accept, not read).
    t.env.ledger().with_mut(|l| l.timestamp = t0 + 3_601);
    assert_eq!(pending(&t), Some(new_issuer.clone()));

    // Accept is what enforces the deadline.
    let err = t.client.try_accept_issuer_transfer(&new_issuer, &NS, &t.token).unwrap_err();
    assert_eq!(err, Ok(RevoraError::IssuerTransferExpired));

    // The failed accept must not have cleared the pending record.
    assert_eq!(pending(&t), Some(new_issuer));

    // Cancel (issuer-side) clears it deterministically.
    t.client.cancel_issuer_transfer(&t.issuer, &NS, &t.token);
    assert_eq!(pending(&t), None);
}

#[test]
fn expired_default_window_read_stable_and_accept_rejected() {
    let t = setup();
    let new_issuer = Address::generate(&t.env);
    let t0 = t.env.ledger().timestamp();

    t.client.propose_issuer_transfer(&t.issuer, &NS, &t.token, &new_issuer);

    // Jump well past the 7-day default window.
    t.env.ledger().with_mut(|l| l.timestamp = t0 + 8 * 24 * 60 * 60);
    assert_eq!(pending(&t), Some(new_issuer.clone()));

    let err = t.client.try_accept_issuer_transfer(&new_issuer, &NS, &t.token).unwrap_err();
    assert_eq!(err, Ok(RevoraError::IssuerTransferExpired));
    assert_eq!(pending(&t), Some(new_issuer));
}
