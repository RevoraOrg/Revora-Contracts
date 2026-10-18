//! Adversarial coverage for the `get_audit_summary` reader in `src/lib.rs` (#1120).
//!
//! `get_audit_summary` is the public read path of the per-offering audit cache
//! (`DataKey::AuditSummary`). It is permissionless, so its contract is narrow and
//! must be exact:
//!
//! * it returns `None` for every identity that has no persisted summary, and
//!   `Some` as soon as a report is durably written — including a report whose
//!   total is `0` (absent must never be confused with zero);
//! * the value is scoped to the full `OfferingId`
//!   `(issuer, namespace, token)`, including boundary namespace symbols
//!   (empty and longer than `symbol_short!` allows);
//! * reads are pure: no authorization entry consumed, no event emitted, no
//!   storage mutation, and identical bytes on every repetition;
//! * every rejected or unauthorized mutation path (bad `period_id`, negative
//!   amount, missing override target, below-threshold, frozen contract/offering,
//!   wrong caller, event-only mode) leaves the previously observed summary
//!   untouched and keeps it reconcilable against the authoritative period index.
//!
//! Test matrix (issue #1120):
//!
//! | Case | Expected outcome |
//! |------|------------------|
//! | registered offering, no reports | `None` (never a synthetic zero) |
//! | report of `0` | `Some { total_revenue: 0, report_count: 1 }` |
//! | uninitialized contract instance | `None`, no panic |
//! | wrong issuer / namespace / token | `None` for the untouched identity |
//! | empty `Symbol` and >9-char `Symbol` namespaces | independent summaries |
//! | repeated reads | identical value, no auth, no events |
//! | override up / down / to zero | exact `±delta`, `report_count` frozen |
//! | `period_id` 0, non-monotonic, `u64::MAX` | `InvalidPeriodId`, unchanged |
//! | negative amount (`-1`, `i128::MIN`) | `InvalidAmount`, unchanged |
//! | override on a missing period | `MissingReportForOverride`, unchanged |
//! | duplicate period without override | `Ok(())` but summary unchanged |
//! | below-threshold report | no summary materialised / unchanged |
//! | offering frozen / contract frozen | `OfferingFrozen`/`ContractFrozen`, readable |
//! | injected cache drift + `repair_audit_summary` | getter returns recomputed value |
//! | unauthorized caller (`repair`, `threshold`, `freeze`) | typed error, state unchanged |
//! | event-only mode | events emitted, nothing persisted |
//! | multi-period sequence | getter equals `get_revenue_range` over the index |

#![cfg(test)]

extern crate alloc;

use super::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, Symbol,
};

// ── helpers ─────────────────────────────────────────────────────────────────────

/// Deploy a fresh contract with one registered offering in namespace `def`.
///
/// `event_only` mirrors the third `initialize` argument so the event-only
/// (no-persistence) deployment mode can be exercised.
fn setup(event_only: bool) -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.initialize(&issuer, &None::<Address>, &Some(event_only));
    register(&client, &env, &issuer, &symbol_short!("def"), &token, &payout);
    (env, contract_id, issuer, token, payout)
}

fn register(
    client: &RevoraRevenueShareClient<'_>,
    env: &Env,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    payout: &Address,
) {
    client.register_offering(
        issuer,
        &Vec::new(env),
        &1u32,
        namespace,
        token,
        &1_000,
        payout,
        &0,
        &symbol_short!(""),
        &0,
    );
}

fn default_ns() -> Symbol {
    symbol_short!("def")
}

/// Read the getter as a plain tuple so assertions stay readable and diffable.
fn read(
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) -> Option<(i128, u64)> {
    client
        .get_audit_summary(issuer, namespace, token)
        .map(|summary| (summary.total_revenue, summary.report_count))
}

fn offering_id(issuer: &Address, namespace: &Symbol, token: &Address) -> OfferingId {
    OfferingId { issuer: issuer.clone(), namespace: namespace.clone(), token: token.clone() }
}

// ── tests ──────────────────────────────────────────────────────────────────────

/// Absent is not zero: a registered offering without reports must read `None`,
/// and a report of `0` must materialise `Some { 0, 1 }`.
#[test]
fn absent_summary_is_none_and_zero_valued_report_is_some() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();

    assert!(client.get_offering(&issuer, &ns, &token).is_some());
    assert_eq!(read(&client, &issuer, &ns, &token), None);

    client.report_revenue(&issuer, &ns, &token, &payout, &0, &1, &false);

    assert_eq!(read(&client, &issuer, &ns, &token), Some((0, 1)));
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 0);
}

/// `env` boundary: reading an instance that was never initialized (and hence has
/// no `DataKey::AuditSummary` at all) degrades to `None` instead of panicking.
#[test]
fn summary_is_none_on_an_uninitialized_contract_instance() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);

    assert!(RevoraRevenueShare::get_admin(env.clone()).is_none());
    assert_eq!(read(&client, &issuer, &default_ns(), &token), None);
}

/// `issuer` / `namespace` / `token` isolation: a report filed under one offering
/// identity must never be observable through any other combination.
#[test]
fn summary_is_scoped_to_the_full_offering_identity() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();
    let alt_ns = symbol_short!("alt");
    let other_token = Address::generate(&env);
    let other_issuer = Address::generate(&env);
    register(&client, &env, &issuer, &alt_ns, &other_token, &payout);

    client.report_revenue(&issuer, &ns, &token, &payout, &100, &1, &false);
    client.report_revenue(&issuer, &alt_ns, &other_token, &payout, &700, &1, &false);

    assert_eq!(read(&client, &issuer, &ns, &token), Some((100, 1)));
    assert_eq!(read(&client, &issuer, &alt_ns, &other_token), Some((700, 1)));
    // Same issuer + namespace, different token.
    assert_eq!(read(&client, &issuer, &ns, &other_token), None);
    // Same issuer + token, different (unregistered) namespace.
    assert_eq!(read(&client, &issuer, &symbol_short!("zzz"), &token), None);
    // Same namespace + token, different issuer.
    assert_eq!(read(&client, &other_issuer, &ns, &token), None);
    // Cross-tenant combination that was never registered by anybody.
    assert_eq!(read(&client, &other_issuer, &alt_ns, &other_token), None);
}

/// `namespace` boundary values: the empty `Symbol` and a `Symbol` longer than the
/// 9 characters `symbol_short!` allows must each own an independent summary and
/// must not alias a short symbol.
#[test]
fn empty_and_long_namespace_symbols_keep_independent_summaries() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let short_ns = default_ns();
    let empty_ns = symbol_short!("");
    let long_ns = Symbol::new(&env, "audit-long-ns");
    register(&client, &env, &issuer, &empty_ns, &token, &payout);
    register(&client, &env, &issuer, &long_ns, &token, &payout);

    client.report_revenue(&issuer, &long_ns, &token, &payout, &20, &1, &false);
    client.report_revenue(&issuer, &empty_ns, &token, &payout, &30, &1, &false);

    assert_eq!(read(&client, &issuer, &long_ns, &token), Some((20, 1)));
    assert_eq!(read(&client, &issuer, &empty_ns, &token), Some((30, 1)));
    // The `def` offering was never reported on.
    assert_eq!(read(&client, &issuer, &short_ns, &token), None);
    // A prefix of the registered long namespace is a different key.
    assert_eq!(read(&client, &issuer, &Symbol::new(&env, "audit-long"), &token), None);
}

/// Reads are pure: repeated calls are byte-identical, consume no authorization
/// entry, emit no event, and leave the stored/computed reconciliation intact.
#[test]
fn repeated_reads_are_deterministic_side_effect_free_and_unauthenticated() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();
    client.report_revenue(&issuer, &ns, &token, &payout, &250, &1, &false);
    client.report_revenue(&issuer, &ns, &token, &payout, &25, &2, &false);

    let auths_before = env.auths().len();
    let events_before = env.events().all().len();
    let mut observed = alloc::vec::Vec::new();
    for _ in 0..5 {
        observed.push(read(&client, &issuer, &ns, &token));
    }

    assert_eq!(observed, alloc::vec![Some((275, 2)); 5]);
    assert_eq!(env.auths().len(), auths_before, "getter must not require auth");
    assert_eq!(env.events().all().len(), events_before, "getter must not emit events");

    let reconciliation = client.reconcile_audit_summary(&issuer, &ns, &token);
    assert!(reconciliation.is_consistent);
    assert_eq!(reconciliation.stored_total_revenue, 275);
    assert_eq!(reconciliation.stored_report_count, 2);
    assert_eq!(reconciliation.computed_total_revenue, 275);
    assert_eq!(reconciliation.computed_report_count, 2);
}

/// Corrections move the total by the exact signed delta and never change
/// `report_count`, including a correction down to `0`.
#[test]
fn override_deltas_are_reflected_exactly_and_keep_the_count_frozen() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();
    client.report_revenue(&issuer, &ns, &token, &payout, &100, &1, &false);
    client.report_revenue(&issuer, &ns, &token, &payout, &60, &2, &false);
    client.report_revenue(&issuer, &ns, &token, &payout, &40, &3, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((200, 3)));

    // Correction upward: delta +50, count unchanged.
    client.report_revenue(&issuer, &ns, &token, &payout, &150, &1, &true);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((250, 3)));

    // Correction to zero: delta -150, count unchanged, entry stays present.
    client.report_revenue(&issuer, &ns, &token, &payout, &0, &1, &true);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((100, 3)));

    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 0);
    let reconciliation = client.reconcile_audit_summary(&issuer, &ns, &token);
    assert!(reconciliation.is_consistent);
    assert_eq!(reconciliation.computed_total_revenue, 100);
    assert_eq!(reconciliation.computed_report_count, 3);
}

/// Every rejected `report_revenue` path must leave the previously observed
/// summary bit-for-bit identical, and must not consume the period slot it
/// rejected — the period stays open for a later, valid report.
#[test]
fn rejected_reports_leave_the_summary_unchanged() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();
    client.report_revenue(&issuer, &ns, &token, &payout, &100, &1, &false);
    let baseline = read(&client, &issuer, &ns, &token);
    assert_eq!(baseline, Some((100, 1)));

    // `period_id` below the valid range.
    assert_eq!(
        client.try_report_revenue(&issuer, &ns, &token, &payout, &50, &0, &false),
        Err(Ok(RevoraError::InvalidPeriodId))
    );
    // Non-monotonic `period_id` (skips the next expected period).
    assert_eq!(
        client.try_report_revenue(&issuer, &ns, &token, &payout, &50, &5, &false),
        Err(Ok(RevoraError::InvalidPeriodId))
    );
    // Upper `u64` boundary is still rejected: it is not `last + 1`.
    assert_eq!(
        client.try_report_revenue(&issuer, &ns, &token, &payout, &50, &u64::MAX, &false),
        Err(Ok(RevoraError::InvalidPeriodId))
    );
    // Negative amounts, including the `i128` lower bound.
    assert_eq!(
        client.try_report_revenue(&issuer, &ns, &token, &payout, &-1, &2, &false),
        Err(Ok(RevoraError::InvalidAmount))
    );
    assert_eq!(
        client.try_report_revenue(&issuer, &ns, &token, &payout, &i128::MIN, &2, &false),
        Err(Ok(RevoraError::InvalidAmount))
    );
    // Override for a period that was never reported.
    assert_eq!(
        client.try_report_revenue(&issuer, &ns, &token, &payout, &50, &2, &true),
        Err(Ok(RevoraError::MissingReportForOverride))
    );
    // Duplicate period without override: succeeds, but is a semantic no-op.
    client.report_revenue(&issuer, &ns, &token, &payout, &250, &1, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), baseline);
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 100);

    // The rejected period 2 is still open.
    client.report_revenue(&issuer, &ns, &token, &payout, &20, &2, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((120, 2)));
    assert!(client.reconcile_audit_summary(&issuer, &ns, &token).is_consistent);
}

/// Threshold rejections are invisible to the reader: no summary is created when
/// none existed, and an existing summary is left untouched.
#[test]
fn below_threshold_reports_never_disturb_the_summary() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();
    client.set_min_revenue_threshold(&issuer, &ns, &token, &1_000);

    // No summary exists yet, and a below-threshold report must not create one.
    client.report_revenue(&issuer, &ns, &token, &payout, &999, &1, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), None);
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 0);

    // Exactly at the threshold is accepted (inclusive boundary).
    client.report_revenue(&issuer, &ns, &token, &payout, &1_000, &1, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((1_000, 1)));

    // Raise the threshold, then reject period 2: the summary must not move.
    client.set_min_revenue_threshold(&issuer, &ns, &token, &5_000);
    client.report_revenue(&issuer, &ns, &token, &payout, &4_999, &2, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((1_000, 1)));
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &2), 0);
    assert!(client.reconcile_audit_summary(&issuer, &ns, &token).is_consistent);
}

/// Freezing never hides or mutates the audit cache: reads stay available and
/// rejected reports leave the summary unchanged.
#[test]
fn frozen_offering_and_frozen_contract_keep_the_summary_readable() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();
    client.report_revenue(&issuer, &ns, &token, &payout, &100, &1, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((100, 1)));

    // Per-offering freeze.
    RevoraRevenueShare::freeze_offering(
        env.clone(),
        issuer.clone(),
        issuer.clone(),
        ns.clone(),
        token.clone(),
    )
    .unwrap();
    assert!(RevoraRevenueShare::is_offering_frozen(
        env.clone(),
        issuer.clone(),
        ns.clone(),
        token.clone()
    ));
    assert_eq!(
        client.try_report_revenue(&issuer, &ns, &token, &payout, &500, &2, &false),
        Err(Ok(RevoraError::OfferingFrozen))
    );
    assert_eq!(read(&client, &issuer, &ns, &token), Some((100, 1)));

    // Unfreezing restores reporting and the summary tracks the accepted report.
    RevoraRevenueShare::unfreeze_offering(
        env.clone(),
        issuer.clone(),
        issuer.clone(),
        ns.clone(),
        token.clone(),
    )
    .unwrap();
    assert!(!RevoraRevenueShare::is_offering_frozen(
        env.clone(),
        issuer.clone(),
        ns.clone(),
        token.clone()
    ));
    client.report_revenue(&issuer, &ns, &token, &payout, &500, &2, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((600, 2)));

    // Global freeze.
    RevoraRevenueShare::set_freeze(env.clone(), FreezeReason::Compliance).unwrap();
    assert_eq!(RevoraRevenueShare::get_freeze_reason(env.clone()), Some(FreezeReason::Compliance));
    assert_eq!(
        client.try_report_revenue(&issuer, &ns, &token, &payout, &7, &3, &false),
        Err(Ok(RevoraError::ContractFrozen))
    );
    assert_eq!(read(&client, &issuer, &ns, &token), Some((600, 2)));
}

/// A corrupted cache is observable through the reader, and
/// `repair_audit_summary` makes the reader report the recomputed value again.
#[test]
fn repair_audit_summary_rewrites_the_value_the_getter_returns() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();
    client.report_revenue(&issuer, &ns, &token, &payout, &100, &1, &false);
    client.report_revenue(&issuer, &ns, &token, &payout, &60, &2, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((160, 2)));

    // Inject drift straight into the cache to emulate a corrupted summary.
    env.storage().persistent().set(
        &DataKey::AuditSummary(offering_id(&issuer, &ns, &token)),
        &AuditSummary { total_revenue: 1, report_count: 99 },
    );
    assert_eq!(read(&client, &issuer, &ns, &token), Some((1, 99)));
    assert!(!client.reconcile_audit_summary(&issuer, &ns, &token).is_consistent);

    let repaired = client.repair_audit_summary(&issuer, &issuer, &ns, &token);
    assert_eq!(repaired, AuditSummary { total_revenue: 160, report_count: 2 });
    assert_eq!(read(&client, &issuer, &ns, &token), Some((160, 2)));
    assert!(client.reconcile_audit_summary(&issuer, &ns, &token).is_consistent);
}

/// `i128` upper boundary: the saturated total is reported deterministically and
/// stays sticky, while the report counter keeps advancing.
#[test]
fn saturated_total_is_observable_and_stays_sticky() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();
    client.report_revenue(&issuer, &ns, &token, &payout, &(i128::MAX - 5), &1, &false);
    client.report_revenue(&issuer, &ns, &token, &payout, &10, &2, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((i128::MAX, 2)));

    // Further reports cannot exceed the bound but still increment the count.
    client.report_revenue(&issuer, &ns, &token, &payout, &1, &3, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((i128::MAX, 3)));
    assert_eq!(read(&client, &issuer, &ns, &token), Some((i128::MAX, 3)));

    // The drift is visible through reconciliation, which flags saturation.
    let reconciliation = client.reconcile_audit_summary(&issuer, &ns, &token);
    assert!(reconciliation.is_saturated);
    assert!(!reconciliation.is_consistent);
    assert_eq!(reconciliation.stored_total_revenue, i128::MAX);
    assert_eq!(reconciliation.computed_report_count, 3);
}

/// The reader is intentionally permissionless, but every unauthorized mutation
/// attempt on the cache is rejected with a typed error and leaves state intact.
#[test]
fn unauthorized_callers_cannot_mutate_the_summary_while_reads_stay_open() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();
    let attacker = Address::generate(&env);
    client.report_revenue(&issuer, &ns, &token, &payout, &100, &1, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((100, 1)));

    // Contract-level identity checks reject the wrong caller.
    assert_eq!(
        client.try_repair_audit_summary(&attacker, &issuer, &ns, &token),
        Err(Ok(RevoraError::NotAuthorized))
    );
    assert_eq!(
        client.try_set_min_revenue_threshold(&attacker, &issuer, &ns, &token, &0),
        Err(Ok(RevoraError::OfferingNotFound))
    );
    assert_eq!(
        RevoraRevenueShare::freeze_offering(
            env.clone(),
            attacker.clone(),
            issuer.clone(),
            ns.clone(),
            token.clone(),
        ),
        Err(RevoraError::NotAuthorized)
    );

    // Nothing moved.
    assert_eq!(read(&client, &issuer, &ns, &token), Some((100, 1)));
    assert_eq!(client.get_min_revenue_threshold(&issuer, &ns, &token), 0);
    assert!(!RevoraRevenueShare::is_offering_frozen(
        env.clone(),
        issuer.clone(),
        ns.clone(),
        token.clone()
    ));

    // The read path stays open but reveals nothing outside the offering, and
    // reading never consumes an authorization entry.
    assert_eq!(read(&client, &attacker, &ns, &token), None);
    let auths_before = env.auths().len();
    assert_eq!(read(&client, &issuer, &ns, &token), Some((100, 1)));
    assert_eq!(env.auths().len(), auths_before);
}

/// Event-only deployment mode emits the report events but persists no summary,
/// so the reader must stay silent instead of fabricating a value.
#[test]
fn event_only_mode_never_persists_a_summary() {
    let (env, contract_id, issuer, token, payout) = setup(true);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();

    client.report_revenue(&issuer, &ns, &token, &payout, &100, &1, &false);
    assert_eq!(read(&client, &issuer, &ns, &token), None);

    let reconciliation = client.reconcile_audit_summary(&issuer, &ns, &token);
    assert!(reconciliation.is_consistent);
    assert_eq!(reconciliation.stored_total_revenue, 0);
    assert_eq!(reconciliation.stored_report_count, 0);

    // Even an unregistered identity is accepted in event-only mode and still
    // leaves no persisted summary behind.
    let unknown_token = Address::generate(&env);
    client.report_revenue(
        &issuer,
        &symbol_short!("none"),
        &unknown_token,
        &payout,
        &100,
        &1,
        &false,
    );
    assert_eq!(read(&client, &issuer, &symbol_short!("none"), &unknown_token), None);
}

/// The cached total must agree with the authoritative per-period index, both
/// value-wise and over a range query, and stay stable across re-reads.
#[test]
fn summary_matches_the_authoritative_period_index() {
    let (env, contract_id, issuer, token, payout) = setup(false);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let ns = default_ns();
    for (period_id, amount) in [(1u64, 100i128), (2, 250), (3, 375)] {
        client.report_revenue(&issuer, &ns, &token, &payout, &amount, &period_id, &false);
    }

    assert_eq!(read(&client, &issuer, &ns, &token), Some((725, 3)));
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &1), 100);
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &2), 250);
    assert_eq!(client.get_revenue_by_period(&issuer, &ns, &token, &3), 375);
    assert_eq!(client.get_revenue_range(&issuer, &ns, &token, &1, &3), 725);
    assert_eq!(read(&client, &issuer, &ns, &token), Some((725, 3)));
    assert!(client.reconcile_audit_summary(&issuer, &ns, &token).is_consistent);
}
