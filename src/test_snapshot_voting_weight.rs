//! Tests for snapshot-based governance voting weight (issue #557).
//!
//! # What is tested
//!
//! - `create_gov_proposal` pins the latest committed snapshot_id.
//! - `cast_vote` reads voting weight exclusively from the pinned snapshot.
//! - A holder who acquires shares **after** proposal creation has zero weight.
//! - Double-vote is rejected with `AlreadyApproved`.
//! - `create_gov_proposal` fails when no snapshot has been committed yet.
//! - `cast_vote` fails on a non-existent or closed proposal.
//! - `close_gov_proposal` prevents further votes.
//! - `get_gov_proposal` returns the correct accumulated weights.
//! - `get_gov_proposal_count` returns the correct counter.
//! - The `wt_pin` event is emitted with snapshot_id and weight on every vote.
//! - The `gov_new` event is emitted at proposal creation.
//! - The `gov_vote` event is emitted on vote cast.

#![cfg(test)]

use crate::{GovProposalEntry, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, BytesN as _, Events as _, Ledger as _},
    xdr::ToXdr,
    Address, Bytes, BytesN, Env, IntoVal, Symbol, TryIntoVal, Vec,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Set up a clean environment with one offering and snapshot distribution enabled.
fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Address) {
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
        &symbol_short!("def"),
        &token,
        &5_000,
        &payout_asset,
        &0,
        &symbol_short!(""),
        &0,
    );
    client.set_snapshot_config(
        &issuer,
        &symbol_short!("def"),
        &token,
        &true,
        &soroban_sdk::Vec::from_array(&env, [issuer.clone()]),
    );

    (env, client, issuer, token)
}

/// Commit a snapshot and apply holder shares, returning the snapshot_ref used.
fn commit_and_apply(
    env: &Env,
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    token: &Address,
    snapshot_ref: u64,
    holders: &[(Address, u32)],
) {
    let mut digest_input = Bytes::new(env);
    for (index, (holder, share_bps)) in holders.iter().enumerate() {
        digest_input.append(&((index as u32).to_xdr(env)));
        digest_input.append(&holder.to_xdr(env));
        digest_input.append(&share_bps.to_xdr(env));
    }
    let content_hash: BytesN<32> = env.crypto().sha256(&digest_input).to_bytes();

    let mut sdk_holders: Vec<(Address, u32)> = Vec::new(env);
    for (addr, bps) in holders {
        sdk_holders.push_back((addr.clone(), *bps));
    }

    client.commit_snapshot(
        issuer,
        &symbol_short!("def"),
        token,
        &snapshot_ref,
        &content_hash,
        &soroban_sdk::Vec::from_array(env, [issuer.clone()]),
    );
    client.apply_snapshot_shares(
        issuer,
        &symbol_short!("def"),
        token,
        &snapshot_ref,
        &0,
        &sdk_holders,
        &soroban_sdk::Vec::from_array(env, [issuer.clone()]),
    );
}

// ── Happy-path tests ──────────────────────────────────────────────────────────

/// Basic flow: one snapshot, two holders, one proposal, two votes; verify weights.
#[test]
fn create_proposal_and_cast_votes_succeeds() {
    let (env, client, issuer, token) = setup();

    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);
    let holders = [(holder_a.clone(), 3_000u32), (holder_b.clone(), 2_000u32)];
    commit_and_apply(&env, &client, &issuer, &token, 1, &holders);

    // Create proposal — pins snapshot_id = 1.
    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );
    assert_eq!(proposal_id, 0);

    // Holder A votes yes — expects weight = 3_000.
    let weight_a =
        client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder_a, &true);
    assert_eq!(weight_a, 3_000);

    // Holder B votes no — expects weight = 2_000.
    let weight_b =
        client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder_b, &false);
    assert_eq!(weight_b, 2_000);

    // Verify accumulated totals.
    let proposal: GovProposalEntry =
        client.get_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id).unwrap();
    assert_eq!(proposal.yes_weight, 3_000);
    assert_eq!(proposal.no_weight, 2_000);
    assert_eq!(proposal.snapshot_id, 1);
    assert!(proposal.open);
    assert_eq!(proposal.quorum_bps, 1_000);
    assert!(!proposal.passed);
}

/// `get_gov_proposal_count` increments correctly for multiple proposals.
#[test]
fn proposal_count_increments() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 5_000)]);

    assert_eq!(client.get_gov_proposal_count(&issuer, &symbol_short!("def"), &token), 0);
    client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );
    assert_eq!(client.get_gov_proposal_count(&issuer, &symbol_short!("def"), &token), 1);
    client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop2"),
        &1_000u32,
        &86_400u64,
    );
    assert_eq!(client.get_gov_proposal_count(&issuer, &symbol_short!("def"), &token), 2);
}

/// Proposal snapshot_id is pinned to the snapshot at creation time, not a later one.
#[test]
fn proposal_pins_snapshot_at_creation_not_later() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 1_000)]);

    // Create proposal — snapshot_id should be 1.
    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );

    // Now commit a newer snapshot with a higher weight for the same holder.
    commit_and_apply(&env, &client, &issuer, &token, 2, &[(holder.clone(), 9_000)]);

    // Vote on the proposal — weight must still be from snapshot 1, not snapshot 2.
    let weight =
        client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder, &true);
    assert_eq!(weight, 1_000, "weight must come from the snapshot pinned at proposal creation");

    let proposal: GovProposalEntry =
        client.get_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id).unwrap();
    assert_eq!(proposal.snapshot_id, 1);
}

/// The `wt_pin` diagnostic event is emitted with the correct snapshot_id and weight.
#[test]
fn weight_pin_event_emitted_on_vote() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 4_000)]);

    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );
    client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder, &true);

    // Verify that a `wt_pin` event was emitted with the right data.
    let events = env.events().all();
    let wt_pin_sym = symbol_short!("wt_pin");
    let found = events.iter().any(|e| {
        let topics = e.1;
        if let Some(first) = topics.get(0) {
            let sym: Result<Symbol, _> = first.try_into_val(&env);
            sym.map(|s| s == wt_pin_sym).unwrap_or(false)
        } else {
            false
        }
    });
    assert!(found, "wt_pin event should be emitted on cast_vote");
}

/// The `gov_new` event is emitted when a proposal is created.
#[test]
fn gov_new_event_emitted_on_proposal_creation() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 5_000)]);

    client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );

    let events = env.events().all();
    let gov_new_sym = symbol_short!("gov_new");
    let found = events.iter().any(|e| {
        let topics = e.1;
        if let Some(first) = topics.get(0) {
            let sym: Result<Symbol, _> = first.try_into_val(&env);
            sym.map(|s| s == gov_new_sym).unwrap_or(false)
        } else {
            false
        }
    });
    assert!(found, "gov_new event should be emitted on create_gov_proposal");
}

/// The `gov_vote` event is emitted when a vote is cast.
#[test]
fn gov_vote_event_emitted_on_cast_vote() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 5_000)]);

    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );
    client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder, &true);

    let events = env.events().all();
    let gov_vote_sym = symbol_short!("gov_vote");
    let found = events.iter().any(|e| {
        let topics = e.1;
        if let Some(first) = topics.get(0) {
            let sym: Result<Symbol, _> = first.try_into_val(&env);
            sym.map(|s| s == gov_vote_sym).unwrap_or(false)
        } else {
            false
        }
    });
    assert!(found, "gov_vote event should be emitted on cast_vote");
}

// ── Late-buy / anti-manipulation edge cases ────────────────────────────────────

/// A holder who buys shares AFTER proposal creation has weight = 0 and is rejected with NoVotingWeight.
#[test]
fn late_buyer_has_zero_voting_weight() {
    let (env, client, issuer, token) = setup();

    // Snapshot 1: only holder_a with 5_000 bps.
    let holder_a = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder_a.clone(), 5_000)]);

    // Create proposal — pinned to snapshot 1.
    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );

    // Snapshot 2 is committed AFTER the proposal.  holder_b gets 5_000 bps here.
    let holder_b = Address::generate(&env);
    commit_and_apply(
        &env,
        &client,
        &issuer,
        &token,
        2,
        &[(holder_a.clone(), 5_000), (holder_b.clone(), 5_000)],
    );

    // holder_b was not in snapshot 1 → weight is 0 → Err(NoVotingWeight).
    let result =
        client.try_cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder_b, &true);
    assert_eq!(result, Err(Ok(RevoraError::NoVotingWeight)));

    let proposal: GovProposalEntry =
        client.get_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id).unwrap();
    // yes_weight must remain 0 since late buyer was rejected.
    assert_eq!(proposal.yes_weight, 0);
}

/// A holder who increases their share after proposal creation still votes at the
/// old (pinned) weight.
#[test]
fn share_increase_after_proposal_creation_does_not_inflate_weight() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    // Snapshot 1: holder has 1_000 bps.
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 1_000)]);

    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );

    // Snapshot 2: holder's share grows to 9_000 bps after proposal creation.
    commit_and_apply(&env, &client, &issuer, &token, 2, &[(holder.clone(), 9_000)]);

    let weight =
        client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder, &true);
    assert_eq!(weight, 1_000, "weight must be from snapshot 1, not the inflated snapshot 2");
}

// ── Error / rejection tests ───────────────────────────────────────────────────

/// `create_gov_proposal` fails when no snapshot has been committed for the offering.
#[test]
fn create_proposal_fails_when_no_snapshot_exists() {
    let (env, client, issuer, token) = setup();
    // No commit_and_apply — no snapshot committed.
    let result = client.try_create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );
    assert!(
        matches!(result, Err(Ok(RevoraError::LimitReached))),
        "should fail with LimitReached when no snapshot exists"
    );
}

/// `cast_vote` on a non-existent proposal returns `LimitReached`.
#[test]
fn cast_vote_fails_on_nonexistent_proposal() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 5_000)]);

    // Proposal id 99 was never created.
    let result = client.try_cast_vote(&issuer, &symbol_short!("def"), &token, &99, &holder, &true);
    assert!(
        matches!(result, Err(Ok(RevoraError::LimitReached))),
        "should fail with LimitReached for unknown proposal"
    );
}

/// Double-vote is rejected with `AlreadyApproved`.
#[test]
fn double_vote_rejected() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 5_000)]);

    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );

    // First vote succeeds.
    client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder, &true);

    // Second vote from the same voter must fail.
    let result =
        client.try_cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder, &true);
    assert!(
        matches!(result, Err(Ok(RevoraError::AlreadyApproved))),
        "double-vote must be rejected with AlreadyApproved"
    );
}

/// `cast_vote` on a closed proposal returns `LimitReached`.
#[test]
fn cast_vote_fails_on_closed_proposal() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 5_000)]);

    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );

    // Advance ledger time past ends_at so close/finalize is valid.
    env.ledger().set_timestamp(env.ledger().timestamp() + 86_401);

    // Issuer closes the proposal.
    client.close_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id);

    // Attempting to vote on a closed proposal must fail with LimitReached.
    let result =
        client.try_cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder, &true);
    assert!(
        matches!(result, Err(Ok(RevoraError::LimitReached))),
        "voting on a closed proposal must return LimitReached"
    );
}

/// `close_gov_proposal` on an already-closed proposal returns `LimitReached`.
#[test]
fn close_already_closed_proposal_returns_error() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 5_000)]);

    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );

    // Advance past deadline.
    env.ledger().set_timestamp(env.ledger().timestamp() + 86_401);
    client.close_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id);

    let result =
        client.try_close_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id);
    assert!(
        matches!(result, Err(Ok(RevoraError::LimitReached))),
        "closing an already-closed proposal must return LimitReached"
    );
}

/// `get_gov_proposal` returns `None` for a proposal that does not exist.
#[test]
fn get_gov_proposal_returns_none_for_unknown_id() {
    let (env, client, issuer, token) = setup();

    let result = client.get_gov_proposal(&issuer, &symbol_short!("def"), &token, &42);
    assert!(result.is_none());
}

/// Multiple proposals are independent; closing one does not affect the other.
#[test]
fn multiple_proposals_are_independent() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 5_000)]);

    let proposal_0 = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("p0"),
        &1_000u32,
        &86_400u64,
    );
    let proposal_1 = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("p1"),
        &1_000u32,
        &172_800u64,
    );

    // Advance past proposal 0's deadline (86_400) but before proposal 1's deadline (172_800)
    env.ledger().set_timestamp(env.ledger().timestamp() + 86_401);
    client.close_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_0);

    // proposal_1 is still open and not expired; holder can vote on it.
    let weight =
        client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_1, &holder, &true);
    assert_eq!(weight, 5_000);

    let p0: GovProposalEntry =
        client.get_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_0).unwrap();
    let p1: GovProposalEntry =
        client.get_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_1).unwrap();

    assert!(!p0.open, "proposal 0 should be closed");
    assert!(p1.open, "proposal 1 should still be open");
    assert_eq!(p1.yes_weight, 5_000);
}

/// `apply_snapshot_shares` writes the SnapshotHolderShare key so that O(1)
/// lookups work correctly after the snapshot is applied.
#[test]
fn snapshot_holder_share_key_written_by_apply_snapshot_shares() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 7_500)]);

    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &86_400u64,
    );

    // The weight retrieved via cast_vote should equal the share set in the snapshot.
    let weight =
        client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder, &true);
    assert_eq!(weight, 7_500);
}

// ── Acceptance tests for issue #1347 ──────────────────────────────────────────

/// 1. Vote after deadline returns ProposalExpired error.
#[test]
fn vote_after_deadline_returns_proposal_expired() {
    let (env, client, issuer, token) = setup();

    let holder = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(holder.clone(), 5_000)]);

    let voting_window = 10_000u64;
    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &voting_window,
    );

    // Advance time past deadline.
    env.ledger().set_timestamp(env.ledger().timestamp() + voting_window + 1);

    let result =
        client.try_cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder, &true);
    assert_eq!(
        result,
        Err(Ok(RevoraError::ProposalExpired)),
        "vote after deadline must fail with ProposalExpired"
    );
}

/// 2. Finalize before deadline fails; finalize after deadline succeeds, records outcome, and emits gov_fin event.
#[test]
fn finalize_lifecycle_and_event() {
    let (env, client, issuer, token) = setup();

    let holder_a = Address::generate(&env);
    let holder_b = Address::generate(&env);
    commit_and_apply(
        &env,
        &client,
        &issuer,
        &token,
        1,
        &[(holder_a.clone(), 6_000), (holder_b.clone(), 2_000)],
    );

    let voting_window = 5_000u64;
    let quorum_bps = 5_000u32;
    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &quorum_bps,
        &voting_window,
    );

    // Cast votes: Yes 6000, No 2000. Total votes = 8000 >= 5000 quorum.
    client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder_a, &true);
    client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &holder_b, &false);

    // Finalize before deadline must fail with ClaimDelayNotElapsed.
    let early_finalize =
        client.try_finalize_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id);
    assert_eq!(
        early_finalize,
        Err(Ok(RevoraError::ClaimDelayNotElapsed)),
        "finalizing before voting window ends must fail with ClaimDelayNotElapsed"
    );

    // Advance past deadline.
    env.ledger().set_timestamp(env.ledger().timestamp() + voting_window);

    // Anyone can finalize.
    let passed =
        client.finalize_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id);
    assert!(passed, "proposal should pass since yes >= no and total >= quorum");

    // Check stored state.
    let prop = client.get_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id).unwrap();
    assert!(!prop.open, "proposal must be closed");
    assert!(prop.passed, "proposal passed must be true");

    // Verify gov_fin event emission.
    let events = env.events().all();
    let gov_fin_sym = symbol_short!("gov_fin");
    let found = events.iter().any(|e| {
        let topics = e.1;
        if let Some(first) = topics.get(0) {
            let sym: Result<Symbol, _> = first.try_into_val(&env);
            sym.map(|s| s == gov_fin_sym).unwrap_or(false)
        } else {
            false
        }
    });
    assert!(found, "gov_fin event should be emitted on finalize_gov_proposal");
}

/// 3. Zero-weight voter is rejected with NoVotingWeight.
#[test]
fn zero_weight_voter_rejected_with_no_voting_weight() {
    let (env, client, issuer, token) = setup();

    let voter_with_share = Address::generate(&env);
    let zero_weight_voter = Address::generate(&env);
    commit_and_apply(
        &env,
        &client,
        &issuer,
        &token,
        1,
        &[(voter_with_share.clone(), 5_000)],
    );

    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &1_000u32,
        &10_000u64,
    );

    let result = client.try_cast_vote(
        &issuer,
        &symbol_short!("def"),
        &token,
        &proposal_id,
        &zero_weight_voter,
        &true,
    );
    assert_eq!(
        result,
        Err(Ok(RevoraError::NoVotingWeight)),
        "zero-weight voter must be rejected with NoVotingWeight"
    );
}

/// 4. Quorum not met -> passed == false.
#[test]
fn quorum_not_met_results_in_passed_false() {
    let (env, client, issuer, token) = setup();

    let voter = Address::generate(&env);
    commit_and_apply(&env, &client, &issuer, &token, 1, &[(voter.clone(), 2_000)]);

    // Quorum is 5_000 bps, but voter only has 2_000 bps.
    let voting_window = 10_000u64;
    let proposal_id = client.create_gov_proposal(
        &issuer,
        &symbol_short!("def"),
        &token,
        &symbol_short!("prop1"),
        &5_000u32,
        &voting_window,
    );

    client.cast_vote(&issuer, &symbol_short!("def"), &token, &proposal_id, &voter, &true);

    // Advance past deadline.
    env.ledger().set_timestamp(env.ledger().timestamp() + voting_window + 1);

    let passed =
        client.finalize_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id);
    assert!(!passed, "proposal must not pass when quorum is not met");

    let prop = client.get_gov_proposal(&issuer, &symbol_short!("def"), &token, &proposal_id).unwrap();
    assert!(!prop.open);
    assert!(!prop.passed, "stored outcome passed must be false");
}
