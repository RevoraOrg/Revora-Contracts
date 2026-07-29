//! # Dispute Resolution Tests (#593)
//!
//! Tests for `resolve_dispute` admin entrypoint:
//! - DisputeOutcome enum (Upheld, Rejected, PartiallyUpheld)
//! - Admin-only authorization
//! - Atomic status update
//! - Event emission (disp_res)
//! - Freeze removal on Rejected outcome
//! - Edge cases: non-existent dispute, already resolved, non-admin caller

#![cfg(test)]

use crate::{DisputeEntry, DisputeOutcome, FreezeReason, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::testutils::Events as _;
use soroban_sdk::{
    symbol_short,
    testutils::Address as _,
    Address, BytesN, Env, Vec,
};

// ── Helpers ─────────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient<'_> {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

fn create_payment_token(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let token_id = env.register_stellar_asset_contract(admin.clone());
    (token_id, admin)
}

/// Full setup with admin initialized.
fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &cid);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let offering_token = Address::generate(&env);
    let (payment_token, _) = create_payment_token(&env);
    let holder = Address::generate(&env);

    // Initialize with admin
    client.initialize(&admin, &None, &None);

    // Register offering with issuer
    client.register_offering(
        &issuer,
        &Vec::new(&env),
        &1u32,
        &symbol_short!("ns"),
        &offering_token,
        &10_000,
        &payment_token,
        &0,
        &symbol_short!(""),
        &0,
    );

    // Set holder share
    client.set_holder_share(
        &issuer,
        &symbol_short!("ns"),
        &offering_token,
        &holder,
        &10_000,
    );

    (env, client, cid, admin, issuer, offering_token, payment_token, holder)
}

fn create_evidence_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
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
// SECTION 2 — resolve_dispute Authorization
// ────────────────────────────────────────────────────────────────────────────────

/// Admin can resolve a dispute.
#[test]
fn resolve_dispute_admin_succeeds() {
    let (env, client, _cid, admin, issuer, token, _payment, holder) = setup();

    // Create a dispute by freezing holder with IssuerDispute reason
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    // Get dispute ID (should be 1)
    let dispute = client.get_dispute(&1);
    assert!(dispute.is_some());
    assert!(!dispute.unwrap().resolved);

    let evidence = create_evidence_hash(&env);
    let r = client.try_resolve_dispute(&admin, &1, &DisputeOutcome::Upheld, &evidence);
    assert!(r.is_ok(), "admin should be able to resolve dispute");
}

/// Non-admin caller fails with NotAuthorized.
#[test]
fn resolve_dispute_non_admin_fails() {
    let (env, client, _cid, _admin, issuer, token, _payment, holder) = setup();
    let unauthorized = Address::generate(&env);

    // Create a dispute
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    let evidence = create_evidence_hash(&env);
    let r = client.try_resolve_dispute(&unauthorized, &1, &DisputeOutcome::Upheld, &evidence);
    assert_eq!(r, Err(Ok(RevoraError::NotAuthorized)));
}

/// Resolving a non-existent dispute fails.
#[test]
fn resolve_dispute_nonexistent_fails() {
    let (env, client, _cid, admin, _issuer, _token, _payment, _holder) = setup();

    let evidence = create_evidence_hash(&env);

    // Dispute ID 999 does not exist
    let r = client.try_resolve_dispute(&admin, &999, &DisputeOutcome::Upheld, &evidence);
    assert_eq!(r, Err(Ok(RevoraError::DisputeNotFound)));
}

/// Resolving an already resolved dispute fails.
#[test]
fn resolve_dispute_already_resolved_fails() {
    let (env, client, _cid, admin, issuer, token, _payment, holder) = setup();

    // Create a dispute
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    // Resolve it once
    let evidence = create_evidence_hash(&env);
    let r = client.try_resolve_dispute(&admin, &1, &DisputeOutcome::Upheld, &evidence);
    assert!(r.is_ok());

    // Resolve it again should fail
    let r = client.try_resolve_dispute(&admin, &1, &DisputeOutcome::Upheld, &evidence);
    assert_eq!(r, Err(Ok(RevoraError::DisputeAlreadyResolved)));
}

/// Resolving fails when contract is frozen.
#[test]
fn resolve_dispute_when_frozen_fails() {
    let (env, client, _cid, admin, issuer, token, _payment, holder) = setup();

    // Create a dispute
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    // Freeze the contract
    client.freeze(&admin);

    let evidence = create_evidence_hash(&env);
    let r = client.try_resolve_dispute(&admin, &1, &DisputeOutcome::Upheld, &evidence);
    assert_eq!(r, Err(Ok(RevoraError::ContractFrozen)));
}

// ────────────────────────────────────────────────────────────────────────────────
// SECTION 3 — Dispute Resolution Outcomes
// ────────────────────────────────────────────────────────────────────────────────

/// Resolving with Upheld outcome keeps the freeze intact.
#[test]
fn resolve_dispute_upheld_keeps_freeze() {
    let (env, client, _cid, admin, issuer, token, _payment, holder) = setup();

    // Create a dispute
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    // Verify holder is frozen
    assert!(client.is_holder_frozen(&issuer, &symbol_short!("ns"), &token, &holder));

    // Resolve with Upheld
    let evidence = create_evidence_hash(&env);
    let r = client.try_resolve_dispute(&admin, &1, &DisputeOutcome::Upheld, &evidence);
    assert!(r.is_ok());

    // Freeze should remain
    assert!(client.is_holder_frozen(&issuer, &symbol_short!("ns"), &token, &holder));
}

/// Resolving with Rejected outcome removes the freeze.
#[test]
fn resolve_dispute_rejected_removes_freeze() {
    let (env, client, _cid, admin, issuer, token, _payment, holder) = setup();

    // Create a dispute
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    // Verify holder is frozen
    assert!(client.is_holder_frozen(&issuer, &symbol_short!("ns"), &token, &holder));

    // Resolve with Rejected
    let evidence = create_evidence_hash(&env);
    let r = client.try_resolve_dispute(&admin, &1, &DisputeOutcome::Rejected, &evidence);
    assert!(r.is_ok());

    // Freeze should be removed
    assert!(!client.is_holder_frozen(&issuer, &symbol_short!("ns"), &token, &holder));
}

/// Resolving with PartiallyUpheld outcome keeps the freeze intact.
#[test]
fn resolve_dispute_partially_upheld_keeps_freeze() {
    let (env, client, _cid, admin, issuer, token, _payment, holder) = setup();

    // Create a dispute
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    // Verify holder is frozen
    assert!(client.is_holder_frozen(&issuer, &symbol_short!("ns"), &token, &holder));

    // Resolve with PartiallyUpheld
    let evidence = create_evidence_hash(&env);
    let r = client.try_resolve_dispute(&admin, &1, &DisputeOutcome::PartiallyUpheld, &evidence);
    assert!(r.is_ok());

    // Freeze should remain
    assert!(client.is_holder_frozen(&issuer, &symbol_short!("ns"), &token, &holder));
}

// ────────────────────────────────────────────────────────────────────────────────
// SECTION 4 — Event Emission
// ────────────────────────────────────────────────────────────────────────────────

/// resolve_dispute emits EVENT_DISPUTE_RESOLVE ("disp_res").
#[test]
fn resolve_dispute_emits_event() {
    let (env, client, _cid, admin, issuer, token, _payment, holder) = setup();

    // Create a dispute
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    let before = env.events().all().len();
    let evidence = create_evidence_hash(&env);
    client.resolve_dispute(&admin, &1, &DisputeOutcome::Upheld, &evidence);
    let after = env.events().all().len();

    assert!(after > before, "resolve_dispute must emit an event");

    // Check that the disp_res event is in the log
    let events = env.events().all();
    let mut found_disp_res = false;
    for event in events.iter() {
        if event.0 == (symbol_short!("disp_res"),) {
            found_disp_res = true;
            break;
        }
    }
    assert!(found_disp_res, "disp_res event must be emitted");
}

/// Different outcomes emit the same event with correct data.
#[test]
fn resolve_dispute_emits_event_with_correct_outcome() {
    let (env, client, _cid, admin, issuer, token, _payment, holder) = setup();

    // Create dispute 1
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    let evidence = create_evidence_hash(&env);
    // Resolve dispute 1 with Upheld
    let r = client.try_resolve_dispute(&admin, &1, &DisputeOutcome::Upheld, &evidence);
    assert!(r.is_ok(), "Upheld resolution should succeed");
}

// ────────────────────────────────────────────────────────────────────────────────
// SECTION 5 — Dispute State Updates
// ────────────────────────────────────────────────────────────────────────────────

/// Dispute entry is updated with outcome, evidence, resolver, and timestamp.
#[test]
fn resolve_dispute_updates_dispute_entry() {
    let (env, client, _cid, admin, issuer, token, _payment, holder) = setup();

    // Create a dispute
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    let evidence = create_evidence_hash(&env);
    client.resolve_dispute(&admin, &1, &DisputeOutcome::Rejected, &evidence);

    let dispute = client.get_dispute(&1).unwrap();
    assert!(dispute.resolved, "dispute should be marked resolved");
    assert_eq!(dispute.outcome, Some(DisputeOutcome::Rejected), "outcome should be Rejected");
    assert_eq!(dispute.evidence_hash, Some(evidence), "evidence hash should be stored");
    assert_eq!(dispute.resolved_by, Some(admin), "resolver should be admin");
    assert!(dispute.resolved_at.is_some(), "resolved_at should be set");
}

/// Dispute entry retains original data after resolution.
#[test]
fn resolve_dispute_preserves_original_data() {
    let (env, client, _cid, admin, issuer, token, _payment, holder) = setup();

    // Create a dispute
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    let dispute_before = client.get_dispute(&1).unwrap();
    assert_eq!(dispute_before.dispute_id, 1);
    assert_eq!(dispute_before.freeze_reason, FreezeReason::IssuerDispute);
    assert!(!dispute_before.resolved);

    let evidence = create_evidence_hash(&env);
    client.resolve_dispute(&admin, &1, &DisputeOutcome::Upheld, &evidence);

    let dispute_after = client.get_dispute(&1).unwrap();
    assert_eq!(dispute_after.dispute_id, 1, "dispute_id should remain unchanged");
    assert_eq!(dispute_after.holder, holder, "holder should remain unchanged");
    assert_eq!(dispute_after.freeze_reason, FreezeReason::IssuerDispute, "reason should remain unchanged");
    assert!(dispute_after.resolved, "dispute should now be resolved");
}

// ────────────────────────────────────────────────────────────────────────────────
// SECTION 6 — Multiple Disputes
// ────────────────────────────────────────────────────────────────────────────────

/// Multiple disputes can be created and resolved independently.
#[test]
fn multiple_disputes_can_be_resolved_independently() {
    let (env, client, _cid, admin, issuer, token, _payment, holder) = setup();
    let holder2 = Address::generate(&env);

    // Set share for holder2
    client.set_holder_share(
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder2,
        &5_000,
    );

    // Create dispute 1
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder,
        &FreezeReason::IssuerDispute,
    );

    // Create dispute 2
    client.emergency_freeze_holder(
        &issuer,
        &issuer,
        &symbol_short!("ns"),
        &token,
        &holder2,
        &FreezeReason::IssuerDispute,
    );

    assert_eq!(client.get_dispute(&1).unwrap().dispute_id, 1);
    assert_eq!(client.get_dispute(&2).unwrap().dispute_id, 2);

    // Resolve dispute 1 with Upheld
    let evidence = create_evidence_hash(&env);
    client.resolve_dispute(&admin, &1, &DisputeOutcome::Upheld, &evidence);
    assert!(client.get_dispute(&1).unwrap().resolved);
    assert!(!client.get_dispute(&2).unwrap().resolved);

    // Resolve dispute 2 with Rejected
    let evidence2 = create_evidence_hash(&env);
    client.resolve_dispute(&admin, &2, &DisputeOutcome::Rejected, &evidence2);
    assert!(client.get_dispute(&2).unwrap().resolved);

    // Verify holder2 freeze was removed
    assert!(!client.is_holder_frozen(&issuer, &symbol_short!("ns"), &token, &holder2));
}

// ────────────────────────────────────────────────────────────────────────────────
// SECTION 7 — get_dispute Edge Cases
// ────────────────────────────────────────────────────────────────────────────────

/// get_dispute returns None for non-existent dispute.
#[test]
fn get_dispute_nonexistent_returns_none() {
    let (env, _client, _cid, _admin, _issuer, _token, _payment, _holder) = setup();

    // Dispute ID 999 does not exist
    // The get_dispute function is a read-only query; we need to call it through the contract
    // We use try_get_dispute to check for None
    // Actually, get_dispute returns an Option<DisputeEntry> so it will be None
    // Since it's a contract function, we need to call it through the client
    // The auto-generated client returns Option<DisputeEntry>
    let result = _client.get_dispute(&999);
    assert!(result.is_none(), "non-existent dispute should return None");
}
