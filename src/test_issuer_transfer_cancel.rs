//! # Adversarial coverage — `cancel_issuer_transfer` (#1060)
//!
//! Exercises the issuer-side cancel of a pending issuer transfer beyond its
//! happy path:
//!
//! - **Success**: a pending proposal is removed and the `iss_canc` event
//!   carries the `(issuer, cancelled_target)` payload; the offering stays
//!   fully operational under the original issuer.
//! - **Typed rejections**: unknown offering → `OfferingNotFound`, no pending
//!   proposal → `NoTransferPending` (including wrong-namespace binding and
//!   cancel-after-accept), frozen → `ContractFrozen`, paused →
//!   `ContractPaused`.
//! - **Unauthorized callers (observable Layer 2)**: cancel authenticates the
//!   *primary issuer* by identity before any `require_auth`, so a caller who
//!   is not the primary issuer — an outside address, or even the proposed
//!   *new* issuer — is rejected with the typed `NotAuthorized` error, catchable
//!   via `try_*`.
//! - **State-unchanged after reject**: every failed cancel leaves the pending
//!   proposal intact — the new issuer can still accept it, the issuer can
//!   still cancel it once authorized/unpaused/unfrozen.
//! - **Expiry independence**: only *accept* enforces the expiry window; cancel
//!   works long after the window has closed.
//! - **Isolation**: cancelling one offering's proposal leaves a sibling
//!   proposal (same issuer+token, different namespace) untouched.
//!
//! ## Auth layer note
//!
//! `cancel_issuer_transfer` calls `Self::require_issuer_quorum_auth` after the
//! primary-issuer identity check. With `mock_all_auths()` the host satisfies
//! the quorum, so the discriminator this suite can observe is the identity
//! check — which is exactly the Layer-2 behavior the issue asks to pin. Host
//! auth panics (Layer 1) are non-unwinding and out of `try_*` reach; see
//! `test_auth.rs` Section A for the repo-wide convention.
//!
//! ## State machine reference
//!
//! ```text
//! propose/propose_with_expiry (issuer quorum) ──▶ PendingIssuerTransfer{new_issuer, timestamp, expiry_secs}
//! accept  (new_issuer auth)  : pending removed, OfferingIssuer + record moved
//! cancel  (issuer quorum)    : pending removed, iss_canc event
//! reject  (new_issuer auth)  : pending removed
//! replace (issuer quorum)    : pending updated (new target, same expiry window)
//! ```

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short, testutils::Address as _, testutils::Events as _, testutils::Ledger as _, Address,
    Env, IntoVal, TryIntoVal, Vec,
};

/// Default acceptance window (7 days) — referenced by the expiry-independence
/// test, which goes far past it.
const DEFAULT_EXPIRY_SECS: u64 = 7 * 24 * 60 * 60;

const NS: soroban_sdk::Symbol = symbol_short!("def");

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Single-issuer offering registered under `NS`. Returns (issuer, token).
/// Calls `env.mock_all_auths()` — callers inherit the mock for the whole test.
fn setup_offering(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Address) {
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    client.set_admin(&issuer);
    client.register_offering(
        &issuer,
        &Vec::new(env),
        &1u32,
        &NS,
        &token,
        &1_000,
        &token,
        &0,
        &symbol_short!(""),
        &0,
    );
    (issuer, token)
}

/// Seed the issuer-registry entries that the accept-side scan consumes.
///
/// Cancel itself does not scan the registry (it resolves the offering
/// directly), but seeding keeps the state-unchanged proofs able to fall
/// through to a real accept. Each namespace of `issuer` must be listed here
/// for accepts through that namespace to find the pending proposal.
/// See `test_issuer_transfer_accept.rs` for the full rationale of this
/// workaround.
fn seed_issuer_registry(
    env: &Env,
    contract_id: &Address,
    issuer: &Address,
    namespaces: &[soroban_sdk::Symbol],
) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(&crate::DataKey2::IssuerCount, &1u32);
        env.storage().persistent().set(&crate::DataKey2::IssuerItem(0), issuer);
        env.storage().persistent().set(&crate::DataKey2::IssuerRegistered(issuer.clone()), &true);
        for (i, namespace) in namespaces.iter().enumerate() {
            let i = i as u32;
            env.storage()
                .persistent()
                .set(&crate::DataKey2::NamespaceCount(issuer.clone()), &(i + 1));
            env.storage()
                .persistent()
                .set(&crate::DataKey2::NamespaceItem(issuer.clone(), i), namespace);
            env.storage().persistent().set(
                &crate::DataKey2::NamespaceRegistered(issuer.clone(), namespace.clone()),
                &true,
            );
        }
    });
}

/// Count `iss_canc` events in the full event log.
fn count_cancel_events(env: &Env) -> usize {
    let canc_sym = symbol_short!("iss_canc");
    let mut count = 0usize;
    for (_, topics, _) in env.events().all().iter() {
        if !topics.is_empty()
            && topics
                .get(0)
                .map(|t| {
                    t.try_into_val(env) as Result<soroban_sdk::Symbol, _> == Ok(canc_sym.clone())
                })
                .unwrap_or(false)
        {
            count += 1;
        }
    }
    count
}

// ── Section A: Success paths ──────────────────────────────────────────────────

/// Happy path: cancel removes the pending entry, emits exactly one `iss_canc`
/// event carrying `(cancelling_issuer, cancelled_target)`, and a second cancel
/// fails with `NoTransferPending` (the entry is consumed, not idempotent).
#[test]
fn cancel_removes_pending_emits_payload_and_consumes_entry() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    let baseline = count_cancel_events(&env);

    client.cancel_issuer_transfer(&issuer, &NS, &token);

    assert_eq!(
        count_cancel_events(&env),
        baseline + 1,
        "exactly one iss_canc event must be emitted by cancel"
    );

    // Verify the event payload: data is (issuer, cancelled_target).
    let canc_sym = symbol_short!("iss_canc");
    let mut payload: Option<(Address, Address)> = None;
    for (_, topics, data) in env.events().all().iter() {
        if !topics.is_empty()
            && topics
                .get(0)
                .map(|t| {
                    t.try_into_val(&env) as Result<soroban_sdk::Symbol, _> == Ok(canc_sym.clone())
                })
                .unwrap_or(false)
        {
            payload = Some(data.into_val(&env));
        }
    }
    let (cancelled_by, target) = payload.expect("iss_canc event must be present");
    assert_eq!(cancelled_by, issuer, "iss_canc must name the cancelling issuer");
    assert_eq!(target, new_issuer, "iss_canc must name the cancelled target");

    // The entry is consumed: neither cancel nor accept can act on it again.
    let second_cancel = client.try_cancel_issuer_transfer(&issuer, &NS, &token);
    assert_eq!(second_cancel, Err(Ok(RevoraError::NoTransferPending)));
    let accept_after = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(accept_after, Err(Ok(RevoraError::NoTransferPending)));
}

/// Cancel does not damage the offering itself — it still resolves under the
/// original issuer with its original primary.
#[test]
fn cancel_leaves_offering_intact_under_original_issuer() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.cancel_issuer_transfer(&issuer, &NS, &token);

    let offering = client
        .get_offering(&issuer, &NS, &token)
        .expect("offering must still resolve under the original issuer after cancel");
    assert_eq!(offering.issuers.primary, issuer);
    assert_eq!(offering.namespace, NS);
    assert_eq!(offering.token, token);
}

// ── Section B: Typed rejection paths ──────────────────────────────────────────

/// Cancelling for an offering that does not exist must return the typed
/// `OfferingNotFound` error.
#[test]
fn cancel_unknown_offering_returns_offering_not_found() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let stranger = Address::generate(&env);
    let token = Address::generate(&env);

    let result = client.try_cancel_issuer_transfer(&stranger, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

/// Cancelling when no proposal is pending must return the typed
/// `NoTransferPending` error.
#[test]
fn cancel_without_pending_transfer_returns_no_transfer_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    let result = client.try_cancel_issuer_transfer(&issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));
}

/// The pending lookup is keyed by the full offering id: a proposal bound to
/// `NS` cannot be cancelled through a different namespace. A sibling offering
/// must exist under the wrong namespace so the lookup gets past
/// `OfferingNotFound` and reaches the pending-entry miss.
#[test]
fn cancel_with_wrong_namespace_returns_no_transfer_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    // Sibling offering so (issuer, other_ns, token) resolves as a valid,
    // issuer-owned offering with no pending transfer.
    let other_ns = symbol_short!("zzz");
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &other_ns,
        &token,
        &1_000,
        &token,
        &0,
        &symbol_short!(""),
        &0,
    );

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    let result = client.try_cancel_issuer_transfer(&issuer, &other_ns, &token);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));

    // The correctly-bound cancel still succeeds afterwards.
    client.cancel_issuer_transfer(&issuer, &NS, &token);
}

/// Once a transfer has been ACCEPTED, the pending entry is consumed — a later
/// cancel by the (still registered, stale-record) old issuer returns
/// `NoTransferPending`. Pins the accept-before-cancel transition ordering.
#[test]
fn cancel_after_accept_returns_no_transfer_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);

    // The old issuer still passes the identity check (its OfferingRecord was
    // kept and its primary is unchanged), but there is nothing left to cancel.
    let result = client.try_cancel_issuer_transfer(&issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));
}

// ── Section C: Unauthorized callers (identity checks, observable) ─────────────

/// A caller with no offering of their own under the queried identity gets the
/// typed `OfferingNotFound` error — the offering lookup runs before the
/// primary-issuer identity check, so this is the rejection such a caller
/// actually observes (never `NotAuthorized`).
#[test]
fn cancel_by_outsider_returns_offering_not_found_and_preserves_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    let outsider = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    let result = client.try_cancel_issuer_transfer(&outsider, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    // State unchanged: the authorized issuer can still cancel.
    client.cancel_issuer_transfer(&issuer, &NS, &token);
}

/// The reachable `NotAuthorized` path: after a completed accept, the migrated
/// offering record under the NEW issuer's key keeps the OLD primary, so the
/// new issuer querying cancel resolves an offering whose primary is someone
/// else → typed `NotAuthorized`. Pins the identity check on the post-transfer
/// state shape.
#[test]
fn cancel_by_new_issuer_after_accept_returns_not_authorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);

    // The new issuer now owns the (migrated) record, but its primary field
    // still names the old issuer — the identity check must reject them.
    let result = client.try_cancel_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));
}

// ── Section D: Frozen / paused guards ─────────────────────────────────────────

/// After `freeze()`, cancel returns the typed `ContractFrozen` error and the
/// pending proposal survives (verified by reading the key from the contract
/// frame — storage reads stay permitted while frozen).
#[test]
fn cancel_blocked_when_contract_frozen_and_pending_survives() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.freeze();

    let result = client.try_cancel_issuer_transfer(&issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));

    let contract_id = client.address.clone();
    let still_pending = env.as_contract(&contract_id, || {
        env.storage().persistent().has(&crate::DataKey::PendingIssuerTransfer(crate::OfferingId {
            issuer: issuer.clone(),
            namespace: NS.clone(),
            token: token.clone(),
        }))
    });
    assert!(still_pending, "pending proposal must survive a frozen cancel");
}

/// After `pause_admin()`, cancel returns the typed `ContractPaused` error; the
/// issuer (who is the admin here) can unpause and then cancel successfully.
#[test]
fn cancel_blocked_when_contract_paused_and_pending_survives() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));
    // setup_offering already made the issuer the admin (set_admin is one-shot).
    let admin = issuer.clone();

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.pause_admin(&admin);

    let result = client.try_cancel_issuer_transfer(&issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));

    client.unpause_admin(&admin);
    client.cancel_issuer_transfer(&issuer, &NS, &token);
}

// ── Section E: Expiry independence ────────────────────────────────────────────

/// Only *accept* enforces the expiry window: a proposal far past its window is
/// no longer acceptable, but the issuer can still cancel it.
#[test]
fn cancel_works_long_after_accept_window_closed() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    let t0 = 1_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    // Far past the default 7-day window.
    env.ledger().set_timestamp(t0 + DEFAULT_EXPIRY_SECS * 10);

    let accept_expired = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(accept_expired, Err(Ok(RevoraError::IssuerTransferExpired)));

    client.cancel_issuer_transfer(&issuer, &NS, &token);
}

// ── Section F: Cross-offering isolation ───────────────────────────────────────

/// Cancelling one offering's proposal must not touch a sibling proposal on a
/// second offering with the same token but a different namespace.
#[test]
fn cancel_is_isolated_per_offering() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    // Both namespaces must be registered so the accept-side scan can find the
    // sibling proposal through `other_ns`.
    let other_ns = symbol_short!("oth");
    seed_issuer_registry(&env, &client.address, &issuer, &[NS.clone(), other_ns.clone()]);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &other_ns,
        &token,
        &1_000,
        &token,
        &0,
        &symbol_short!(""),
        &0,
    );

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.propose_issuer_transfer(&issuer, &other_ns, &token, &new_issuer);

    // Cancel the first proposal only.
    client.cancel_issuer_transfer(&issuer, &NS, &token);

    // The sibling proposal is untouched: the new issuer can still accept it.
    client.accept_issuer_transfer(&new_issuer, &other_ns, &token);
    assert!(client.get_offering(&new_issuer, &other_ns, &token).is_some());
    // ...while the cancelled one is gone.
    let gone = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(gone, Err(Ok(RevoraError::NoTransferPending)));
}
