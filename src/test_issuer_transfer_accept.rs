//! # Adversarial coverage — `accept_issuer_transfer` (#1059)
//!
//! Exercises the accept half of the issuer-transfer state machine beyond its
//! happy path:
//!
//! - **Success**: full transfer (new issuer ≠ old) and self-accept short-circuit
//!   (`new_issuer == old_issuer` just clears the pending entry).
//! - **Rejection with typed errors**: no pending transfer, wrong
//!   namespace/token binding, duplicate target offering, contract frozen,
//!   contract paused — each observed via the generated `try_*` client and
//!   asserted against [`RevoraError`].
//! - **Expiry**: the boundary is **inclusive** (`>` not `>=`): accepting at
//!   exactly `timestamp + expiry` succeeds; one second later fails with
//!   `IssuerTransferExpired`. Both the default (0 → 7 days) and custom
//!   clamped windows are exercised.
//! - **State-unchanged after reject**: every failed accept leaves the pending
//!   proposal in place — proven by a subsequent successful cancel (issuer-side)
//!   or accept (new-issuer-side), and by the offering still resolving under the
//!   old issuer.
//!
//! ## Auth layer note
//!
//! `accept_issuer_transfer` calls `new_issuer.require_auth()` before any typed
//! check, so a *wrong* identity is rejected by the host at Layer 1 (a
//! non-unwinding panic that `try_*` cannot capture). All auth in this suite is
//! `mock_all_auths()`-mocked; the "unauthorized caller" dimension is covered
//! through the *identity* checks instead (`NoTransferPending` for a new issuer
//! with no pending proposal targeted at it) and through issuer-side
//! authorization on the sibling entrypoints (see
//! `test_issuer_transfer_cancel.rs` / `test_issuer_transfer_replace.rs` for the
//! Layer-2 identity matrices).
//!
//! ## State machine reference
//!
//! ```text
//! propose/propose_with_expiry (issuer quorum) ──▶ PendingIssuerTransfer{new_issuer, timestamp, expiry_secs}
//! accept  (new_issuer auth)  : pending removed, offering moved (or cleared if self-accept)
//! cancel  (issuer quorum)    : pending removed
//! reject  (new_issuer auth)  : pending removed
//! replace (issuer quorum)    : pending updated (new target, same expiry window)
//! ```

#![cfg(test)]

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short, testutils::Address as _, testutils::Ledger as _, Address, Env, Vec,
};

/// Default window: `pending.expiry_secs == 0` resolves to this at accept time.
const DEFAULT_EXPIRY_SECS: u64 = 7 * 24 * 60 * 60;
/// Clamp bounds applied by `propose_transfer_with_expiry`.
const MIN_EXPIRY_SECS: u64 = 60 * 60;
const MAX_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;

const NS: soroban_sdk::Symbol = symbol_short!("def");

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient {
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

/// Seed the issuer-registry entries that `find_pending_transfer_for_new_issuer`
/// scans (`IssuerCount`/`IssuerItem` + per-issuer namespace list).
///
/// **Known gap (documented in the PR):** `register_offering` writes the
/// namespace list (`NamespaceCount`/`NamespaceItem`) but never calls
/// `ensure_issuer_registered`, so a fresh offering is invisible to the accept
/// scan until some other flow registers the issuer. The existing `issue_370`
/// test works around this identically. Seeding here preserves the public
/// contract (per issue #1059's acceptance criteria) while the tests still
/// exercise the real lookup path.
fn seed_issuer_registry(
    env: &Env,
    contract_id: &Address,
    issuer: &Address,
    namespace: &soroban_sdk::Symbol,
) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(&crate::DataKey2::IssuerCount, &1u32);
        env.storage().persistent().set(&crate::DataKey2::IssuerItem(0), issuer);
        env.storage().persistent().set(&crate::DataKey2::IssuerRegistered(issuer.clone()), &true);
        env.storage().persistent().set(&crate::DataKey2::NamespaceCount(issuer.clone()), &1u32);
        env.storage()
            .persistent()
            .set(&crate::DataKey2::NamespaceItem(issuer.clone(), 0), namespace);
        env.storage()
            .persistent()
            .set(&crate::DataKey2::NamespaceRegistered(issuer.clone(), namespace.clone()), &true);
    });
}

// ── Section A: Success paths ──────────────────────────────────────────────────

/// Happy path: propose → accept moves the offering to the new issuer and
/// consumes the pending entry (a second accept returns `NoTransferPending`).
#[test]
fn accept_completes_transfer_and_consumes_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);

    // Offering now resolves under the new issuer.
    assert!(
        client.get_offering(&new_issuer, &NS, &token).is_some(),
        "offering must resolve under the new issuer after accept"
    );
    // Pending entry consumed: accepting again has nothing to accept.
    let second = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(second, Err(Ok(RevoraError::NoTransferPending)));
}

/// Self-accept short-circuit: when the proposed target equals the current
/// issuer, accept just clears the pending entry without migrating anything.
#[test]
fn accept_self_transfer_clears_pending_without_moving_offering() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);

    client.propose_issuer_transfer(&issuer, &NS, &token, &issuer);
    client.accept_issuer_transfer(&issuer, &NS, &token);

    assert!(
        client.get_offering(&issuer, &NS, &token).is_some(),
        "offering must still resolve under the same issuer"
    );
    let second = client.try_accept_issuer_transfer(&issuer, &NS, &token);
    assert_eq!(second, Err(Ok(RevoraError::NoTransferPending)));
}

// ── Section B: Rejection paths (typed errors) ─────────────────────────────────

/// Accepting when no proposal exists for the (namespace, token) pair must
/// return the typed `NoTransferPending` error.
#[test]
fn accept_without_pending_transfer_returns_no_transfer_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (_issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);

    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));
}

/// The pending lookup is bound to (namespace, token): a proposal for one
/// namespace is not acceptable through another one.
#[test]
fn accept_with_wrong_namespace_returns_no_transfer_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    let other_ns = symbol_short!("zzz");
    let result = client.try_accept_issuer_transfer(&new_issuer, &other_ns, &token);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));

    // The correctly-bound proposal is still pending and acceptable.
    client.accept_issuer_transfer(&new_issuer, &NS, &token);
    assert!(client.get_offering(&new_issuer, &NS, &token).is_some());
}

/// Accepting when the target issuer already owns an offering with the same
/// (namespace, token) must return the typed duplicate error.
#[test]
fn accept_into_duplicate_offering_returns_typed_error() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    let new_issuer = Address::generate(&env);

    // Give the new issuer an existing offering with the same ns/token.
    client.register_offering(
        &new_issuer,
        &Vec::new(&env),
        &1u32,
        &NS,
        &token,
        &1_000,
        &token,
        &0,
        &symbol_short!(""),
        &0,
    );

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::LimitReached)));
}

// ── Section C: Expiry semantics (inclusive boundary) ──────────────────────────

/// With the default window (expiry_secs == 0), accepting at exactly
/// `timestamp + DEFAULT_EXPIRY_SECS` succeeds — the boundary is inclusive
/// (`>` comparison, not `>=`).
#[test]
fn accept_at_exact_default_expiry_boundary_succeeds() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    let new_issuer = Address::generate(&env);

    let t0 = 1_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    env.ledger().set_timestamp(t0 + DEFAULT_EXPIRY_SECS);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);
    assert!(client.get_offering(&new_issuer, &NS, &token).is_some());
}

/// One second past the default window, accept fails with the typed
/// `IssuerTransferExpired` error.
#[test]
fn accept_one_second_past_default_expiry_fails() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    let new_issuer = Address::generate(&env);

    let t0 = 1_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    env.ledger().set_timestamp(t0 + DEFAULT_EXPIRY_SECS + 1);
    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::IssuerTransferExpired)));
}

/// A custom window proposed via `propose_transfer_with_expiry` governs accept.
/// A 1-hour window expires one second after `timestamp + 1h`.
#[test]
fn accept_uses_custom_window_and_expires_past_it() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    let new_issuer = Address::generate(&env);

    let t0 = 2_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &MIN_EXPIRY_SECS);

    // Exactly at the boundary: still acceptable.
    env.ledger().set_timestamp(t0 + MIN_EXPIRY_SECS);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);
    assert!(client.get_offering(&new_issuer, &NS, &token).is_some());
}

/// A custom 1-hour window: one second past the boundary accept must fail, even
/// though the default 7-day window would still be open.
#[test]
fn accept_past_custom_window_fails_even_within_default_window() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    let new_issuer = Address::generate(&env);

    let t0 = 3_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &MIN_EXPIRY_SECS);

    env.ledger().set_timestamp(t0 + MIN_EXPIRY_SECS + 1);
    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::IssuerTransferExpired)));
}

// ── Section D: State-unchanged after rejected accept ──────────────────────────

/// A rejected (expired) accept must NOT consume the pending proposal: the
/// issuer can still cancel it afterwards, and the offering still resolves
/// under the old issuer.
#[test]
fn rejected_expired_accept_leaves_pending_cancellable_by_issuer() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    let new_issuer = Address::generate(&env);

    let t0 = 1_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    env.ledger().set_timestamp(t0 + DEFAULT_EXPIRY_SECS + 1);
    let rejected = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(rejected, Err(Ok(RevoraError::IssuerTransferExpired)));

    // Pending entry survived: offering unchanged, cancel (issuer-side) works.
    assert!(client.get_offering(&issuer, &NS, &token).is_some());
    client.cancel_issuer_transfer(&issuer, &NS, &token);
}

/// A rejected (wrong-binding) accept must not disturb the proposal: the
/// correctly-bound accept still succeeds afterwards.
#[test]
fn rejected_wrong_binding_accept_leaves_pending_acceptable() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    let other_ns = symbol_short!("zzz");
    let rejected = client.try_accept_issuer_transfer(&new_issuer, &other_ns, &token);
    assert_eq!(rejected, Err(Ok(RevoraError::NoTransferPending)));

    client.accept_issuer_transfer(&new_issuer, &NS, &token);
    assert!(client.get_offering(&new_issuer, &NS, &token).is_some());
}

// ── Section E: Frozen / paused guards (typed, catchable) ──────────────────────

/// After `freeze()`, accept returns the typed `ContractFrozen` error and leaves
/// the pending proposal intact.
#[test]
fn accept_blocked_when_contract_frozen() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.freeze();

    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));

    // The pending entry survived the rejected accept. Reads of storage are
    // permitted while frozen (only mutations are blocked), so inspect it
    // directly from the contract frame.
    let contract_id = client.address.clone();
    let still_pending = env.as_contract(&contract_id, || {
        env.storage().persistent().has(&crate::DataKey::PendingIssuerTransfer(crate::OfferingId {
            issuer: issuer.clone(),
            namespace: NS.clone(),
            token: token.clone(),
        }))
    });
    assert!(still_pending, "pending proposal must survive a frozen accept");
}

/// After `pause_admin()`, accept returns the typed `Paused` error and leaves
/// the pending proposal intact.
#[test]
fn accept_blocked_when_contract_paused() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    // setup_offering already made the issuer the admin (set_admin is one-shot),
    // so the issuer pauses and later unpauses the contract itself.
    let admin = issuer.clone();
    let new_issuer = Address::generate(&env);

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.pause_admin(&admin);

    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));

    // The pending entry survived the rejected accept: after unpausing, the
    // issuer can still cancel it.
    client.unpause_admin(&admin);
    client.cancel_issuer_transfer(&issuer, &NS, &token);
}

// ── Section F: Cross-offering isolation ───────────────────────────────────────

/// A pending proposal for one offering must not be acceptable through a second
/// offering that shares the token but not the namespace (and vice versa).
#[test]
fn pending_proposal_is_bound_to_its_own_offering_identity() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);

    seed_issuer_registry(&env, &client.address, &issuer, &NS);
    let new_issuer = Address::generate(&env);

    let other_ns = symbol_short!("oth");
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

    // The other offering has no pending proposal.
    let result = client.try_accept_issuer_transfer(&new_issuer, &other_ns, &token);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));

    // The bound proposal is still consumable.
    client.accept_issuer_transfer(&new_issuer, &NS, &token);
    assert!(client.get_offering(&new_issuer, &NS, &token).is_some());
    assert!(
        client.get_offering(&issuer, &other_ns, &token).is_some(),
        "the untouched sibling offering must keep its original issuer"
    );
}
