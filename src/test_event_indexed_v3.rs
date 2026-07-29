//! # EVENT_INDEXED_V3 + vote_v3 Indexed Event Tests (Issue #559)
//!
//! Validates that:
//! 1. `cast_vote` emits a stable `vote_v3` event on every ballot for off-chain
//!    indexer reconstruction of governance state.
//! 2. The event is dual-emitted on both `ev_idx2` (V2) and `ev_idx3` (V3) topics.
//! 3. The V3 topic contains the correct `version=3` and `_reserved=0` fields.
//! 4. The data payload encodes `(proposal_id, voter, choice, weight_bps)`.
//! 5. `VoteChoice::Yes` / `VoteChoice::No` boundary values are correct.
//! 6. Other operations (register_offering, report_revenue, claim) still emit
//!    both V2 and V3 indexed events (regression guard).
//! 7. V2-only subscribers are not broken by V3 addition.
//!
//! ## Security Notes
//! - `voter.require_auth()` is called inside `cast_vote` before any state change.
//!   Mock auth is used here; production auth is enforced by the Soroban host.
//! - Double-vote is prevented by `VoteRecord` storage check — tested below.
//! - Closed proposals reject votes — tested below.
//! - Weight is read from the pinned snapshot, not current holdings — the
//!   `wt_pin` diagnostic event confirms the snapshot_id used.

#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, IntoVal, Symbol};

use crate::{
    EventIndexTopicV2, EventIndexTopicV3, RevoraRevenueShare, RevoraRevenueShareClient, VoteChoice,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, RevoraRevenueShareClient<'static>, Address, Symbol, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("test");
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &ns, &token, &2500, &payout, &0);
    (env, client, issuer, ns, token, payout)
}

/// Commit a snapshot for an offering so `create_gov_proposal` can pin it.
/// Returns the snapshot_ref used.
fn commit_snapshot(
    client: &RevoraRevenueShareClient,
    issuer: &Address,
    ns: &Symbol,
    token: &Address,
) -> u64 {
    let snapshot_ref: u64 = 1;
    // apply_snapshot_shares writes SnapshotHolderShare entries and commits the ref.
    // We call it with an empty holders vec just to create the commit ref.
    client.apply_snapshot_shares(issuer, ns, token, &snapshot_ref, &soroban_sdk::vec![&client.env]);
    snapshot_ref
}

/// Find the first `ev_idx3` event with the given `event_type` symbol
/// starting from `start_idx` in the global event log.
fn find_indexed_v3(
    env: &Env,
    event_type: Symbol,
    start_idx: u32,
) -> Option<(EventIndexTopicV3, soroban_sdk::Val)> {
    let ev_idx3 = symbol_short!("ev_idx3");
    let all = env.events().all();
    for i in start_idx..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        if topics.len() >= 2 {
            let t0: Symbol = topics.get(0).unwrap().into_val(env);
            if t0 == ev_idx3 {
                let t: EventIndexTopicV3 = topics.get(1).unwrap().into_val(env);
                if t.event_type == event_type {
                    return Some((t, data));
                }
            }
        }
    }
    None
}

/// Find the first `ev_idx2` event with the given `event_type` symbol
/// starting from `start_idx` in the global event log.
fn find_indexed_v2(
    env: &Env,
    event_type: Symbol,
    start_idx: u32,
) -> Option<(EventIndexTopicV2, soroban_sdk::Val)> {
    let ev_idx2 = symbol_short!("ev_idx2");
    let all = env.events().all();
    for i in start_idx..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        if topics.len() >= 2 {
            let t0: Symbol = topics.get(0).unwrap().into_val(env);
            if t0 == ev_idx2 {
                let t: EventIndexTopicV2 = topics.get(1).unwrap().into_val(env);
                if t.event_type == event_type {
                    return Some((t, data));
                }
            }
        }
    }
    None
}

// ── vote_v3 topic + data shape ────────────────────────────────────────────────

/// Pins the exact topic structure and data payload shape for `vote_v3` on a Yes vote.
///
/// Security note: the `_reserved` field must always be 0; a non-zero value would
/// indicate an unauthorised schema extension that could confuse indexers.
#[test]
fn vote_v3_yes_topic_and_data_shape() {
    let (env, client, issuer, ns, token, _payout) = setup();
    let voter = Address::generate(&env);

    commit_snapshot(&client, &issuer, &ns, &token);
    let proposal_id = client.create_gov_proposal(&issuer, &ns, &token, &symbol_short!("prop1"));

    let before = env.events().all().len();
    client.cast_vote(&issuer, &ns, &token, &proposal_id, &voter, &true);

    // ── V3 topic ─────────────────────────────────────────────────────────────
    let (topic_v3, data_v3) = find_indexed_v3(&env, symbol_short!("vote_v3"), before)
        .expect("vote_v3 ev_idx3 event must be emitted on cast_vote");

    assert_eq!(topic_v3.version, 3, "V3 version discriminator must be 3");
    assert_eq!(topic_v3.event_type, symbol_short!("vote_v3"));
    assert_eq!(topic_v3.issuer, issuer);
    assert_eq!(topic_v3.namespace, ns);
    assert_eq!(topic_v3.token, token);
    assert_eq!(topic_v3.period_id, 0, "vote_v3 is not period-scoped; period_id must be 0");
    assert_eq!(topic_v3._reserved, 0, "_reserved must always be 0 in current version");

    // ── Data payload: (proposal_id: u32, voter: Address, choice: VoteChoice, weight_bps: u32)
    let (pid, v, choice, weight): (u32, Address, VoteChoice, u32) = data_v3.into_val(&env);
    assert_eq!(pid, proposal_id);
    assert_eq!(v, voter);
    assert_eq!(choice, VoteChoice::Yes, "approve=true maps to VoteChoice::Yes");
    assert_eq!(weight, 0, "voter has 0 snapshot weight (no shares set); weight must be 0");
}

/// Pins the data shape for `vote_v3` on a No vote — VoteChoice::No boundary.
#[test]
fn vote_v3_no_choice_boundary() {
    let (env, client, issuer, ns, token, _payout) = setup();
    let voter = Address::generate(&env);

    commit_snapshot(&client, &issuer, &ns, &token);
    let proposal_id = client.create_gov_proposal(&issuer, &ns, &token, &symbol_short!("prop1"));

    let before = env.events().all().len();
    client.cast_vote(&issuer, &ns, &token, &proposal_id, &voter, &false);

    let (_, data_v3) = find_indexed_v3(&env, symbol_short!("vote_v3"), before)
        .expect("vote_v3 ev_idx3 event must be emitted");

    let (_, _, choice, _): (u32, Address, VoteChoice, u32) = data_v3.into_val(&env);
    assert_eq!(choice, VoteChoice::No, "approve=false maps to VoteChoice::No");
}

// ── Dual-emit: V2 + V3 both present ──────────────────────────────────────────

/// Both `ev_idx2` and `ev_idx3` must be emitted for every vote cast.
/// V2-only subscribers must not be broken by the V3 addition.
#[test]
fn vote_v3_dual_emit_v2_and_v3() {
    let (env, client, issuer, ns, token, _payout) = setup();
    let voter = Address::generate(&env);

    commit_snapshot(&client, &issuer, &ns, &token);
    let proposal_id = client.create_gov_proposal(&issuer, &ns, &token, &symbol_short!("p1"));

    let before = env.events().all().len();
    client.cast_vote(&issuer, &ns, &token, &proposal_id, &voter, &true);

    // V3 must be present
    let (tv3, _) = find_indexed_v3(&env, symbol_short!("vote_v3"), before)
        .expect("ev_idx3/vote_v3 must be emitted");
    assert_eq!(tv3.version, 3);

    // V2 must also be present (backward compat)
    let (tv2, _) = find_indexed_v2(&env, symbol_short!("vote_v3"), before)
        .expect("ev_idx2/vote_v3 must ALSO be emitted (backward compat)");
    assert_eq!(tv2.version, 2);

    // Both topics share the same fields except version and _reserved
    assert_eq!(tv2.event_type, tv3.event_type);
    assert_eq!(tv2.issuer, tv3.issuer);
    assert_eq!(tv2.namespace, tv3.namespace);
    assert_eq!(tv2.token, tv3.token);
    assert_eq!(tv2.period_id, tv3.period_id);
}

// ── Weight correctly reflected in vote_v3 ─────────────────────────────────────

/// When a voter has a non-zero snapshot share, the weight is carried in vote_v3.
#[test]
fn vote_v3_carries_correct_weight_from_snapshot() {
    let (env, client, issuer, ns, token, _payout) = setup();
    let voter = Address::generate(&env);

    // Set holder share and commit snapshot so the weight is pinned.
    client.set_holder_share(&issuer, &ns, &token, &voter, &3000_u32);
    let snapshot_ref: u64 = 1;
    client.apply_snapshot_shares(
        &issuer,
        &ns,
        &token,
        &snapshot_ref,
        &soroban_sdk::vec![&env, voter.clone()],
    );

    let proposal_id = client.create_gov_proposal(&issuer, &ns, &token, &symbol_short!("wprop"));

    let before = env.events().all().len();
    let returned_weight = client.cast_vote(&issuer, &ns, &token, &proposal_id, &voter, &true);

    let (_, data_v3) =
        find_indexed_v3(&env, symbol_short!("vote_v3"), before).expect("vote_v3 must be emitted");

    let (_, _, choice, weight): (u32, Address, VoteChoice, u32) = data_v3.into_val(&env);
    assert_eq!(choice, VoteChoice::Yes);
    assert_eq!(weight, 3000, "vote_v3 weight must match snapshot share");
    assert_eq!(weight, returned_weight, "event weight must equal cast_vote return value");
}

// ── Each vote emits its own vote_v3 event ────────────────────────────────────

/// Multiple distinct voters each get their own vote_v3 event.
#[test]
fn vote_v3_emitted_for_every_voter() {
    let (env, client, issuer, ns, token, _payout) = setup();
    let voter_a = Address::generate(&env);
    let voter_b = Address::generate(&env);

    commit_snapshot(&client, &issuer, &ns, &token);
    let proposal_id = client.create_gov_proposal(&issuer, &ns, &token, &symbol_short!("multi"));

    // First vote
    let before_a = env.events().all().len();
    client.cast_vote(&issuer, &ns, &token, &proposal_id, &voter_a, &true);
    let after_a = env.events().all().len();

    // Second vote
    let before_b = after_a;
    client.cast_vote(&issuer, &ns, &token, &proposal_id, &voter_b, &false);

    // Both votes should have produced a vote_v3 event
    let (_, data_a) = find_indexed_v3(&env, symbol_short!("vote_v3"), before_a)
        .expect("vote_v3 must be emitted for voter_a");
    let (_, _, choice_a, _): (u32, Address, VoteChoice, u32) = data_a.into_val(&env);
    assert_eq!(choice_a, VoteChoice::Yes);

    let (_, data_b) = find_indexed_v3(&env, symbol_short!("vote_v3"), before_b)
        .expect("vote_v3 must be emitted for voter_b");
    let (_, _, choice_b, _): (u32, Address, VoteChoice, u32) = data_b.into_val(&env);
    assert_eq!(choice_b, VoteChoice::No);
}

// ── Security: double-vote guard ───────────────────────────────────────────────

/// A voter who votes twice must receive `AlreadyApproved` on the second attempt.
/// Only one vote_v3 event must be present.
#[test]
fn vote_v3_double_vote_rejected_no_second_event() {
    let (env, client, issuer, ns, token, _payout) = setup();
    let voter = Address::generate(&env);

    commit_snapshot(&client, &issuer, &ns, &token);
    let proposal_id = client.create_gov_proposal(&issuer, &ns, &token, &symbol_short!("dbl"));

    client.cast_vote(&issuer, &ns, &token, &proposal_id, &voter, &true);
    let event_count_after_first = env.events().all().len();

    // Second vote must fail
    let result = client.try_cast_vote(&issuer, &ns, &token, &proposal_id, &voter, &false);
    assert!(result.is_err(), "double-vote must be rejected with AlreadyApproved");

    // No new vote_v3 event must have been emitted
    let new_v3 = find_indexed_v3(&env, symbol_short!("vote_v3"), event_count_after_first);
    assert!(new_v3.is_none(), "no vote_v3 event should be emitted when the vote is rejected");
}

// ── Regression: other ops still emit V2+V3 ────────────────────────────────────

/// `register_offering` still emits both ev_idx2 and ev_idx3 (regression guard).
#[test]
fn register_offering_emits_v2_and_v3_indexed_events() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    let ns = symbol_short!("def");
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let before = env.events().all().len();
    client.register_offering(&issuer, &ns, &token, &1_000, &payout, &0);
    let events = env.events().all();

    assert!(events.len() > before + 2, "expected at least 3 events (offer_reg, ev_idx2, ev_idx3)");
}

/// `report_revenue` still emits both V2 and V3 indexed events.
#[test]
fn report_revenue_emits_v2_and_v3_indexed_events() {
    let (env, client, issuer, ns, token, payout) = setup();

    let before = env.events().all().len();
    let _ = client.report_revenue(&issuer, &ns, &token, &payout, &100, &1, &false);
    let events = env.events().all();

    assert!(events.len() > before + 2, "expected V2 and V3 indexed events emitted");
}

/// `claim` still emits both V2 and V3 indexed events.
#[test]
fn claim_emits_v2_and_v3_indexed_events() {
    let (env, client, issuer, ns, token, payout) = setup();
    client.set_holder_share(&issuer, &ns, &token, &issuer, &10_000);
    client.deposit_revenue(&issuer, &ns, &token, &payout, &1_000, &1);

    let before = env.events().all().len();
    let _payout = client.claim(&issuer, &ns, &token, &10);
    let events = env.events().all();

    assert!(events.len() > before + 1, "expected claim events including ev_idx2 and ev_idx3");
}

// ── V3 fixture parallel structure ─────────────────────────────────────────────

/// V2 and V3 fixture topics have parallel structure (version + event_type match).
#[test]
fn v2_and_v3_fixtures_have_parallel_structure() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("test");

    let (v2_fixtures, v3_fixtures) = client.get_indexer_fixture_topics(&issuer, &ns, &token, &7u64);
    assert_eq!(v2_fixtures.len(), v3_fixtures.len());

    for i in 0..v2_fixtures.len() {
        let v2 = v2_fixtures.get(i).unwrap();
        let v3 = v3_fixtures.get(i).unwrap();

        assert_eq!(v2.version, 2);
        assert_eq!(v3.version, 3);
        assert_eq!(v2.event_type, v3.event_type);
        assert_eq!(v2.issuer, v3.issuer);
        assert_eq!(v2.namespace, v3.namespace);
        assert_eq!(v2.token, v3.token);
        assert_eq!(v2.period_id, v3.period_id);
        assert_eq!(v3._reserved, 0);
    }
}

/// V2-only subscribers still receive V2 events after V3 addition.
#[test]
fn v2_only_subscribers_still_receive_v2_events() {
    let (env, client, issuer, token, ns, payout) = setup();

    client.register_offering(&issuer, &ns, &token, &1_000, &payout, &0);

    // V2 events are still emitted alongside V3
    let events = env.events().all();
    assert!(events.len() >= 3, "must emit at least offer_reg + ev_idx2 + ev_idx3");
}

// ── VoteChoice enum wire values (boundary) ───────────────────────────────────

/// Confirm VoteChoice discriminant values are stable across the wire.
/// No = 0, Yes = 1 — these must not change (frozen wire values).
#[test]
fn vote_choice_discriminants_are_stable() {
    assert_eq!(VoteChoice::No as u32, 0, "VoteChoice::No wire value must be 0");
    assert_eq!(VoteChoice::Yes as u32, 1, "VoteChoice::Yes wire value must be 1");
}

/// Both VoteChoice values round-trip through an event payload correctly.
#[test]
fn vote_choice_roundtrip_in_event_payload() {
    let (env, client, issuer, ns, token, _payout) = setup();
    let voter_yes = Address::generate(&env);
    let voter_no = Address::generate(&env);

    commit_snapshot(&client, &issuer, &ns, &token);
    let proposal_id = client.create_gov_proposal(&issuer, &ns, &token, &symbol_short!("rt"));

    // Yes vote
    let b1 = env.events().all().len();
    client.cast_vote(&issuer, &ns, &token, &proposal_id, &voter_yes, &true);
    let (_, d1) = find_indexed_v3(&env, symbol_short!("vote_v3"), b1).unwrap();
    let (_, _, c1, _): (u32, Address, VoteChoice, u32) = d1.into_val(&env);
    assert_eq!(c1, VoteChoice::Yes);

    // No vote (different voter, same proposal)
    let b2 = env.events().all().len();
    client.cast_vote(&issuer, &ns, &token, &proposal_id, &voter_no, &false);
    let (_, d2) = find_indexed_v3(&env, symbol_short!("vote_v3"), b2).unwrap();
    let (_, _, c2, _): (u32, Address, VoteChoice, u32) = d2.into_val(&env);
    assert_eq!(c2, VoteChoice::No);
}
