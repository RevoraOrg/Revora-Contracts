//! Adversarial coverage for `get_revenue_by_period` (issue #1086).
//!
//! `RevoraRevenueShare::get_revenue_by_period(env, issuer, namespace, token, period_id)`
//! is a read-only selector over `DataKey::RevenueIndex(OfferingId, period_id)` that
//! returns `0` for any key that has never been written. It has no `require_auth`
//! guard, and it is the primitive that `get_revenue_range` /
//! `get_revenue_range_chunk` aggregate over, so a silently wrong read here leaks
//! into every downstream revenue report.
//!
//! This module pins the selector's observable contract:
//!
//! * **Sentinel / boundary `period_id`** — `0`, `1`, `u64::MAX - 1`, `u64::MAX` and
//!   sparse large ids never panic and read as `0` while unreported.
//! * **Key-component isolation** — every one of `issuer`, `namespace` and `token`
//!   participates in the lookup; a near-miss key never leaks another offering's value.
//! * **Per-period isolation** — period `N` never leaks into period `N ± 1`.
//! * **Report semantics** — a plain report writes the amount, `override_existing = true`
//!   replaces (does not accumulate), and a duplicate without override is a no-op.
//! * **Zero vs. absent** — a persisted `0` report is indistinguishable from an unset
//!   key through this selector alone; the audit summary is the disambiguator.
//! * **Rejected operations** — unauthorized, zero-period, negative-amount,
//!   missing-override, below-threshold, frozen-contract and failed-deposit paths all
//!   leave both the period index and the audit summary byte-for-byte unchanged.
//! * **Permissionless reads** — reads succeed with every authorization revoked.
//!
//! Assertions are deliberately written against the current source semantics
//! (`src/lib.rs::get_revenue_by_period` reads the report index only; the deposit
//! ledger lives at `DataKey::PeriodRevenue`).

#![cfg(test)]

use super::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, Vec};

const SHARE_BPS: u32 = 1_000;

/// Register `(issuer, namespace, token)` as an offering bound to `env`/`client`.
fn register_offering(
    env: &Env,
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
) {
    client
        .register_offering(
            issuer,
            &Vec::new(env),
            &1u32,
            namespace,
            token,
            &SHARE_BPS,
            token,
            &0i128,
            &symbol_short!(""),
            &0u32,
        )
        .unwrap();
}

/// Report `amount` for `period_id` without overriding, asserting success.
fn report(
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    amount: i128,
    period_id: u64,
) {
    client
        .report_revenue(issuer, namespace, token, token, &amount, &period_id, &false)
        .unwrap();
}

fn period(
    client: &RevoraRevenueShareClient<'_>,
    issuer: &Address,
    namespace: &Symbol,
    token: &Address,
    period_id: u64,
) -> i128 {
    client.get_revenue_by_period(issuer, namespace, token, &period_id)
}

// ── Sentinel and boundary period ids ─────────────────────────────────────────

/// An offering with no reports reads `0` for every boundary `period_id`,
/// including the reserved sentinel `0` and the `u64` extremes.
#[test]
fn unreported_periods_read_zero_at_every_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    for boundary in [0u64, 1u64, 2u64, u64::MAX - 1, u64::MAX] {
        assert_eq!(
            period(&client, &issuer, &namespace, &token, boundary),
            0,
            "unreported period {} must read as 0",
            boundary
        );
    }

    // Sparse id that can never collide with a sequential period counter.
    assert_eq!(period(&client, &issuer, &namespace, &token, 1u64 << 40), 0);
}

/// Every component of the composite key participates in the lookup: mutating
/// any single component reads `0` even when the offering itself is registered.
#[test]
fn key_components_are_not_interchangeable() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("nsA");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    let other_issuer = Address::generate(&env);
    let other_namespace = symbol_short!("nsB");
    let other_token = Address::generate(&env);

    // Exact key, still unreported.
    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 0);
    // One component swapped at a time.
    assert_eq!(period(&client, &other_issuer, &namespace, &token, 1), 0);
    assert_eq!(period(&client, &issuer, &other_namespace, &token, 1), 0);
    assert_eq!(period(&client, &issuer, &namespace, &other_token, 1), 0);
    // Nothing registered at all.
    assert_eq!(period(&client, &other_issuer, &other_namespace, &other_token, 1), 0);
}

// ── Report round-trip, override semantics and the zero/absent ambiguity ──────

/// Reported amounts round-trip per period and never bleed across periods.
#[test]
fn reported_amounts_round_trip_and_are_period_isolated() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    report(&client, &issuer, &namespace, &token, 100, 1);
    report(&client, &issuer, &namespace, &token, 0, 2);
    report(&client, &issuer, &namespace, &token, 4_200, 3);
    report(&client, &issuer, &namespace, &token, i128::MAX, 4);

    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 100);
    // A persisted zero and an unset key are both `0` through this selector.
    assert_eq!(period(&client, &issuer, &namespace, &token, 2), 0);
    assert_eq!(period(&client, &issuer, &namespace, &token, 3), 4_200);
    // Saturation boundary: the selector returns the stored `i128` verbatim.
    assert_eq!(period(&client, &issuer, &namespace, &token, 4), i128::MAX);
    // Period 5 was never reported.
    assert_eq!(period(&client, &issuer, &namespace, &token, 5), 0);
}

/// `override_existing = true` replaces the stored amount instead of accumulating.
#[test]
fn override_replaces_instead_of_accumulating() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    report(&client, &issuer, &namespace, &token, 500, 1);
    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 500);

    client
        .report_revenue(&issuer, &namespace, &token, &token, &800, &1, &true)
        .unwrap();
    assert_eq!(
        period(&client, &issuer, &namespace, &token, 1),
        800,
        "override must replace, not accumulate (500 + 800 = 1300 would be wrong)"
    );

    client
        .report_revenue(&issuer, &namespace, &token, &token, &50, &1, &true)
        .unwrap();
    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 50);

    let summary = client.get_audit_summary(&issuer, &namespace, &token).unwrap();
    assert_eq!(summary.total_revenue, 50);
    assert_eq!(summary.report_count, 1);
}

/// A duplicate report without override is accepted but must not change the index.
#[test]
fn duplicate_report_without_override_is_a_no_op() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    report(&client, &issuer, &namespace, &token, 100, 1);
    report(&client, &issuer, &namespace, &token, 250, 1);

    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 100);
    let summary = client.get_audit_summary(&issuer, &namespace, &token).unwrap();
    assert_eq!(summary.total_revenue, 100);
    assert_eq!(summary.report_count, 1);
}

/// A zero-amount report is persisted (and counted) even though the selector
/// cannot distinguish it from an unset key.
#[test]
fn zero_amount_report_is_persisted_but_reads_as_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    report(&client, &issuer, &namespace, &token, 0, 1);

    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 0);
    // The audit summary is the only on-chain proof the period was reported.
    let summary = client.get_audit_summary(&issuer, &namespace, &token).unwrap();
    assert_eq!(summary.total_revenue, 0);
    assert_eq!(summary.report_count, 1);
    // The zero report occupies the sequential slot, so period 2 is next.
    assert_eq!(period(&client, &issuer, &namespace, &token, 2), 0);
}

// ── Rejected operations leave the index untouched ────────────────────────────

/// A non-owner address cannot report, even with every authorization mocked:
/// the offering lookup fails and no index entry is created.
#[test]
fn report_by_non_owner_is_rejected_without_writing_index() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    let attacker = Address::generate(&env);
    let result = client.try_report_revenue(
        &attacker,
        &namespace,
        &token,
        &token,
        &999i128,
        &1u64,
        &false,
    );
    assert_eq!(result, Err(Ok(RevoraError::OfferingNotFound)));

    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 0);
    assert!(client.get_audit_summary(&issuer, &namespace, &token).is_none());
}

/// With all authorizations revoked the write path fails and the index is untouched.
#[test]
fn unauthenticated_report_is_rejected_without_writing_index() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);
    report(&client, &issuer, &namespace, &token, 100, 1);

    // Drop every mocked authorization for the remainder of the test.
    env.mock_auths(&[]);

    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &token,
        &777i128,
        &2u64,
        &false,
    );
    assert!(result.is_err(), "report without authorization must be rejected");

    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 100);
    assert_eq!(period(&client, &issuer, &namespace, &token, 2), 0);
}

/// `period_id = 0` is the reserved sentinel and must be rejected before any write.
#[test]
fn zero_period_id_is_rejected_without_writing_index() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &token,
        &100i128,
        &0u64,
        &false,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidPeriodId)));

    assert_eq!(period(&client, &issuer, &namespace, &token, 0), 0);
    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 0);
    assert!(client.get_audit_summary(&issuer, &namespace, &token).is_none());
}

/// A negative amount is rejected by the amount-validation matrix with no write.
#[test]
fn negative_amount_is_rejected_without_writing_index() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &token,
        &-1i128,
        &1u64,
        &false,
    );
    assert_eq!(result, Err(Ok(RevoraError::InvalidAmount)));

    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 0);
    assert!(client.get_audit_summary(&issuer, &namespace, &token).is_none());
}

/// Overriding a period that was never reported fails and writes nothing.
#[test]
fn override_of_missing_period_is_rejected_without_writing_index() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &token,
        &100i128,
        &1u64,
        &true,
    );
    assert_eq!(result, Err(Ok(RevoraError::MissingReportForOverride)));

    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 0);
    assert!(client.get_audit_summary(&issuer, &namespace, &token).is_none());
}

/// A report below the configured minimum threshold is silently skipped: it does
/// not write the index and does not consume the sequential period slot, so the
/// same `period_id` is still reportable once the amount clears the threshold.
#[test]
fn below_threshold_report_is_skipped_and_period_stays_available() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    client
        .set_min_revenue_threshold(&issuer, &namespace, &token, &1_000i128)
        .unwrap();

    // 999 < 1000 → skipped, but the call still reports success.
    report(&client, &issuer, &namespace, &token, 999, 1);
    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 0);
    assert!(client.get_audit_summary(&issuer, &namespace, &token).is_none());

    // Exactly at the threshold is accepted, and period 1 was not consumed.
    report(&client, &issuer, &namespace, &token, 1_000, 1);
    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 1_000);
    let summary = client.get_audit_summary(&issuer, &namespace, &token).unwrap();
    assert_eq!(summary.total_revenue, 1_000);
    assert_eq!(summary.report_count, 1);
}

/// A failed deposit (token transfer reverts) writes neither the deposit ledger
/// nor the report index read by `get_revenue_by_period`.
#[test]
fn failed_deposit_leaves_report_index_untouched() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    // A non-contract payment token makes the transfer revert deterministically.
    let unusable_payment_token = Address::generate(&env);
    let result = client.try_deposit_revenue(
        &issuer,
        &namespace,
        &token,
        &unusable_payment_token,
        &1_000i128,
        &1u64,
    );
    assert!(result.is_err(), "deposit with a non-contract payment token must fail");

    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 0);
    assert_eq!(client.get_period_count(&issuer, &namespace, &token), 0);
}

/// A frozen contract rejects reports outright and preserves the index.
#[test]
fn frozen_contract_rejects_report_and_preserves_index() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);
    report(&client, &issuer, &namespace, &token, 100, 1);

    client.freeze().unwrap();

    let result = client.try_report_revenue(
        &issuer,
        &namespace,
        &token,
        &token,
        &500i128,
        &2u64,
        &false,
    );
    assert_eq!(result, Err(Ok(RevoraError::ContractFrozen)));

    // Reads remain available while frozen, and nothing was mutated.
    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 100);
    assert_eq!(period(&client, &issuer, &namespace, &token, 2), 0);
    let summary = client.get_audit_summary(&issuer, &namespace, &token).unwrap();
    assert_eq!(summary.total_revenue, 100);
    assert_eq!(summary.report_count, 1);
}

// ── Compound invariants ─────────────────────────────────────────────────────

/// A sequence of rejected/duplicate operations must not perturb an established
/// index, audit summary or range aggregate.
#[test]
fn rejected_operation_sequence_leaves_prior_state_intact() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    report(&client, &issuer, &namespace, &token, 100, 1);

    let attacker = Address::generate(&env);
    // 1. non-owner
    assert!(client
        .try_report_revenue(&attacker, &namespace, &token, &token, &1i128, &2u64, &false)
        .is_err());
    // 2. reserved period id
    assert_eq!(
        client.try_report_revenue(&issuer, &namespace, &token, &token, &1i128, &0u64, &false),
        Err(Ok(RevoraError::InvalidPeriodId))
    );
    // 3. negative amount
    assert_eq!(
        client.try_report_revenue(&issuer, &namespace, &token, &token, &-1i128, &2u64, &false),
        Err(Ok(RevoraError::InvalidAmount))
    );
    // 4. override without an existing report
    assert_eq!(
        client.try_report_revenue(&issuer, &namespace, &token, &token, &5i128, &2u64, &true),
        Err(Ok(RevoraError::MissingReportForOverride))
    );
    // 5. duplicate without override (accepted, but a no-op)
    report(&client, &issuer, &namespace, &token, 777, 1);

    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 100);
    assert_eq!(period(&client, &issuer, &namespace, &token, 2), 0);
    assert_eq!(client.get_revenue_range(&issuer, &namespace, &token, &1u64, &2u64), 100);

    let summary = client.get_audit_summary(&issuer, &namespace, &token).unwrap();
    assert_eq!(summary.total_revenue, 100);
    assert_eq!(summary.report_count, 1);
}

/// The same `period_id` under different `(namespace, token)` pairs is fully
/// independent; cross queries never observe another offering's value.
#[test]
fn namespace_and_token_scopes_are_independent() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace_a = symbol_short!("nsA");
    let namespace_b = symbol_short!("nsB");
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    register_offering(&env, &client, &issuer, &namespace_a, &token_a);
    register_offering(&env, &client, &issuer, &namespace_a, &token_b);
    register_offering(&env, &client, &issuer, &namespace_b, &token_a);

    report(&client, &issuer, &namespace_a, &token_a, 100, 1);
    report(&client, &issuer, &namespace_a, &token_b, 200, 1);
    report(&client, &issuer, &namespace_b, &token_a, 300, 1);

    assert_eq!(period(&client, &issuer, &namespace_a, &token_a, 1), 100);
    assert_eq!(period(&client, &issuer, &namespace_a, &token_b, 1), 200);
    assert_eq!(period(&client, &issuer, &namespace_b, &token_a, 1), 300);
    // A scope that was never reported stays empty.
    assert_eq!(period(&client, &issuer, &namespace_b, &token_b, 1), 0);
}

/// `get_revenue_by_period` is the atom behind the range helpers: the unbounded
/// range, the chunked range and the per-period reads must agree exactly.
#[test]
fn index_agrees_with_unbounded_and_chunked_ranges() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);

    report(&client, &issuer, &namespace, &token, 100, 1);
    report(&client, &issuer, &namespace, &token, 200, 2);
    report(&client, &issuer, &namespace, &token, 300, 3);

    let per_period_sum = period(&client, &issuer, &namespace, &token, 1)
        + period(&client, &issuer, &namespace, &token, 2)
        + period(&client, &issuer, &namespace, &token, 3);

    assert_eq!(per_period_sum, 600);
    assert_eq!(client.get_revenue_range(&issuer, &namespace, &token, &1u64, &3u64), 600);
    assert_eq!(client.get_revenue_range(&issuer, &namespace, &token, &1u64, &1u64), 100);
    // Periods beyond the reported window contribute nothing.
    assert_eq!(client.get_revenue_range(&issuer, &namespace, &token, &4u64, &10u64), 0);

    // Chunked iteration: first page stops at the cap and hands back a cursor.
    let (first_sum, first_next) =
        client.get_revenue_range_chunk(&issuer, &namespace, &token, &1u64, &3u64, &2u32);
    assert_eq!(first_sum, 300);
    assert_eq!(first_next, Some(3u64));

    let (second_sum, second_next) = client.get_revenue_range_chunk(
        &issuer,
        &namespace,
        &token,
        &first_next.unwrap(),
        &3u64,
        &2u32,
    );
    assert_eq!(second_sum, 300);
    assert_eq!(second_next, None);
    assert_eq!(first_sum + second_sum, per_period_sum);
}

/// Inverted ranges are empty and read no periods at all.
#[test]
fn inverted_ranges_read_nothing() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);
    report(&client, &issuer, &namespace, &token, 100, 1);

    assert_eq!(client.get_revenue_range(&issuer, &namespace, &token, &3u64, &1u64), 0);

    let (sum, next) =
        client.get_revenue_range_chunk(&issuer, &namespace, &token, &3u64, &1u64, &10u32);
    assert_eq!(sum, 0);
    assert_eq!(next, None);
}

/// Reads are permissionless: with every authorization revoked, all revenue
/// selectors still succeed.
#[test]
fn reads_succeed_without_any_authorization() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &id);

    let issuer = Address::generate(&env);
    let namespace = symbol_short!("ns");
    let token = Address::generate(&env);
    register_offering(&env, &client, &issuer, &namespace, &token);
    report(&client, &issuer, &namespace, &token, 100, 1);

    env.mock_auths(&[]);

    assert_eq!(period(&client, &issuer, &namespace, &token, 1), 100);
    assert_eq!(client.get_revenue_range(&issuer, &namespace, &token, &1u64, &1u64), 100);
    assert_eq!(
        client.get_revenue_range_chunk(&issuer, &namespace, &token, &1u64, &1u64, &5u32),
        (100, None)
    );
}
