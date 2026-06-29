//! # EVENT_INDEXED_V2 Topic Stability Tests (Issue #412)
//!
//! Pins the exact topic structure and data payload shape for each indexed event type.
//! Off-chain indexers rely on these exact fields; any schema change is a breaking change.
//!
//! ## Coverage
//! - **rv_init**: Initial revenue report for a new period
//! - **rv_ovr**: Revenue report override (correction) for existing period
//! - **rv_rej**: Rejected duplicate report attempt (override_existing=false)
//! - **rv_rep**: Unconditional report receipt (always emitted)
//! - **claim**: Holder claim event (period_id=0, not period-scoped)
//!
//! ## Assertions
//! - Topic tuple order: `(EVENT_INDEXED_V2, EventIndexTopicV2)`
//! - `EventIndexTopicV2` fields: `{version, event_type, issuer, namespace, token, period_id}`
//! - Data tuple arity and types per event_type (locked shape)

#![cfg(test)]

use crate::{EventIndexTopicV2, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol,
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

/// Find the first `EVENT_INDEXED_V2` event with the given `event_type` symbol
/// starting from `start_idx` in the global event log.
/// Returns `(topic, data_val)`.
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

// ── rv_init ───────────────────────────────────────────────────────────────────

/// Pins the topic structure and data shape for `rv_init` (initial revenue report).
#[test]
fn event_indexed_v2_rv_init_topic_and_data_shape() {
    let (env, client, issuer, ns, token, payout) = setup();
    let before = env.events().all().len();
    client.report_revenue(&issuer, &ns, &token, &payout, &10_000, &1, &false);

    let (topic, data) = find_indexed_v2(&env, symbol_short!("rv_init"), before as u32)
        .expect("rv_init EVENT_INDEXED_V2 must be emitted on initial report");

    // Topic shape
    assert_eq!(topic.version, 2);
    assert_eq!(topic.event_type, symbol_short!("rv_init"));
    assert_eq!(topic.issuer, issuer);
    assert_eq!(topic.namespace, ns);
    assert_eq!(topic.token, token);
    assert_eq!(topic.period_id, 1);

    // Data shape: (amount: i128, payout_asset: Address)
    let (amount, asset): (i128, Address) = data.into_val(&env);
    assert_eq!(amount, 10_000);
    assert_eq!(asset, payout);
}

// ── rv_rej ────────────────────────────────────────────────────────────────────

/// Pins the topic structure and data shape for `rv_rej` (duplicate report rejected).
#[test]
fn event_indexed_v2_rv_rej_topic_and_data_shape() {
    let (env, client, issuer, ns, token, payout) = setup();
    client.report_revenue(&issuer, &ns, &token, &payout, &10_000, &1, &false);
    let before = env.events().all().len();
    // Same period_id + override_existing=false → rv_rej
    client.report_revenue(&issuer, &ns, &token, &payout, &20_000, &1, &false);

    let (topic, data) = find_indexed_v2(&env, symbol_short!("rv_rej"), before as u32)
        .expect("rv_rej EVENT_INDEXED_V2 must be emitted on duplicate report");

    assert_eq!(topic.version, 2);
    assert_eq!(topic.event_type, symbol_short!("rv_rej"));
    assert_eq!(topic.issuer, issuer);
    assert_eq!(topic.namespace, ns);
    assert_eq!(topic.token, token);
    assert_eq!(topic.period_id, 1);

    // Data shape: (amount: i128, existing_amount: i128, payout_asset: Address)
    let (amount, existing, asset): (i128, i128, Address) = data.into_val(&env);
    assert_eq!(amount, 20_000);
    assert_eq!(existing, 10_000);
    assert_eq!(asset, payout);
}

// ── rv_ovr ────────────────────────────────────────────────────────────────────

/// Pins the topic structure and data shape for `rv_ovr` (override/correction).
#[test]
fn event_indexed_v2_rv_ovr_topic_and_data_shape() {
    let (env, client, issuer, ns, token, payout) = setup();
    client.report_revenue(&issuer, &ns, &token, &payout, &10_000, &1, &false);
    let before = env.events().all().len();
    // override_existing=true → rv_ovr
    client.report_revenue(&issuer, &ns, &token, &payout, &15_000, &1, &true);

    let (topic, data) = find_indexed_v2(&env, symbol_short!("rv_ovr"), before as u32)
        .expect("rv_ovr EVENT_INDEXED_V2 must be emitted on override");

    assert_eq!(topic.version, 2);
    assert_eq!(topic.event_type, symbol_short!("rv_ovr"));
    assert_eq!(topic.issuer, issuer);
    assert_eq!(topic.namespace, ns);
    assert_eq!(topic.token, token);
    assert_eq!(topic.period_id, 1);

    // Data shape: (amount: i128, existing_amount: i128, payout_asset: Address)
    let (amount, existing, asset): (i128, i128, Address) = data.into_val(&env);
    assert_eq!(amount, 15_000);
    assert_eq!(existing, 10_000);
    assert_eq!(asset, payout);
}

// ── rv_rep ────────────────────────────────────────────────────────────────────

/// Pins the topic structure and data shape for `rv_rep` (unconditional report receipt).
#[test]
fn event_indexed_v2_rv_rep_topic_and_data_shape() {
    let (env, client, issuer, ns, token, payout) = setup();
    let before = env.events().all().len();
    client.report_revenue(&issuer, &ns, &token, &payout, &10_000, &1, &false);

    let (topic, data) = find_indexed_v2(&env, symbol_short!("rv_rep"), before as u32)
        .expect("rv_rep EVENT_INDEXED_V2 must be emitted unconditionally");

    assert_eq!(topic.version, 2);
    assert_eq!(topic.event_type, symbol_short!("rv_rep"));
    assert_eq!(topic.issuer, issuer);
    assert_eq!(topic.namespace, ns);
    assert_eq!(topic.token, token);
    assert_eq!(topic.period_id, 1);

    // Data shape: (amount: i128, payout_asset: Address, actual_override: bool)
    let (amount, asset, actual_override): (i128, Address, bool) = data.into_val(&env);
    assert_eq!(amount, 10_000);
    assert_eq!(asset, payout);
    assert!(!actual_override); // initial report, not an override
}

/// `actual_override` flag is `true` when the report corrects an existing period.
#[test]
fn event_indexed_v2_rv_rep_actual_override_true_on_correction() {
    let (env, client, issuer, ns, token, payout) = setup();
    client.report_revenue(&issuer, &ns, &token, &payout, &10_000, &1, &false);
    let before = env.events().all().len();
    client.report_revenue(&issuer, &ns, &token, &payout, &15_000, &1, &true);

    let (_, data) = find_indexed_v2(&env, symbol_short!("rv_rep"), before as u32).unwrap();
    let (_, _, actual_override): (i128, Address, bool) = data.into_val(&env);
    assert!(actual_override);
}

// ── claim ─────────────────────────────────────────────────────────────────────

/// Pins the topic structure and data shape for `claim`.
/// `period_id` must always be 0 (claim is not period-scoped).
#[test]
fn event_indexed_v2_claim_topic_and_data_shape() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("test");
    let token = Address::generate(&env);
    let payout = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(&env, &payout).mint(&issuer, &1_000_000);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &ns, &token, &2500, &payout, &0);

    let holder = Address::generate(&env);
    client.deposit_revenue(&issuer, &ns, &token, &payout, &100_000, &1);
    client.set_holder_share(&issuer, &ns, &token, &holder, &5_000); // 50%
    let before = env.events().all().len();
    client.claim(&holder, &issuer, &ns, &token, &10);

    let (topic, data) = find_indexed_v2(&env, symbol_short!("claim"), before as u32)
        .expect("claim EVENT_INDEXED_V2 must be emitted");

    assert_eq!(topic.version, 2);
    assert_eq!(topic.event_type, symbol_short!("claim"));
    assert_eq!(topic.issuer, issuer);
    assert_eq!(topic.namespace, ns);
    assert_eq!(topic.token, token);
    assert_eq!(topic.period_id, 0); // Security: claim is not period-scoped

    // Data shape: (total_payout: i128,) — single-element tuple
    let (total_payout,): (i128,) = data.into_val(&env);
    assert!(total_payout > 0);
}

/// claim `period_id` must be 0 even when multiple periods are claimed.
#[test]
fn event_indexed_v2_claim_period_id_always_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("test");
    let token = Address::generate(&env);
    let payout = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(&env, &payout).mint(&issuer, &1_000_000);

    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &ns, &token, &2500, &payout, &0);

    let holder = Address::generate(&env);
    client.deposit_revenue(&issuer, &ns, &token, &payout, &100_000, &1);
    client.deposit_revenue(&issuer, &ns, &token, &payout, &200_000, &2);
    client.set_holder_share(&issuer, &ns, &token, &holder, &5_000);
    let before = env.events().all().len();
    client.claim(&holder, &issuer, &ns, &token, &10);

    let (topic, _) = find_indexed_v2(&env, symbol_short!("claim"), before as u32).unwrap();
    assert_eq!(topic.period_id, 0);
}

// ── payout_asset variations ───────────────────────────────────────────────────

/// Different offerings with different payout assets emit the correct asset in data.
#[test]
fn event_indexed_v2_payout_asset_bound_correctly_per_offering() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, RevoraRevenueShare);
    let client = RevoraRevenueShareClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let ns = symbol_short!("test");
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout_a = Address::generate(&env);
    let payout_b = Address::generate(&env);
    client.initialize(&admin, &None::<Address>, &None::<bool>);
    client.register_offering(&issuer, &ns, &token_a, &2500, &payout_a, &0);
    client.register_offering(&issuer, &ns, &token_b, &2500, &payout_b, &0);

    let before_a = env.events().all().len();
    client.report_revenue(&issuer, &ns, &token_a, &payout_a, &10_000, &1, &false);
    let (_, data_a) = find_indexed_v2(&env, symbol_short!("rv_init"), before_a as u32).unwrap();
    let (_, asset_a): (i128, Address) = data_a.into_val(&env);
    assert_eq!(asset_a, payout_a);

    let before_b = env.events().all().len();
    client.report_revenue(&issuer, &ns, &token_b, &payout_b, &20_000, &1, &false);
    let (_, data_b) = find_indexed_v2(&env, symbol_short!("rv_init"), before_b as u32).unwrap();
    let (_, asset_b): (i128, Address) = data_b.into_val(&env);
    assert_eq!(asset_b, payout_b);
}

// ── Schema version guard ──────────────────────────────────────────────────────

/// Every EVENT_INDEXED_V2 event must carry version=2.
/// Guards against accidental version bump breaking all indexers.
#[test]
fn event_indexed_v2_version_field_always_2() {
    let (env, client, issuer, ns, token, payout) = setup();
    let before = env.events().all().len();
    client.report_revenue(&issuer, &ns, &token, &payout, &10_000, &1, &false);

    let ev_idx2 = symbol_short!("ev_idx2");
    let all = env.events().all();
    let mut count = 0u32;
    for i in before as u32..all.len() {
        let (_, topics, _) = all.get(i).unwrap();
        if topics.len() >= 2 {
            let t0: Symbol = topics.get(0).unwrap().into_val(&env);
            if t0 == ev_idx2 {
                let t: EventIndexTopicV2 = topics.get(1).unwrap().into_val(&env);
                assert_eq!(t.version, 2, "version must be 2 on all EVENT_INDEXED_V2 events");
                count += 1;
            }
        }
    }
    assert!(count >= 2, "expected at least rv_init + rv_rep indexed events");
}
