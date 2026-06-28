//! # Platform Fee Model Tests (Issue #468)
//!
//! Verifies the per-offering platform fee model:
//! - `set_offering_platform_fee` stores a `(fee_bps, treasury)` model, admin-only,
//!   and rejects configurations where `fee_bps + holder_aggregate_bps > 10_000`
//!   with `FeeExceedsHolderShare`.
//! - `report_revenue` deducts the configured fee and emits a `plat_fee` event with
//!   `(treasury, fee_bps, fee_amount, period_id)`.
//! - `fee_bps = 0` (and zero-revenue reports) skip the deduction and emit no `plat_fee` event.
//!
//! Off-chain indexers rely on `plat_fee` being present only when a real, non-zero fee
//! was routed to the treasury.

#![cfg(test)]

use crate::{PlatformFeeModel, RevoraError, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol,
};

// ── Helpers ─────────────────────────────────────────────────────────────────────

struct Ctx {
    env: Env,
    client: RevoraRevenueShareClient<'static>,
    admin: Address,
    issuer: Address,
    ns: Symbol,
    token: Address,
    payout: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("def");
    let token = Address::generate(&env);
    let payout = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &ns, &token, &2_500, &payout, &0);
    Ctx { env, client, admin, issuer, ns, token, payout }
}

/// Find the first `plat_fee` event at or after `start_idx` and decode its data tuple
/// `(treasury, fee_bps, fee_amount, period_id)`.
fn find_plat_fee(env: &Env, start_idx: u32) -> Option<(Address, u32, i128, u64)> {
    let plat_fee = symbol_short!("plat_fee");
    let all = env.events().all();
    for i in start_idx..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        if !topics.is_empty() {
            let t0: Symbol = topics.get(0).unwrap().into_val(env);
            if t0 == plat_fee {
                let decoded: (Address, u32, i128, u64) = data.into_val(env);
                return Some(decoded);
            }
        }
    }
    None
}

/// Count `plat_fee` events emitted at or after `start_idx`.
fn count_plat_fee(env: &Env, start_idx: u32) -> u32 {
    let plat_fee = symbol_short!("plat_fee");
    let all = env.events().all();
    let mut count = 0;
    for i in start_idx..all.len() {
        let (_, topics, _) = all.get(i).unwrap();
        if !topics.is_empty() {
            let t0: Symbol = topics.get(0).unwrap().into_val(env);
            if t0 == plat_fee {
                count += 1;
            }
        }
    }
    count
}

// ── Configuration ─────────────────────────────────────────────────────────────────

#[test]
fn set_offering_platform_fee_stores_model_and_getter_returns_it() {
    let c = setup();
    let treasury = Address::generate(&c.env);

    assert_eq!(c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token), None);

    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &500, &treasury);

    let model = c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token);
    assert_eq!(model, Some(PlatformFeeModel { fee_bps: 500, treasury: treasury.clone() }));
}

#[test]
fn set_offering_platform_fee_emits_config_event() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    let before = c.env.events().all().len();

    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &750, &treasury);

    let pfee_set = symbol_short!("pfee_set");
    let all = c.env.events().all();
    let mut found = false;
    for i in before..all.len() {
        let (_, topics, data) = all.get(i).unwrap();
        let t0: Symbol = topics.get(0).unwrap().into_val(&c.env);
        if t0 == pfee_set {
            let decoded: (u32, Address) = data.into_val(&c.env);
            assert_eq!(decoded, (750, treasury.clone()));
            found = true;
        }
    }
    assert!(found, "expected a pfee_set config event");
}

#[test]
fn set_offering_platform_fee_rejects_unknown_offering() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    let unknown_token = Address::generate(&c.env);

    let res =
        c.client.try_set_offering_platform_fee(&c.issuer, &c.ns, &unknown_token, &100, &treasury);
    assert_eq!(res, Err(Ok(RevoraError::OfferingNotFound)));
}

#[test]
fn set_offering_platform_fee_rejects_when_not_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let treasury = Address::generate(&env);
    let ns = symbol_short!("def");

    // No initialize() call: admin is unset.
    let res = client.try_set_offering_platform_fee(&issuer, &ns, &token, &100, &treasury);
    assert_eq!(res, Err(Ok(RevoraError::NotInitialized)));
}

// ── Fee + holder-share invariant ────────────────────────────────────────────────

#[test]
fn fee_plus_holder_aggregate_at_exactly_10000_is_allowed() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    let holder = Address::generate(&c.env);

    // Aggregate holder share = 7_000; fee 3_000 → sum exactly 10_000 bps.
    c.client.set_holder_share(&c.issuer, &c.ns, &c.token, &holder, &7_000);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &3_000, &treasury);

    let model = c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token);
    assert_eq!(model.unwrap().fee_bps, 3_000);
}

#[test]
fn fee_plus_holder_aggregate_over_10000_is_rejected() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    let holder = Address::generate(&c.env);

    // Aggregate holder share = 7_000; fee 3_001 → sum 10_001 bps → rejected.
    c.client.set_holder_share(&c.issuer, &c.ns, &c.token, &holder, &7_000);
    let res = c.client.try_set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &3_001, &treasury);
    assert_eq!(res, Err(Ok(RevoraError::FeeExceedsHolderShare)));

    // Nothing was persisted on the rejected path.
    assert_eq!(c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token), None);
}

#[test]
fn fee_alone_above_10000_is_rejected() {
    let c = setup();
    let treasury = Address::generate(&c.env);

    // No holder share set (aggregate = 0); fee 10_001 still exceeds the budget.
    let res =
        c.client.try_set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &10_001, &treasury);
    assert_eq!(res, Err(Ok(RevoraError::FeeExceedsHolderShare)));
}

#[test]
fn full_fee_with_no_holders_is_allowed() {
    let c = setup();
    let treasury = Address::generate(&c.env);

    // 100% platform fee with no holders configured is a valid edge.
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &10_000, &treasury);
    assert_eq!(
        c.client.get_offering_platform_fee(&c.issuer, &c.ns, &c.token).unwrap().fee_bps,
        10_000
    );
}

// ── Deduction on report_revenue ─────────────────────────────────────────────────

#[test]
fn report_revenue_emits_plat_fee_with_correct_amount() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &500, &treasury); // 5%

    let before = c.env.events().all().len();
    c.client.report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &10_000, &1, &false);

    let (t, fee_bps, fee_amount, period_id) =
        find_plat_fee(&c.env, before).expect("expected a plat_fee event");
    assert_eq!(t, treasury);
    assert_eq!(fee_bps, 500);
    assert_eq!(fee_amount, 500); // 10_000 * 500 / 10_000
    assert_eq!(period_id, 1);
}

#[test]
fn report_revenue_fee_rounds_down() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &333, &treasury); // 3.33%

    let before = c.env.events().all().len();
    c.client.report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &1_000, &1, &false);

    let (_, _, fee_amount, _) = find_plat_fee(&c.env, before).expect("expected a plat_fee event");
    // 1_000 * 333 / 10_000 = 33.3 → truncated to 33.
    assert_eq!(fee_amount, 33);
}

#[test]
fn report_revenue_with_zero_fee_bps_emits_no_plat_fee_event() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &0, &treasury);

    let before = c.env.events().all().len();
    c.client.report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &10_000, &1, &false);

    assert_eq!(count_plat_fee(&c.env, before), 0, "fee_bps=0 must emit no plat_fee event");
}

#[test]
fn report_revenue_without_fee_model_emits_no_plat_fee_event() {
    let c = setup();

    let before = c.env.events().all().len();
    c.client.report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &10_000, &1, &false);

    assert_eq!(count_plat_fee(&c.env, before), 0, "no fee model must emit no plat_fee event");
}

#[test]
fn report_revenue_zero_amount_emits_no_plat_fee_event() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &500, &treasury);

    let before = c.env.events().all().len();
    // Zero revenue is a valid report but yields a zero fee → no event.
    c.client.report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &0, &1, &false);

    assert_eq!(
        count_plat_fee(&c.env, before),
        0,
        "zero-revenue report must emit no plat_fee event"
    );
}

#[test]
fn report_revenue_tiny_amount_below_one_bps_unit_emits_no_event() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &1, &treasury); // 0.01%

    let before = c.env.events().all().len();
    // 100 * 1 / 10_000 = 0.01 → truncates to 0 → no event.
    c.client.report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &100, &1, &false);

    assert_eq!(count_plat_fee(&c.env, before), 0, "sub-unit fee must emit no plat_fee event");
}

#[test]
fn report_revenue_fee_applied_on_override_path() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &1_000, &treasury); // 10%

    // Initial report for period 1.
    c.client.report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &5_000, &1, &false);

    // Override period 1 with a larger amount; the fee tracks the new gross amount.
    let before = c.env.events().all().len();
    c.client.report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &8_000, &1, &true);

    let (t, fee_bps, fee_amount, period_id) =
        find_plat_fee(&c.env, before).expect("expected a plat_fee event on override");
    assert_eq!(t, treasury);
    assert_eq!(fee_bps, 1_000);
    assert_eq!(fee_amount, 800); // 8_000 * 1_000 / 10_000
    assert_eq!(period_id, 1);
}

#[test]
fn report_revenue_rejected_duplicate_emits_no_plat_fee_event() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &1_000, &treasury);

    // Record period 1 (this one takes a fee).
    c.client.report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &5_000, &1, &false);

    // A duplicate report with override_existing=false is rejected (no new record) and
    // must not take a fee.
    let before = c.env.events().all().len();
    c.client.report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &9_000, &1, &false);

    assert_eq!(count_plat_fee(&c.env, before), 0, "rejected duplicate must emit no plat_fee event");
}

#[test]
fn report_revenue_below_threshold_emits_no_plat_fee_event() {
    let c = setup();
    let treasury = Address::generate(&c.env);
    c.client.set_offering_platform_fee(&c.issuer, &c.ns, &c.token, &1_000, &treasury);
    // Require at least 1_000 revenue before a report is recorded.
    c.client.set_min_revenue_threshold(&c.issuer, &c.ns, &c.token, &1_000);

    let before = c.env.events().all().len();
    // Below threshold → no report recorded → no fee.
    c.client.report_revenue(&c.issuer, &c.ns, &c.token, &c.payout, &500, &1, &false);

    assert_eq!(
        count_plat_fee(&c.env, before),
        0,
        "below-threshold report must emit no plat_fee event"
    );
}
