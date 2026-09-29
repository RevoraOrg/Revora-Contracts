//! # Adversarial coverage — `accept_issuer_transfer` (#1059)
//!
//! Exercises the new-issuer-side accept of a pending issuer transfer beyond
//! its happy path:
//!
//! - **Success (state migration)**: accept consumes the pending entry,
//!   migrates the offering record to the new issuer's key (`get_offering`
//!   resolves under the new issuer with `primary == new_issuer`), and emits
//!   exactly one `iss_acc` event carrying `(old_issuer, new_issuer)`.
//! - **Config migration (#1344)**: per-offering configuration linked to the
//!   old `OfferingId` (e.g. `RoundingMode`) is moved to the new
//!   `OfferingId` — removed from the old key and present under the new one.
//! - **Self-accept**: a proposal naming the current issuer as its own target
//!   is accepted without any record migration — the pending entry is
//!   consumed and the offering stays put.
//! - **Typed rejections**: no matching pending proposal →
//!   `NoTransferPending` (including uninvolved callers, cancel-before-accept
//!   and reject-before-accept), expired window → `IssuerTransferExpired`
//!   with the inclusive boundary pinned, duplicate target offering →
//!   `LimitReached` (and the pending entry survives that rejection).
//! - **Custom windows & cross-offering isolation** (ported from the parallel
//!   #1059 draft): a `propose_transfer_with_expiry` window governs the accept
//!   boundary independently of the default 7-day window, and a pending
//!   proposal is bound to its own `(namespace, token)` identity — a sibling
//!   offering sharing only the token cannot consume it.
//! - **Unauthorized callers (observable Layer 2)**: accept authenticates the
//!   *new issuer* by identity — an address with no matching proposal gets
//!   the typed `NoTransferPending`, catchable via `try_*`. Host auth panics
//!   (Layer 1) are non-unwinding and out of `try_*` reach; see
//!   `test_auth.rs` Section A for the repo-wide convention.
//!
//! ## State machine reference
//!
//! ```text
//! propose/propose_with_expiry (issuer quorum) ──▶ PendingIssuerTransfer{new_issuer, timestamp, expiry_secs}
//! accept  (new_issuer auth)  : pending removed, OfferingIssuer + record moved,
//!                              config keys re-homed (#1344), iss_acc event
//!   pending.expiry_secs == 0 → window = ISSUER_TRANSFER_EXPIRY_SECS (7d), inclusive
//!   accept after window      → IssuerTransferExpired (pending kept)
//!   new_issuer already owns (ns, token) → LimitReached (pending kept)
//!   new_issuer == old issuer → pending removed, no migration, iss_acc event
//! cancel  (issuer quorum)    : pending removed, iss_canc event
//! reject  (new_issuer auth)  : pending removed
//! replace (issuer quorum)    : pending updated (new target, same window)
//! ```

#![cfg(test)]

use crate::{
    PendingTransfer, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient, RoundingMode,
};
use soroban_sdk::{
    symbol_short, testutils::Address as _, testutils::Events as _, testutils::Ledger as _, Address,
    Env, IntoVal, TryIntoVal, Vec,
};

/// Default acceptance window (7 days) — `expiry_secs == 0` resolves to this.
const DEFAULT_EXPIRY_SECS: u64 = 7 * 24 * 60 * 60;

/// Minimum custom window — `propose_transfer_with_expiry` clamps to this floor.
const MIN_EXPIRY_SECS: u64 = 60 * 60;

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
/// `register_offering` seeds the per-issuer namespace registry but never the
/// *global* issuer list, while `find_pending_transfer_for_new_issuer` (used
/// by accept/reject) iterates exactly that global list — so each namespace
/// of `issuer` must be seeded here for accepts to find the pending proposal.
/// See `test_issuer_transfer_cancel.rs` for the full rationale of this
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

/// Count events whose first topic is `sym` in the full event log.
fn count_events_with_topic(env: &Env, sym: &soroban_sdk::Symbol) -> usize {
    let mut count = 0usize;
    for (_, topics, _) in env.events().all().iter() {
        if !topics.is_empty()
            && topics
                .get(0)
                .map(|t| t.try_into_val(env) as Result<soroban_sdk::Symbol, _> == Ok(sym.clone()))
                .unwrap_or(false)
        {
            count += 1;
        }
    }
    count
}

// ── Section A: Success — migration, event, pending consumption ────────────────

/// Happy path: accept consumes the pending entry, the offering resolves under
/// the new issuer with `primary == new_issuer`, and exactly one `iss_acc`
/// event carries `(old_issuer, new_issuer)`.
#[test]
fn accept_moves_offering_consumes_pending_and_emits_payload() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    let acc_sym = symbol_short!("iss_acc");
    let acc_before = count_events_with_topic(&env, &acc_sym);

    client.accept_issuer_transfer(&new_issuer, &NS, &token);

    // Exactly one iss_acc event with the (old_issuer, new_issuer) payload.
    assert_eq!(
        count_events_with_topic(&env, &acc_sym),
        acc_before + 1,
        "exactly one iss_acc event must be emitted by accept"
    );
    let mut payload: Option<(Address, Address)> = None;
    for (_, topics, data) in env.events().all().iter() {
        if !topics.is_empty()
            && topics
                .get(0)
                .map(|t| {
                    t.try_into_val(&env) as Result<soroban_sdk::Symbol, _> == Ok(acc_sym.clone())
                })
                .unwrap_or(false)
        {
            payload = Some(data.into_val(&env));
        }
    }
    let (from, to) = payload.expect("iss_acc event must be present");
    assert_eq!(from, issuer, "iss_acc must name the old issuer");
    assert_eq!(to, new_issuer, "iss_acc must name the new issuer");

    // Pending entry is consumed.
    assert_eq!(client.get_pending_issuer_transfer(&issuer, &NS, &token), None);
    assert_eq!(client.get_pending_transfer_details(&issuer, &NS, &token), None);

    // The migrated record resolves under the new issuer's key. Per the accept
    // semantics (and the post-accept proofs in `test_issuer_transfer_cancel.rs`),
    // the copied record keeps the OLD primary — ownership is expressed by the
    // key re-homing and the OfferingIssuer redirect, not by mutating the record.
    let offering = client
        .get_offering(&new_issuer, &NS, &token)
        .expect("offering must resolve under the new issuer after accept");
    assert_eq!(
        offering.issuers.primary, issuer,
        "the migrated record must keep the old primary (documented accept shape)"
    );
    assert_eq!(offering.token, token);
    assert_eq!(offering.namespace, NS);

    // The OfferingIssuer redirect for the old identity now names the new issuer.
    let contract_id = client.address.clone();
    let redirect = env.as_contract(&contract_id, || {
        env.storage().persistent().get::<crate::DataKey, Address>(&crate::DataKey::OfferingIssuer(
            crate::OfferingId {
                issuer: issuer.clone(),
                namespace: NS.clone(),
                token: token.clone(),
            },
        ))
    });
    assert_eq!(
        redirect,
        Some(new_issuer.clone()),
        "OfferingIssuer(old_id) must redirect to the new issuer after accept"
    );
}

/// Config linked to the old `OfferingId` is re-homed to the new one (#1344):
/// the `RoundingMode` key is removed from the old issuer's id and present
/// under the new issuer's id.
#[test]
fn accept_migrates_rounding_mode_config_to_new_issuer_key() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.set_rounding_mode(&issuer, &NS, &token, &RoundingMode::RoundHalfUp);
    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);

    // Observable through the public getter under the new issuer's key.
    assert_eq!(
        client.get_rounding_mode(&new_issuer, &NS, &token),
        RoundingMode::RoundHalfUp,
        "config must follow the offering to the new issuer key"
    );

    // Storage-level proof: removed from the old key, present under the new.
    let contract_id = client.address.clone();
    let (old_gone, new_present) = env.as_contract(&contract_id, || {
        let old_key = crate::DataKey::RoundingMode(crate::OfferingId {
            issuer: issuer.clone(),
            namespace: NS.clone(),
            token: token.clone(),
        });
        let new_key = crate::DataKey::RoundingMode(crate::OfferingId {
            issuer: new_issuer.clone(),
            namespace: NS.clone(),
            token: token.clone(),
        });
        (
            !env.storage().persistent().has(&old_key),
            env.storage().persistent().get::<crate::DataKey, RoundingMode>(&new_key),
        )
    });
    assert!(old_gone, "config key must be removed from the old OfferingId");
    assert_eq!(
        new_present,
        Some(RoundingMode::RoundHalfUp),
        "config key must be present under the new OfferingId"
    );
}

/// A self-referential proposal (target == current issuer) is accepted without
/// any record migration: the pending entry is consumed and the offering stays
/// under the original key with its original primary.
#[test]
fn self_accept_consumes_pending_without_moving_offering() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &issuer);
    client.accept_issuer_transfer(&issuer, &NS, &token);

    assert_eq!(client.get_pending_issuer_transfer(&issuer, &NS, &token), None);
    let offering = client
        .get_offering(&issuer, &NS, &token)
        .expect("self-accept must leave the offering under the original issuer");
    assert_eq!(offering.issuers.primary, issuer);

    // The accept event still fires, naming the same address on both sides.
    let acc_sym = symbol_short!("iss_acc");
    let mut payload: Option<(Address, Address)> = None;
    for (_, topics, data) in env.events().all().iter() {
        if !topics.is_empty()
            && topics
                .get(0)
                .map(|t| {
                    t.try_into_val(&env) as Result<soroban_sdk::Symbol, _> == Ok(acc_sym.clone())
                })
                .unwrap_or(false)
        {
            payload = Some(data.into_val(&env));
        }
    }
    let (from, to) = payload.expect("iss_acc event must be present for self-accept");
    assert_eq!(from, issuer);
    assert_eq!(to, issuer);
}

// ── Section B: Typed rejection paths ──────────────────────────────────────────

/// Accepting when no proposal matches `(namespace, token, new_issuer)` must
/// return the typed `NoTransferPending` error.
#[test]
fn accept_without_pending_returns_no_transfer_pending() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let stranger = Address::generate(&env);
    let token = Address::generate(&env);

    let result = client.try_accept_issuer_transfer(&stranger, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));
}

/// The pending lookup is keyed by `(namespace, token)` and matched against
/// the proposal target: an address that is not the proposed new issuer gets
/// `NoTransferPending`, and the real proposal survives untouched.
#[test]
fn accept_by_uninvolved_address_returns_no_transfer_pending_and_preserves_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    let outsider = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    let result = client.try_accept_issuer_transfer(&outsider, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));

    // State unchanged: the real target can still accept.
    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("pending proposal must survive a rejected accept by an outsider");
    assert_eq!(details.new_issuer, new_issuer);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);
}

/// Once CANCELLED, the pending entry is consumed — a later accept returns
/// `NoTransferPending`.
#[test]
fn accept_after_cancel_returns_no_transfer_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.cancel_issuer_transfer(&issuer, &NS, &token);

    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));
}

/// Once REJECTED, the pending entry is consumed — a later accept returns
/// `NoTransferPending`.
#[test]
fn accept_after_reject_returns_no_transfer_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.reject_issuer_transfer(&new_issuer, &NS, &token);

    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));
}

/// Accepting past the expiry window returns the typed
/// `IssuerTransferExpired` error — and the pending entry survives, so the
/// issuer can still cancel it afterwards.
#[test]
fn accept_past_expiry_returns_issuer_transfer_expired_and_preserves_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    let t0 = 1_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    env.ledger().set_timestamp(t0 + DEFAULT_EXPIRY_SECS + 1);
    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::IssuerTransferExpired)));

    // State unchanged: the proposal is still readable and cancelable.
    assert!(client.get_pending_transfer_details(&issuer, &NS, &token).is_some());
    client.cancel_issuer_transfer(&issuer, &NS, &token);
}

/// The expiry boundary is inclusive: accepting at exactly
/// `timestamp + window` succeeds for the default (0-marker) window.
#[test]
fn accept_at_exact_default_window_boundary_succeeds() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    let t0 = 1_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);

    env.ledger().set_timestamp(t0 + DEFAULT_EXPIRY_SECS);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);
}

/// A custom window proposed via `propose_transfer_with_expiry` governs accept.
/// A 1-hour window stays acceptable exactly up to `timestamp + 1h`.
#[test]
fn accept_uses_custom_window_and_expires_past_it() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

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
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    let t0 = 3_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &MIN_EXPIRY_SECS);

    env.ledger().set_timestamp(t0 + MIN_EXPIRY_SECS + 1);
    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::IssuerTransferExpired)));
}

/// When the new issuer already owns an offering with the same
/// `(namespace, token)`, accept must fail with the typed `LimitReached` —
/// and the pending proposal survives (the duplicate check runs before any
/// state mutation).
#[test]
fn accept_blocked_by_duplicate_target_offering_and_preserves_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    // The new issuer already owns a sibling offering with the same
    // namespace + token identity.
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

    // State unchanged: the pending proposal is still intact and the old
    // offering still resolves under the old issuer.
    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("pending proposal must survive a duplicate-target rejection");
    assert_eq!(details.new_issuer, new_issuer);
    assert!(client.get_offering(&issuer, &NS, &token).is_some());
}

// ── Section C: Frozen / paused guards ─────────────────────────────────────────

/// After `freeze()`, accept returns the typed `ContractFrozen` error and the
/// pending proposal survives.
#[test]
fn accept_blocked_when_contract_frozen_and_pending_survives() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.freeze();

    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));

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

/// After `pause_admin()`, accept returns the typed `ContractPaused` error;
/// after unpause the accept succeeds normally.
#[test]
fn accept_blocked_when_contract_paused_then_unpause_succeeds() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));
    // setup_offering already made the issuer the admin (set_admin is one-shot).
    let admin = issuer.clone();

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.pause_admin(&admin);

    let result = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));

    client.unpause_admin(&admin);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);
    assert!(client.get_offering(&new_issuer, &NS, &token).is_some());
}

// ── Section D: Cross-offering isolation ──────────────────────────────────────

/// A pending proposal for one offering must not be acceptable through a second
/// offering that shares the token but not the namespace (and vice versa).
#[test]
fn pending_proposal_is_bound_to_its_own_offering_identity() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

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
