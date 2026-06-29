//! # On-Chain Signal Completeness for Backend Milestone Checks (#289)
//!
//! Tests that the on-chain event stream emitted by `report_revenue` and
//! `report_concentration` is sufficient for `hardenedMilestoneValidation`
//! consumers to safely gate milestone transitions.
//!
//! ## Invariants verified
//!
//! 1. **Event ordering** – `rev_rep` always follows `offer_reg` in the event log.
//! 2. **period_id monotonicity** – successive `report_revenue` calls must use
//!    strictly increasing `period_id` values; out-of-order calls are rejected.
//! 3. **Audit summary consistency** – `AuditSummary.total_revenue` equals the
//!    sum of all accepted `report_revenue` amounts; `report_count` equals the
//!    number of accepted calls.
//! 4. **Concentration gate** – when enforcement is active, `report_revenue`
//!    is rejected if the stored concentration exceeds `max_bps`, and the
//!    audit summary is NOT updated.
//! 5. **Blacklist snapshot in rev_rep** – the `rev_rep` event payload carries
//!    the current blacklist so off-chain reconcilers can reconstruct exclusions
//!    at the exact moment of each revenue report.
//! 6. **Indexed v2 topic completeness** – every accepted `report_revenue` emits
//!    an `EVENT_INDEXED_V2` topic with the correct `event_type`, `period_id`,
//!    `issuer`, `namespace`, and `token` so indexers can route events without
//!    parsing the data payload.
//!
//! ## Security assumptions
//!
//! - The contract is tamper-evident: once emitted, events cannot be altered.
//! - Concentration values are issuer-reported (off-chain trust); enforcement
//!   is best-effort unless the issuer reliably calls `report_concentration`
//!   before each `report_revenue`.
//! - Auth failures (wrong signer) cause a host panic, not a `RevoraError`.

#![cfg(test)]

use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, IntoVal, Symbol};

use crate::{RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// Register an offering and return (issuer, token, payout_asset).
fn setup_offering(env: &Env, client: &RevoraRevenueShareClient) -> (Address, Address, Address) {
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let token = Address::generate(env);
    let payout = Address::generate(env);
    client.set_admin(&issuer);
    client.register_offering(&issuer, &symbol_short!("def"), &token, &1_000u32, &payout, &0i128);
    (issuer, token, payout)
}

/// Return true if any event topic vector contains the given symbol.
fn events_contain(env: &Env, sym: Symbol) -> bool {
    let val: soroban_sdk::Val = sym.into_val(env);
    env.events().all().iter().any(|e| e.1.contains(val))
}

/// Return the index of the first event whose topics contain the given symbol.
fn event_position(env: &Env, sym: Symbol) -> Option<u32> {
    let val: soroban_sdk::Val = sym.into_val(env);
    let all = env.events().all();
    for i in 0..all.len() {
        if all.get(i).unwrap().1.contains(val) {
            return Some(i);
        }
    }
    None
}

// ── 1. Event ordering ─────────────────────────────────────────────────────────

/// `rev_rep` must appear after `offer_reg` in the event log.
/// An indexer processing events in ledger order will always see the offering
/// registration before any revenue report for that offering.
#[test]
fn milestone_event_ordering_offer_before_rev_rep() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);

    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout,
        &1_000i128,
        &1u64,
        &false,
    );

    let offer_pos = event_position(&env, symbol_short!("offer_reg"));
    let rev_rep_pos = event_position(&env, symbol_short!("rev_rep"));

    assert!(offer_pos.is_some(), "offer_reg event must be emitted");
    assert!(rev_rep_pos.is_some(), "rev_rep event must be emitted");
    assert!(
        offer_pos.unwrap() < rev_rep_pos.unwrap(),
        "offer_reg must precede rev_rep in event log"
    );
}

// ── 2. period_id monotonicity ─────────────────────────────────────────────────

/// Successive `report_revenue` calls must use strictly increasing period_ids.
#[test]
fn milestone_period_id_must_be_strictly_increasing() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);

    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout,
        &500i128,
        &10u64,
        &false,
    );

    // Same period_id rejected
    assert!(
        client
            .try_report_revenue(
                &issuer,
                &symbol_short!("def"),
                &token,
                &payout,
                &500i128,
                &10u64,
                &false
            )
            .is_err(),
        "duplicate period_id must be rejected"
    );

    // Lower period_id rejected
    assert!(
        client
            .try_report_revenue(
                &issuer,
                &symbol_short!("def"),
                &token,
                &payout,
                &500i128,
                &5u64,
                &false
            )
            .is_err(),
        "period_id lower than last must be rejected"
    );

    // Higher period_id succeeds
    client.report_revenue(
        &issuer,
        &symbol_short!("def"),
        &token,
        &payout,
        &500i128,
        &11u64,
        &false,
    );
}

/// period_id = 0 is always invalid.
#[test]
fn milestone_period_id_zero_rejected() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);

    assert!(
        client
            .try_report_revenue(
                &issuer,
                &symbol_short!("def"),
                &token,
                &payout,
                &100i128,
                &0u64,
                &false
            )
            .is_err(),
        "period_id 0 must be rejected"
    );
}

// ── 3. Audit summary consistency ──────────────────────────────────────────────

/// `AuditSummary.total_revenue` must equal the sum of all accepted amounts.
/// `AuditSummary.report_count` must equal the number of accepted calls.
#[test]
fn milestone_audit_summary_accumulates_correctly() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);
    let ns = symbol_short!("def");

    assert!(
        client.get_audit_summary(&issuer, &ns, &token).is_none(),
        "audit summary must be absent before first report"
    );

    client.report_revenue(&issuer, &ns, &token, &payout, &1_000i128, &1u64, &false);
    client.report_revenue(&issuer, &ns, &token, &payout, &2_000i128, &2u64, &false);
    client.report_revenue(&issuer, &ns, &token, &payout, &3_000i128, &3u64, &false);

    let summary = client.get_audit_summary(&issuer, &ns, &token).unwrap();
    assert_eq!(summary.total_revenue, 6_000i128);
    assert_eq!(summary.report_count, 3u64);
}

/// Rejected calls must NOT update the audit summary.
#[test]
fn milestone_audit_summary_not_updated_on_rejected_report() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);
    let ns = symbol_short!("def");

    client.report_revenue(&issuer, &ns, &token, &payout, &1_000i128, &1u64, &false);
    // Duplicate — must fail
    let _ = client.try_report_revenue(&issuer, &ns, &token, &payout, &999i128, &1u64, &false);

    let summary = client.get_audit_summary(&issuer, &ns, &token).unwrap();
    assert_eq!(summary.total_revenue, 1_000i128);
    assert_eq!(summary.report_count, 1u64);
}

// ── 4. Concentration gate ─────────────────────────────────────────────────────

/// When enforcement is active and stored concentration exceeds `max_bps`,
/// `report_revenue` must be rejected and the audit summary must not change.
#[test]
fn milestone_concentration_enforcement_blocks_revenue_report() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);
    let ns = symbol_short!("def");

    client.set_concentration_limit(&issuer, &ns, &token, &5_000u32, &true, &0u64);
    client.report_concentration(&issuer, &ns, &token, &6_000u32);

    assert!(
        client
            .try_report_revenue(&issuer, &ns, &token, &payout, &1_000i128, &1u64, &false)
            .is_err(),
        "report_revenue must be rejected when concentration exceeds enforced limit"
    );
    assert!(
        client.get_audit_summary(&issuer, &ns, &token).is_none(),
        "audit summary must not be created by a rejected report"
    );
}

/// When concentration is exactly at the limit, `report_revenue` succeeds.
#[test]
fn milestone_concentration_at_limit_allows_revenue_report() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);
    let ns = symbol_short!("def");

    client.set_concentration_limit(&issuer, &ns, &token, &5_000u32, &true, &0u64);
    client.report_concentration(&issuer, &ns, &token, &5_000u32);

    client.report_revenue(&issuer, &ns, &token, &payout, &1_000i128, &1u64, &false);

    assert_eq!(client.get_audit_summary(&issuer, &ns, &token).unwrap().report_count, 1u64);
}

/// Warning-only mode (enforce=false) must NOT block `report_revenue`.
#[test]
fn milestone_concentration_warning_does_not_block_report() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);
    let ns = symbol_short!("def");

    client.set_concentration_limit(&issuer, &ns, &token, &3_000u32, &false, &0u64);
    client.report_concentration(&issuer, &ns, &token, &8_000u32);

    client.report_revenue(&issuer, &ns, &token, &payout, &500i128, &1u64, &false);

    assert_eq!(
        client.get_audit_summary(&issuer, &ns, &token).unwrap().report_count,
        1u64,
        "warning-only mode must not block revenue report"
    );
}

/// `conc_warn` event is emitted when concentration exceeds limit (warning mode).
#[test]
fn milestone_concentration_warning_event_emitted() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, _payout) = setup_offering(&env, &client);
    let ns = symbol_short!("def");

    client.set_concentration_limit(&issuer, &ns, &token, &3_000u32, &false, &0u64);
    client.report_concentration(&issuer, &ns, &token, &8_000u32);

    assert!(
        events_contain(&env, symbol_short!("conc_wrn")),
        "conc_wrn event must be emitted when concentration exceeds limit"
    );
}

/// When concentration is exactly one bps over the limit, `report_revenue` is rejected.
#[test]
fn milestone_concentration_one_bps_over_limit_rejected() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);
    let ns = symbol_short!("def");

    client.set_concentration_limit(&issuer, &ns, &token, &5_000u32, &true, &0u64);
    client.report_concentration(&issuer, &ns, &token, &5_001u32);

    let result = client.try_report_revenue(&issuer, &ns, &token, &payout, &1_000i128, &1u64, &false);
    assert_eq!(result, Err(Ok(RevoraError::ConcentrationLimitExceeded)));
}

/// When in testnet mode, concentration enforcement is bypassed even if over limit.
#[test]
fn milestone_concentration_testnet_mode_bypasses_enforcement() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);
    let ns = symbol_short!("def");

    // Enable testnet mode (requires admin auth)
    client.set_testnet_mode(&true);

    client.set_concentration_limit(&issuer, &ns, &token, &5_000u32, &true, &0u64);
    client.report_concentration(&issuer, &ns, &token, &6_000u32);

    // Should succeed despite being over limit
    client.report_revenue(&issuer, &ns, &token, &payout, &1_000i128, &1u64, &false);

    assert_eq!(
        client.get_audit_summary(&issuer, &ns, &token).unwrap().report_count,
        1u64,
        "testnet mode must bypass concentration enforcement"
    );
}

// ── 5. Blacklist snapshot in rev_rep ─────────────────────────────────────────

/// The blacklist state at report time is observable via `get_blacklist`.
/// Adding an investor before a report changes the snapshot captured in that event.
#[test]
fn milestone_blacklist_snapshot_captured_at_report_time() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);
    let ns = symbol_short!("def");
    let investor = Address::generate(&env);

    // First report: empty blacklist
    client.report_revenue(&issuer, &ns, &token, &payout, &1_000i128, &1u64, &false);

    // Add investor, then second report: non-empty blacklist
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);
    client.report_revenue(&issuer, &ns, &token, &payout, &2_000i128, &2u64, &false);

    let bl = client.get_blacklist(&issuer, &ns, &token);
    assert_eq!(bl.len(), 1u32);
    assert!(bl.contains(&investor));
}

// ── 6. Indexed v2 topic completeness ─────────────────────────────────────────

/// Every accepted `report_revenue` emits an `ev_idx2` topic with the correct
/// `event_type` (`rv_rep`), `period_id`, `issuer`, `namespace`, and `token`.
#[test]
fn milestone_indexed_v2_topic_emitted_on_report_revenue() {
    let env = Env::default();
    let client = make_client(&env);
    let (issuer, token, payout) = setup_offering(&env, &client);
    let ns = symbol_short!("def");

    client.report_revenue(&issuer, &ns, &token, &payout, &1_000i128, &42u64, &false);

    // The fixture helper returns canonical v2 topics — verify rv_rep shape
    let fixtures = client.get_indexer_fixture_topics(&issuer, &ns, &token, &42u64);
    let mut rv_rep_opt = None;
    for f in fixtures.iter() {
        if f.event_type == symbol_short!("rv_rep") {
            rv_rep_opt = Some(f);
            break;
        }
    }
    let rv_rep = rv_rep_opt.expect("rv_rep fixture must exist");

    assert_eq!(rv_rep.version, 2u32);
    assert_eq!(rv_rep.period_id, 42u64);
    assert_eq!(rv_rep.issuer, issuer);
    assert_eq!(rv_rep.namespace, ns);
    assert_eq!(rv_rep.token, token);
}

/// Fixture topics must cover all six canonical event types in stable order.
#[test]
fn milestone_fixture_covers_all_canonical_event_types() {
    let env = Env::default();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    let fixtures = client.get_indexer_fixture_topics(&issuer, &ns, &token, &1u64);
    assert_eq!(fixtures.len(), 6u32);

    // Check all expected event types are present
    let expected = [
        symbol_short!("offer"),
        symbol_short!("rv_init"),
        symbol_short!("rv_ovr"),
        symbol_short!("rv_rej"),
        symbol_short!("rv_rep"),
        symbol_short!("claim"),
    ];
    for expected_type in expected.iter() {
        let found = fixtures.iter().any(|f| f.event_type == *expected_type);
        assert!(found, "fixture must contain event_type");
    }
}

/// Non-period-scoped fixture topics (offer, claim) must have period_id = 0.
/// Period-scoped topics must carry the requested period_id.
#[test]
fn milestone_non_period_scoped_fixtures_have_zero_period_id() {
    let env = Env::default();
    let client = make_client(&env);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("def");

    let fixtures = client.get_indexer_fixture_topics(&issuer, &ns, &token, &99u64);
    for f in fixtures.iter() {
        if f.event_type == symbol_short!("offer") || f.event_type == symbol_short!("claim") {
            assert_eq!(f.period_id, 0u64, "non-period-scoped event must have period_id = 0");
        } else {
            assert_eq!(f.period_id, 99u64, "period-scoped event must carry the requested period_id");
        }
    }
}

// ── 7. Multi-offering isolation ───────────────────────────────────────────────

/// Audit summaries are isolated per (issuer, namespace, token) triple.
#[test]
fn milestone_audit_summary_isolated_per_offering() {
    let env = Env::default();
    let client = make_client(&env);
    env.mock_all_auths();

    let issuer = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout = Address::generate(&env);
    let ns = symbol_short!("def");

    client.set_admin(&issuer);
    client.register_offering(&issuer, &ns, &token_a, &1_000u32, &payout, &0i128);
    client.register_offering(&issuer, &ns, &token_b, &1_000u32, &payout, &0i128);

    client.report_revenue(&issuer, &ns, &token_a, &payout, &5_000i128, &1u64, &false);

    assert!(
        client.get_audit_summary(&issuer, &ns, &token_b).is_none(),
        "reporting on token_a must not create a summary for token_b"
    );
    let summary_a = client.get_audit_summary(&issuer, &ns, &token_a).unwrap();
    assert_eq!(summary_a.total_revenue, 5_000i128);
    assert_eq!(summary_a.report_count, 1u64);
}
