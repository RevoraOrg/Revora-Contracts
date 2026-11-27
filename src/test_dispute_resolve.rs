//! # Dispute Resolution Tests (#593)
//!
//! Tests for `resolve_dispute` and `DisputeOutcome` / `get_dispute_entry`:
//! - `DisputeOutcome` enum distinctness
//! - `resolve_dispute` behavior when contract is globally frozen
//! - `disp_res` event payload verification
//! - `Dispute` state updates and original field preservation across resolution
//! - Independent resolution of multiple disputes
//! - `get_dispute_entry` returning `None` for non-existent sequential IDs
//!
//! Note: Duplicate cases already covered in `test_dispute.rs` (wrong caller,
//! non-existent dispute, already resolved, freeze lift, `disp_res` emission)
//! were removed during deduplication.

#![cfg(test)]

use crate::{
    DisputeOutcome, DisputeSeverity, DisputeStatus, OfferingId, RevoraRevenueShare,
    RevoraRevenueShareClient,
};
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{symbol_short, Address, BytesN, Env, IntoVal, Symbol, Vec};

// ── Helpers ─────────────────────────────────────────────────────────────────────

fn setup(
) -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address, Address, Address, Address)
{
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let payment_token = Address::generate(&env);
    let holder = Address::generate(&env);

    client.initialize(&admin, &None, &None);

    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &offering_token,
        &2_500,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0,
    );

    client.set_holder_share(
        &issuer,
        &symbol_short!("ns"),
        &offering_token,
        &holder,
        &5_000,
        &1,
        &Vec::from_array(&env, [issuer.clone()]),
    );

    (env, client, cid, admin, issuer, offering_token, payment_token, holder)
}

fn meta_hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

// ────────────────────────────────────────────────────────────────────────────────
// SECTION 1 — DisputeOutcome Enum Tests
// ────────────────────────────────────────────────────────────────────────────────

/// Verify DisputeOutcome variants exist and are distinct.
#[test]
fn dispute_outcome_variants_are_distinct() {
    assert_ne!(DisputeOutcome::Upheld as u32, DisputeOutcome::Rejected as u32);
    assert_ne!(DisputeOutcome::Rejected as u32, DisputeOutcome::PartiallyUpheld as u32);
    assert_ne!(DisputeOutcome::Upheld as u32, DisputeOutcome::PartiallyUpheld as u32);
}

// ────────────────────────────────────────────────────────────────────────────────
// SECTION 2 — resolve_dispute Lifecycle & Global Freeze Behavior
// ────────────────────────────────────────────────────────────────────────────────

/// Updated for `resolve_dispute` in `src/lib.rs`: `resolve_dispute` does not
/// call `require_not_frozen`, so an issuer can still resolve an open dispute
/// while the contract is globally frozen.
#[test]
fn resolve_dispute_succeeds_when_contract_frozen() {
    let (env, client, _cid, _admin, issuer, token, _payment, holder) = setup();

    let dispute_id = client.open_dispute(
        &holder,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &DisputeSeverity::Standard,
        &meta_hash(&env, 1),
    );

    client.freeze();

    let r = client.try_resolve_dispute(&issuer, &dispute_id, &DisputeStatus::Resolved);
    assert!(r.is_ok(), "resolve_dispute should succeed even when contract is globally frozen");
    assert_eq!(client.get_dispute(&dispute_id).unwrap().status, DisputeStatus::Resolved);
}

/// Updated for `resolve_dispute` in `src/lib.rs`: `disp_res` event payload
/// includes `(offering_id, holder, dispute_id, resolution)`.
#[test]
fn resolve_dispute_emits_event_with_correct_status() {
    let (env, client, _cid, _admin, issuer, token, _payment, holder) = setup();

    let dispute_id = client.open_dispute(
        &holder,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &DisputeSeverity::Standard,
        &meta_hash(&env, 2),
    );

    client.resolve_dispute(&issuer, &dispute_id, &DisputeStatus::Rejected);

    let events = env.events().all();
    let mut found = false;
    for event in events.iter() {
        let (_, topics, data) = event;
        if topics.is_empty() {
            continue;
        }
        let t0: Symbol = topics.get(0).unwrap().into_val(&env);
        if t0 == symbol_short!("disp_res") {
            let payload: (OfferingId, Address, BytesN<32>, DisputeStatus) = data.into_val(&env);
            assert_eq!(payload.1, holder);
            assert_eq!(payload.2, dispute_id);
            assert_eq!(payload.3, DisputeStatus::Rejected);
            found = true;
        }
    }
    assert!(found, "disp_res event with Rejected status must be emitted");
}

/// Updated for `resolve_dispute` and `get_dispute` in `src/lib.rs`:
/// resolving updates `dispute.status` to the target `DisputeStatus`.
#[test]
fn resolve_dispute_updates_dispute_status() {
    let (env, client, _cid, _admin, issuer, token, _payment, holder) = setup();

    let dispute_id = client.open_dispute(
        &holder,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &DisputeSeverity::Standard,
        &meta_hash(&env, 3),
    );

    assert_eq!(client.get_dispute(&dispute_id).unwrap().status, DisputeStatus::Open);

    client.resolve_dispute(&issuer, &dispute_id, &DisputeStatus::Rejected);

    let dispute = client.get_dispute(&dispute_id).unwrap();
    assert_eq!(dispute.status, DisputeStatus::Rejected);
}

/// Updated for `resolve_dispute` and `get_dispute` in `src/lib.rs`:
/// original dispute fields remain unchanged after resolution.
#[test]
fn resolve_dispute_preserves_original_data() {
    let (env, client, _cid, _admin, issuer, token, _payment, holder) = setup();
    let m = meta_hash(&env, 4);

    let dispute_id = client.open_dispute(
        &holder,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &DisputeSeverity::Critical,
        &m,
    );

    let dispute_before = client.get_dispute(&dispute_id).unwrap();
    assert_eq!(dispute_before.status, DisputeStatus::Open);

    client.resolve_dispute(&issuer, &dispute_id, &DisputeStatus::Resolved);

    let dispute_after = client.get_dispute(&dispute_id).unwrap();
    assert_eq!(dispute_after.id, dispute_before.id);
    assert_eq!(dispute_after.holder, holder);
    assert_eq!(dispute_after.offering_id, dispute_before.offering_id);
    assert_eq!(dispute_after.opened_at, dispute_before.opened_at);
    assert_eq!(dispute_after.severity, DisputeSeverity::Critical);
    assert_eq!(dispute_after.meta_hash, m);
    assert_eq!(dispute_after.status, DisputeStatus::Resolved);
}

/// Multiple disputes from different holders can be opened and resolved independently.
#[test]
fn multiple_disputes_can_be_resolved_independently() {
    let (env, client, _cid, _admin, issuer, token, _payment, holder) = setup();
    let holder2 = Address::generate(&env);

    client.set_holder_share(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder2,
        &5_000,
        &1,
        &Vec::from_array(&env, [issuer.clone()]),
    );

    let id1 = client.open_dispute(
        &holder,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &DisputeSeverity::Standard,
        &meta_hash(&env, 10),
    );
    let id2 = client.open_dispute(
        &holder2,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &DisputeSeverity::Standard,
        &meta_hash(&env, 11),
    );

    client.resolve_dispute(&issuer, &id1, &DisputeStatus::Resolved);
    assert_eq!(client.get_dispute(&id1).unwrap().status, DisputeStatus::Resolved);
    assert_eq!(client.get_dispute(&id2).unwrap().status, DisputeStatus::Open);

    client.resolve_dispute(&issuer, &id2, &DisputeStatus::Rejected);
    assert_eq!(client.get_dispute(&id2).unwrap().status, DisputeStatus::Rejected);
}

// ────────────────────────────────────────────────────────────────────────────────
// SECTION 3 — get_dispute_entry Edge Cases
// ────────────────────────────────────────────────────────────────────────────────

/// `get_dispute_entry` returns `None` for non-existent sequential dispute ID.
#[test]
fn get_dispute_entry_nonexistent_returns_none() {
    let (_env, client, _cid, _admin, _issuer, _token, _payment, _holder) = setup();
    let result = client.get_dispute_entry(&999);
    assert!(result.is_none(), "non-existent dispute should return None");
}
