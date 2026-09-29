//! # Adversarial coverage — `replace_issuer_transfer` (#1058)
//!
//! Exercises the issuer-side replacement of a pending issuer transfer beyond
//! its happy path:
//!
//! - **Success (single state machine step)**: replace swaps the pending
//!   target to `new_issuer`, stamps the **new** ledger timestamp, and
//!   **preserves the original `expiry_secs`** (the replacement inherits the
//!   same window — including `0` = default marker). Observable through
//!   `get_pending_transfer_details`.
//! - **Event pair**: exactly one `iss_canc` naming
//!   `(issuer, old_target)` and exactly one `iss_prop` naming
//!   `(new_issuer, timestamp)` per replace — the two-step transition is
//!   emitted as one atomic pair.
//! - **Typed rejections**: unknown offering → `OfferingNotFound`, no pending
//!   proposal → `NoTransferPending` (including wrong-namespace binding and
//!   replace-after-accept), frozen → `ContractFrozen`, paused →
//!   `ContractPaused`.
//! - **Unauthorized callers (observable Layer 2)**: replace authenticates the
//!   *primary issuer* by identity before any `require_auth`, so the
//!   post-accept migrated-record shape yields the typed `NotAuthorized`,
//!   catchable via `try_*`. Host auth panics (Layer 1) are non-unwinding and
//!   out of `try_*` reach; see `test_auth.rs` Section A for the repo-wide
//!   convention.
//! - **State-unchanged after reject**: every failed replace leaves the
//!   pending proposal fully intact — target, timestamp, and stored window.
//! - **Window inheritance is behavioral**: a replace of a proposal made with
//!   `propose_issuer_transfer` (stored window `0`) still expires on the
//!   default 7-day schedule — the replace must not "reset" the window clock
//!   beyond re-stamping the timestamp.
//! - **Isolation**: replacing one offering's proposal leaves a sibling
//!   proposal (same issuer+token, different namespace) untouched.
//!
//! ## State machine reference
//!
//! ```text
//! propose/propose_with_expiry (issuer quorum) ──▶ PendingIssuerTransfer{new_issuer, timestamp, expiry_secs}
//! accept  (new_issuer auth)  : pending removed, OfferingIssuer + record moved
//! cancel  (issuer quorum)    : pending removed, iss_canc event
//! reject  (new_issuer auth)  : pending removed
//! replace (issuer quorum)    : pending updated (new target, NEW timestamp, SAME expiry_secs)
//!                                emits iss_canc(issuer, old_target) + iss_prop(new_issuer, ts)
//! ```

#![cfg(test)]

use crate::{PendingTransfer, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short, testutils::Address as _, testutils::Events as _, testutils::Ledger as _, Address,
    Env, IntoVal, TryIntoVal, Vec,
};

/// Default acceptance window (7 days).
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
/// Replace itself resolves the offering directly, but the state-unchanged
/// proofs fall through to a real accept, and the accept-side scan needs each
/// namespace of `issuer` listed here. See
/// `test_issuer_transfer_accept.rs` / `test_issuer_transfer_cancel.rs` for
/// the full rationale of this workaround.
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

// ── Section A: Success — target swap, timestamp stamp, window inheritance ─────

/// Replace swaps the pending target, stamps the new ledger timestamp, and
/// preserves the original `expiry_secs` verbatim.
#[test]
fn replace_swaps_target_restamps_timestamp_and_preserves_window() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target = Address::generate(&env);
    let second_target = Address::generate(&env);

    let t0 = 10_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &first_target, &3_600);

    env.ledger().set_timestamp(t0 + 500);
    client.replace_issuer_transfer(&issuer, &NS, &token, &second_target);

    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("pending transfer must still exist after replace");
    assert_eq!(details.new_issuer, second_target, "replace must swap the pending target");
    assert_eq!(details.timestamp, t0 + 500, "replace must stamp the replace-time ledger timestamp");
    assert_eq!(
        details.expiry_secs, 3_600,
        "replace must preserve the original expiry_secs verbatim"
    );
}

/// The `expiry_secs == 0` default marker survives a replace too.
#[test]
fn replace_preserves_zero_default_marker() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target = Address::generate(&env);
    let second_target = Address::generate(&env);

    client.propose_issuer_transfer(&issuer, &NS, &token, &first_target);
    client.replace_issuer_transfer(&issuer, &NS, &token, &second_target);

    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("pending transfer must still exist after replace");
    assert_eq!(details.new_issuer, second_target);
    assert_eq!(details.expiry_secs, 0, "the default marker must survive replace untouched");
}

/// Replace emits exactly one `iss_canc` naming `(issuer, old_target)` and
/// exactly one `iss_prop` naming `(new_issuer, timestamp)` — one atomic pair.
#[test]
fn replace_emits_atomic_canc_prop_event_pair() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target = Address::generate(&env);
    let second_target = Address::generate(&env);

    let t0 = 20_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_issuer_transfer(&issuer, &NS, &token, &first_target);

    let canc_sym = symbol_short!("iss_canc");
    let prop_sym = symbol_short!("iss_prop");
    let canc_before = count_events_with_topic(&env, &canc_sym);
    let prop_before = count_events_with_topic(&env, &prop_sym);

    env.ledger().set_timestamp(t0 + 42);
    client.replace_issuer_transfer(&issuer, &NS, &token, &second_target);

    assert_eq!(
        count_events_with_topic(&env, &canc_sym),
        canc_before + 1,
        "exactly one iss_canc event must be emitted by replace"
    );
    assert_eq!(
        count_events_with_topic(&env, &prop_sym),
        prop_before + 1,
        "exactly one iss_prop event must be emitted by replace"
    );

    let mut canc_payload: Option<(Address, Address)> = None;
    let mut prop_payload: Option<(Address, u64)> = None;
    for (_, topics, data) in env.events().all().iter() {
        if !topics.is_empty() {
            match topics.get(0).map(|t| t.try_into_val(&env) as Result<soroban_sdk::Symbol, _>) {
                Some(Ok(sym)) if sym == canc_sym => canc_payload = Some(data.into_val(&env)),
                Some(Ok(sym)) if sym == prop_sym => prop_payload = Some(data.into_val(&env)),
                _ => {}
            }
        }
    }
    let (cancelled_by, old_target) = canc_payload.expect("iss_canc event must be present");
    assert_eq!(cancelled_by, issuer, "iss_canc must name the replacing issuer");
    assert_eq!(old_target, first_target, "iss_canc must name the displaced target");

    let (proposed_target, ts) = prop_payload.expect("iss_prop event must be present");
    assert_eq!(proposed_target, second_target, "iss_prop must name the new target");
    assert_eq!(ts, t0 + 42, "iss_prop must carry the replace-time timestamp");
}

/// Replacing with the *same* target is still a valid transition: the target
/// is unchanged but the timestamp is re-stamped (a clock refresh).
#[test]
fn replace_with_same_target_restamps_timestamp() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let target = Address::generate(&env);

    let t0 = 30_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_issuer_transfer(&issuer, &NS, &token, &target);

    env.ledger().set_timestamp(t0 + 999);
    client.replace_issuer_transfer(&issuer, &NS, &token, &target);

    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("pending transfer must still exist after replace");
    assert_eq!(details.new_issuer, target, "same-target replace keeps the target");
    assert_eq!(details.timestamp, t0 + 999, "same-target replace re-stamps the timestamp");
}

// ── Section B: Typed rejection paths ──────────────────────────────────────────

/// Replacing for an offering that does not exist must return the typed
/// `OfferingNotFound` error.
#[test]
fn replace_unknown_offering_returns_offering_not_found() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let stranger = Address::generate(&env);
    let token = Address::generate(&env);
    let new_issuer = Address::generate(&env);

    let result = client.try_replace_issuer_transfer(&stranger, &NS, &token, &new_issuer);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

/// Replacing when no proposal is pending must return the typed
/// `NoTransferPending` error.
#[test]
fn replace_without_pending_transfer_returns_no_transfer_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);

    let result = client.try_replace_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));
}

/// The pending lookup is keyed by the full offering id: a proposal bound to
/// `NS` cannot be replaced through a different namespace. A sibling offering
/// must exist under the wrong namespace so the lookup gets past
/// `OfferingNotFound` and reaches the pending-entry miss.
#[test]
fn replace_with_wrong_namespace_returns_no_transfer_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target = Address::generate(&env);
    let second_target = Address::generate(&env);

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

    client.propose_issuer_transfer(&issuer, &NS, &token, &first_target);

    let result = client.try_replace_issuer_transfer(&issuer, &other_ns, &token, &second_target);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));

    // State unchanged: the correctly-bound replace still succeeds afterwards.
    client.replace_issuer_transfer(&issuer, &NS, &token, &second_target);
    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("pending transfer must exist after the correctly-bound replace");
    assert_eq!(details.new_issuer, second_target);
}

/// Once a transfer has been ACCEPTED, the pending entry is consumed — a later
/// replace by the old issuer returns `NoTransferPending`.
#[test]
fn replace_after_accept_returns_no_transfer_pending() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target = Address::generate(&env);
    let second_target = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &first_target);
    client.accept_issuer_transfer(&first_target, &NS, &token);

    let result = client.try_replace_issuer_transfer(&issuer, &NS, &token, &second_target);
    assert_eq!(result, Err(Ok(RevoraError::NoTransferPending)));
}

/// The reachable `NotAuthorized` path: after a completed accept, the migrated
/// offering record under the NEW issuer's key keeps the OLD primary, so the
/// new issuer querying replace resolves an offering whose primary is someone
/// else → typed `NotAuthorized`.
#[test]
fn replace_by_new_issuer_after_accept_returns_not_authorized() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    let next_target = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &new_issuer);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);

    // The new issuer owns the migrated record, but its primary field still
    // names the old issuer — the identity check must reject the replace.
    let result = client.try_replace_issuer_transfer(&new_issuer, &NS, &token, &next_target);
    assert_eq!(result, Err(Ok(RevoraError::NotAuthorized)));
}

// ── Section C: Frozen / paused guards ─────────────────────────────────────────

/// After `freeze()`, replace returns the typed `ContractFrozen` error and the
/// pending proposal survives (verified by reading the key from the contract
/// frame — storage reads stay permitted while frozen).
#[test]
fn replace_blocked_when_contract_frozen_and_pending_survives() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target = Address::generate(&env);
    let second_target = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_issuer_transfer(&issuer, &NS, &token, &first_target);
    client.freeze();

    let result = client.try_replace_issuer_transfer(&issuer, &NS, &token, &second_target);
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));

    let contract_id = client.address.clone();
    let still_pending = env.as_contract(&contract_id, || {
        env.storage().persistent().has(&crate::DataKey::PendingIssuerTransfer(crate::OfferingId {
            issuer: issuer.clone(),
            namespace: NS.clone(),
            token: token.clone(),
        }))
    });
    assert!(still_pending, "pending proposal must survive a frozen replace");

    // State unchanged: the stored proposal still names the ORIGINAL target
    // (storage reads stay permitted while frozen). Global freeze is one-way
    // (no unfreeze entrypoint), so the frozen shape itself is the end state.
    let frozen_pending = env.as_contract(&contract_id, || {
        env.storage().persistent().get(&crate::DataKey::PendingIssuerTransfer(crate::OfferingId {
            issuer: issuer.clone(),
            namespace: NS.clone(),
            token: token.clone(),
        }))
    });
    let frozen_pending: Option<crate::PendingTransfer> = frozen_pending;
    assert_eq!(
        frozen_pending.map(|p| p.new_issuer),
        Some(first_target),
        "frozen replace must not touch the stored target"
    );
}

/// After `pause_admin()`, replace returns the typed `ContractPaused` error;
/// the issuer (who is the admin here) can unpause and then replace
/// successfully.
#[test]
fn replace_blocked_when_contract_paused_then_unpause_succeeds() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target = Address::generate(&env);
    let second_target = Address::generate(&env);
    // setup_offering already made the issuer the admin (set_admin is one-shot).
    let admin = issuer.clone();

    client.propose_issuer_transfer(&issuer, &NS, &token, &first_target);
    client.pause_admin(&admin);

    let result = client.try_replace_issuer_transfer(&issuer, &NS, &token, &second_target);
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));

    client.unpause_admin(&admin);
    client.replace_issuer_transfer(&issuer, &NS, &token, &second_target);
    assert_eq!(client.get_pending_issuer_transfer(&issuer, &NS, &token), Some(second_target));
}

// ── Section D: Window inheritance is behavioral ───────────────────────────────

/// A replace of a default-window proposal still expires on the default 7-day
/// schedule measured from the ORIGINAL proposal... except that replace
/// re-stamps the timestamp, so the window restarts from the replace time.
/// This pins the composed behavior: `accept` at `replace_ts + default`
/// succeeds and at `replace_ts + default + 1` fails.
#[test]
fn replaced_default_window_accepts_exactly_one_default_window_after_replace() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target = Address::generate(&env);
    let second_target = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    let t0 = 1_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_issuer_transfer(&issuer, &NS, &token, &first_target);

    let t_replace = t0 + DEFAULT_EXPIRY_SECS * 2; // long after the original window
    env.ledger().set_timestamp(t_replace);
    client.replace_issuer_transfer(&issuer, &NS, &token, &second_target);

    // Exactly one default window after the REPLACE timestamp: accepted.
    env.ledger().set_timestamp(t_replace + DEFAULT_EXPIRY_SECS);
    client.accept_issuer_transfer(&second_target, &NS, &token);
}

/// A replaced custom-window proposal keeps the custom window measured from
/// the replace timestamp: `expiry_secs` survives, the clock restarts.
#[test]
fn replaced_custom_window_uses_preserved_secs_from_replace_time() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target = Address::generate(&env);
    let second_target = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    let t0 = 1_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &first_target, &3_600);

    let t_replace = t0 + 10_000;
    env.ledger().set_timestamp(t_replace);
    client.replace_issuer_transfer(&issuer, &NS, &token, &second_target);

    // One second past the preserved window, measured from the replace time.
    env.ledger().set_timestamp(t_replace + 3_601);
    let expired = client.try_accept_issuer_transfer(&second_target, &NS, &token);
    assert_eq!(expired, Err(Ok(RevoraError::IssuerTransferExpired)));
}

// ── Section E: Cross-offering isolation ───────────────────────────────────────

/// Replacing one offering's proposal must not touch a sibling proposal on a
/// second offering with the same token but a different namespace.
#[test]
fn replace_is_isolated_per_offering() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target_ns = Address::generate(&env);
    let replacement_target_ns = Address::generate(&env);
    let target_other = Address::generate(&env);

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

    client.propose_issuer_transfer(&issuer, &NS, &token, &first_target_ns);
    client.propose_issuer_transfer(&issuer, &other_ns, &token, &target_other);

    // Replace only the first proposal.
    client.replace_issuer_transfer(&issuer, &NS, &token, &replacement_target_ns);

    // The sibling proposal is untouched: the original target can still accept it.
    client.accept_issuer_transfer(&target_other, &other_ns, &token);
    assert!(client.get_offering(&target_other, &other_ns, &token).is_some());

    // ...while the replaced one now names the replacement target.
    assert_eq!(
        client.get_pending_issuer_transfer(&issuer, &NS, &token),
        Some(replacement_target_ns)
    );
}
