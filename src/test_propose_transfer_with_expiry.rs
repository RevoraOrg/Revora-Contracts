//! # Adversarial coverage — `propose_transfer_with_expiry` (#1057)
//!
//! Exercises the custom-expiry variant of the issuer-transfer proposal beyond
//! its happy path:
//!
//! - **Success**: a proposal stores a `PendingTransfer` whose `timestamp`
//!   equals the proposal ledger timestamp and whose `expiry_secs` is recorded
//!   verbatim (`0` = "use default"), emits exactly one `iss_prop` event
//!   carrying `(new_issuer, timestamp)`, and is visible through both
//!   `get_pending_issuer_transfer` and `get_pending_transfer_details`.
//! - **Expiry clamping (the core behavior of this entrypoint)**: non-zero
//!   windows are clamped to `[MIN_ISSUER_TRANSFER_EXPIRY_SECS, MAX]`
//!   (1h..30d). The clamp is made *observable* through the accept window:
//!   a proposed `1` behaves as `3600` (accept at `+3600` succeeds, which a
//!   raw `1` would reject), and `u64::MAX` behaves as `2_592_000` (accept at
//!   `+2_592_001` fails). `0` is passed through as the default marker.
//! - **Typed rejections**: unknown offering → `OfferingNotFound`, already
//!   pending → `IssuerTransferPending` (state unchanged: the original target
//!   survives), frozen → `ContractFrozen`, paused → `ContractPaused`.
//! - **Unauthorized callers (observable Layer 2)**: proposal authenticates
//!   the *primary issuer* by identity before any `require_auth`, so the
//!   post-accept stale primary shape yields the typed `OfferingNotFound`,
//!   catchable via `try_*`. Host auth panics (Layer 1) are non-unwinding and
//!   out of `try_*` reach; see `test_auth.rs` Section A for the repo-wide
//!   convention.
//! - **Lifecycle**: a cancelled proposal frees the slot — a new proposal with
//!   a different target succeeds afterwards.
//!
//! ## State machine reference
//!
//! ```text
//! propose/propose_with_expiry (issuer quorum) ──▶ PendingIssuerTransfer{new_issuer, timestamp, expiry_secs}
//!   pending.expiry_secs == 0       → accept window = ISSUER_TRANSFER_EXPIRY_SECS (7d)
//!   pending.expiry_secs == x != 0  → accept window = clamp(x, 1h, 30d)
//! accept (new_issuer auth) : pending removed, offering moved
//! cancel  (issuer quorum)  : pending removed, iss_canc event
//! reject  (new_issuer auth): pending removed
//! replace (issuer quorum)  : pending updated (new target, same stored expiry_secs)
//! ```

#![cfg(test)]

use crate::{PendingTransfer, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short, testutils::Address as _, testutils::Events as _, testutils::Ledger as _, Address,
    Env, IntoVal, TryIntoVal, Vec,
};

/// Default acceptance window (7 days) — `expiry_secs == 0` resolves to this.
const DEFAULT_EXPIRY_SECS: u64 = 7 * 24 * 60 * 60;
/// `MIN_ISSUER_TRANSFER_EXPIRY_SECS` (1 hour) — clamp floor.
const MIN_EXPIRY_SECS: u64 = 60 * 60;
/// `MAX_ISSUER_TRANSFER_EXPIRY_SECS` (30 days) — clamp ceiling.
const MAX_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;

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
/// `propose_transfer_with_expiry` itself does not scan the registry, but the
/// state-unchanged and expiry-window proofs fall through to a real accept,
/// and the accept-side scan needs each namespace of `issuer` listed here.
/// See `test_issuer_transfer_accept.rs` / `test_issuer_transfer_cancel.rs`
/// for the full rationale of this workaround.
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

// ── Section A: Happy path & storage shape ─────────────────────────────────────

/// `expiry_secs == 0` is stored verbatim as the default marker: the pending
/// entry records the proposal timestamp and `expiry_secs == 0`, and is
/// visible through both pending getters.
#[test]
fn zero_expiry_is_stored_as_default_marker() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);

    let t0 = 1_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &0);

    let pending_issuer = client.get_pending_issuer_transfer(&issuer, &NS, &token);
    assert_eq!(pending_issuer, Some(new_issuer.clone()));

    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("pending transfer must be readable after propose");
    assert_eq!(details.new_issuer, new_issuer);
    assert_eq!(details.timestamp, t0, "pending.timestamp must be the proposal ledger timestamp");
    assert_eq!(details.expiry_secs, 0, "expiry_secs == 0 must be stored as the default marker");
}

/// A non-zero in-range expiry is stored verbatim (not re-normalized).
#[test]
fn in_range_expiry_is_stored_verbatim() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);

    client.propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &3_600);

    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("pending transfer must be readable after propose");
    assert_eq!(details.expiry_secs, 3_600);
}

/// Exactly one `iss_prop` event is emitted per successful proposal, carrying
/// `(new_issuer, timestamp)` as data with the issuer/namespace/token topics.
#[test]
fn propose_emits_single_iss_prop_event_with_payload() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);

    let t0 = 5_000u64;
    env.ledger().set_timestamp(t0);
    let prop_sym = symbol_short!("iss_prop");
    let baseline = count_events_with_topic(&env, &prop_sym);

    client.propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &0);

    assert_eq!(
        count_events_with_topic(&env, &prop_sym),
        baseline + 1,
        "exactly one iss_prop event must be emitted by propose"
    );

    let mut payload: Option<(Address, u64)> = None;
    for (_, topics, data) in env.events().all().iter() {
        if !topics.is_empty()
            && topics
                .get(0)
                .map(|t| {
                    t.try_into_val(&env) as Result<soroban_sdk::Symbol, _> == Ok(prop_sym.clone())
                })
                .unwrap_or(false)
        {
            payload = Some(data.into_val(&env));
        }
    }
    let (proposed_target, ts) = payload.expect("iss_prop event must be present");
    assert_eq!(proposed_target, new_issuer, "iss_prop must name the proposed new issuer");
    assert_eq!(ts, t0, "iss_prop payload timestamp must be the proposal ledger timestamp");
}

// ── Section B: Expiry clamping (core behavior of this entrypoint) ─────────────

/// Sub-minimum expiries are clamped up to `MIN_ISSUER_TRANSFER_EXPIRY_SECS`.
/// The clamp is made observable through the accept window: proposing `1`
/// accepts at exactly `t0 + 3600`, which a stored raw `1` would reject.
#[test]
fn sub_minimum_expiry_is_clamped_up_to_one_hour() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    let t0 = 1_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &1);

    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("pending transfer must be readable after propose");
    assert_eq!(
        details.expiry_secs, MIN_EXPIRY_SECS,
        "a sub-minimum expiry must be clamped up to the 1-hour floor"
    );

    // At exactly t0 + 3600 the accept must succeed (boundary inclusive). If
    // the raw `1` had been stored, this accept would be far past expiry.
    env.ledger().set_timestamp(t0 + MIN_EXPIRY_SECS);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);
}

/// Above-maximum expiries are clamped down to `MAX_ISSUER_TRANSFER_EXPIRY_SECS`.
/// Observable through the accept window: proposing `u64::MAX` still expires
/// at `t0 + 30d + 1s`.
#[test]
fn above_maximum_expiry_is_clamped_down_to_thirty_days() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    let t0 = 1_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &u64::MAX);

    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("pending transfer must be readable after propose");
    assert_eq!(
        details.expiry_secs, MAX_EXPIRY_SECS,
        "an above-maximum expiry must be clamped down to the 30-day ceiling"
    );

    // One second past the clamped ceiling the accept must be rejected — an
    // unclamped u64::MAX window would accept at any future time.
    env.ledger().set_timestamp(t0 + MAX_EXPIRY_SECS + 1);
    let expired = client.try_accept_issuer_transfer(&new_issuer, &NS, &token);
    assert_eq!(expired, Err(Ok(RevoraError::IssuerTransferExpired)));
}

/// The clamp bounds themselves are accepted verbatim at both edges.
#[test]
fn clamp_boundary_values_are_accepted_verbatim() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);

    for window in [MIN_EXPIRY_SECS, MAX_EXPIRY_SECS] {
        client.propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &window);
        let details: PendingTransfer = client
            .get_pending_transfer_details(&issuer, &NS, &token)
            .expect("pending transfer must be readable after propose");
        assert_eq!(details.expiry_secs, window, "boundary value must be stored verbatim");
        // Free the slot for the next iteration.
        client.cancel_issuer_transfer(&issuer, &NS, &token);
    }
}

// ── Section C: Typed rejection paths ──────────────────────────────────────────

/// Proposing for an offering that does not exist must return the typed
/// `OfferingNotFound` error.
#[test]
fn propose_unknown_offering_returns_offering_not_found() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();
    let stranger = Address::generate(&env);
    let token = Address::generate(&env);
    let new_issuer = Address::generate(&env);

    let result = client.try_propose_transfer_with_expiry(&stranger, &NS, &token, &new_issuer, &0);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

/// A second proposal while one is pending must return the typed
/// `IssuerTransferPending` error and leave the original proposal untouched
/// (same target, same timestamp).
#[test]
fn propose_while_pending_returns_error_and_state_unchanged() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target = Address::generate(&env);
    let second_target = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    let t0 = 2_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &first_target, &0);

    let result = client.try_propose_transfer_with_expiry(&issuer, &NS, &token, &second_target, &0);
    assert_eq!(result, Err(Ok(RevoraError::IssuerTransferPending)));

    // State unchanged: the original proposal survives the rejected one.
    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("original pending transfer must survive the rejected second propose");
    assert_eq!(details.new_issuer, first_target, "pending target must be unchanged");
    assert_eq!(details.timestamp, t0, "pending timestamp must be unchanged");

    // The original proposal is still fully actionable.
    client.accept_issuer_transfer(&first_target, &NS, &token);
}

/// The reachable identity-check path: after a completed accept, the migrated
/// offering record under the NEW issuer's key keeps the OLD primary, so the
/// new issuer querying a proposal resolves an offering whose primary is
/// someone else → typed `OfferingNotFound`. Pins the identity check on the
/// post-transfer state shape.
#[test]
fn propose_by_new_issuer_after_accept_returns_offering_not_found() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    let next_target = Address::generate(&env);
    seed_issuer_registry(&env, &client.address, &issuer, core::slice::from_ref(&NS));

    client.propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &0);
    client.accept_issuer_transfer(&new_issuer, &NS, &token);

    // The new issuer owns the migrated record, but its primary field still
    // names the old issuer — the identity check must reject the proposal.
    let result =
        client.try_propose_transfer_with_expiry(&new_issuer, &NS, &token, &next_target, &0);
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));
}

// ── Section D: Frozen / paused guards ─────────────────────────────────────────

/// After `freeze()`, propose returns the typed `ContractFrozen` error and no
/// pending entry is created.
#[test]
fn propose_blocked_when_contract_frozen_and_no_state_written() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);

    client.freeze();

    let result = client.try_propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &0);
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));

    let contract_id = client.address.clone();
    let pending_absent = env.as_contract(&contract_id, || {
        !env.storage().persistent().has(&crate::DataKey::PendingIssuerTransfer(crate::OfferingId {
            issuer: issuer.clone(),
            namespace: NS.clone(),
            token: token.clone(),
        }))
    });
    assert!(pending_absent, "a frozen propose must not write a pending entry");
}

/// After `pause_admin()`, propose returns the typed `ContractPaused` error;
/// the issuer (who is the admin here) can unpause and then propose
/// successfully.
#[test]
fn propose_blocked_when_contract_paused_then_unpause_succeeds() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let new_issuer = Address::generate(&env);
    // setup_offering already made the issuer the admin (set_admin is one-shot).
    let admin = issuer.clone();

    client.pause_admin(&admin);
    let result = client.try_propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &0);
    assert_eq!(result, Err(Ok(RevoraError::ContractPaused)));

    client.unpause_admin(&admin);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &new_issuer, &0);
    assert!(client.get_pending_issuer_transfer(&issuer, &NS, &token).is_some());
}

// ── Section E: Proposal lifecycle ─────────────────────────────────────────────

/// A cancelled proposal frees the slot: a new proposal with a different
/// target succeeds and fully replaces the observable pending state.
#[test]
fn propose_after_cancel_starts_fresh_slot() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token) = setup_offering(&env, &client);
    let first_target = Address::generate(&env);
    let second_target = Address::generate(&env);

    let t0 = 3_000u64;
    env.ledger().set_timestamp(t0);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &first_target, &0);
    client.cancel_issuer_transfer(&issuer, &NS, &token);

    env.ledger().set_timestamp(t0 + 100);
    client.propose_transfer_with_expiry(&issuer, &NS, &token, &second_target, &0);

    let details: PendingTransfer = client
        .get_pending_transfer_details(&issuer, &NS, &token)
        .expect("pending transfer must exist after the fresh proposal");
    assert_eq!(details.new_issuer, second_target);
    assert_eq!(details.timestamp, t0 + 100, "the fresh proposal must carry its own timestamp");
}
